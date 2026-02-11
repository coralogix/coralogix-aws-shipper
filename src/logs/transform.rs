//! Log transformation module.
//!
//! This module provides the ability to transform logs using Starlark scripts,
//! enabling users to write custom parsing and transformation logic including:
//! - Unnesting JSON arrays into individual log entries
//! - Filtering logs based on custom conditions
//! - Transforming log structure before sending to Coralogix
//!
//! # Configuration
//!
//! Set the `STARLARK_SCRIPT` environment variable with your transformation script.
//! When not set, logs pass through unchanged.
//!
//! # Example Starlark Script
//!
//! ```starlark
//! def transform(event):
//!     """Transform a single log event. Return a list of events."""
//!     if "logs" in event and type(event["logs"]) == "list":
//!         # Unnest the logs array
//!         return event["logs"]
//!     return [event]
//! ```

use crate::logs::config::Config;
use aws_config::SdkConfig;
use starlark::collections::SmallMap;
use starlark::environment::{FrozenModule, Globals, GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::dict::DictRef;
use starlark::values::float::StarlarkFloat;
use starlark::values::list::ListRef;
use starlark::values::{Heap, Value, ValueLike};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

// ============================================================================
// Caching
// ============================================================================

/// Cached transformer - resolved and compiled once per Lambda container.
/// This avoids repeated S3/HTTP fetches and recompilation for record-by-record
/// event sources (Kinesis, SQS) where `process_batches()` is called multiple times.
/// Uses Mutex to allow test-only reset via reset_cache().
static CACHED_TRANSFORMER: Mutex<Option<Option<StarlarkTransformer>>> = Mutex::const_new(None);

// ============================================================================
// Public API
// ============================================================================

/// Apply log transformation based on config.
///
/// Resolves the Starlark script from S3, URL, Base64, or raw script (in that priority order).
/// If a script is found, compiles and applies the transformation.
/// Otherwise, returns the logs unchanged.
///
/// The resolved and compiled transformer is cached per Lambda container to avoid
/// repeated S3/HTTP fetches and recompilation for record-by-record event sources.
///
/// This is the main entry point for the transformation pipeline.
pub async fn transform_logs(
    logs: Vec<String>,
    config: &Config,
    aws_config: &SdkConfig,
) -> Result<Vec<String>, TransformError> {
    // Get or initialize the cached transformer
    {
        let guard = CACHED_TRANSFORMER.lock().await;
        if guard.is_none() {
            drop(guard);
            // Resolve script from any configured source (must release lock before await)
            let resolved_script = match config.resolve_starlark_script(aws_config).await {
                Ok(s) => s,
                Err(e) => {
                    warn!(
                        "Starlark script resolution failed, passing through {} logs unchanged: {}",
                        logs.len(),
                        e
                    );
                    return Ok(logs);
                }
            };

            let transformer = match resolved_script {
                Some(script) => match StarlarkTransformer::new(&script) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        warn!(
                            "Starlark script compilation failed, passing through {} logs unchanged: {}",
                            logs.len(),
                            e
                        );
                        return Ok(logs);
                    }
                },
                None => {
                    debug!("No Starlark script configured, caching None");
                    None
                }
            };
            *CACHED_TRANSFORMER.lock().await = Some(transformer);
        }
    }

    let guard = CACHED_TRANSFORMER.lock().await;
    let transformer = guard.as_ref().unwrap();

    let Some(transformer) = transformer else {
        return Ok(logs);
    };

    info!("Applying Starlark transformation to {} logs", logs.len());
    let transformed = transformer.transform_batch(logs)?;
    info!(
        "Starlark transformation complete: {} logs after transformation",
        transformed.len()
    );

    Ok(transformed)
}

/// Reset the cached transformer. For use in tests only, to ensure test isolation
/// when different tests need different STARLARK_SCRIPT configuration.
#[doc(hidden)]
pub async fn reset_cache() {
    *CACHED_TRANSFORMER.lock().await = None;
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during log transformation
#[derive(thiserror::Error, Debug)]
pub enum TransformError {
    #[error("Failed to parse Starlark script: {0}")]
    ParseError(String),
    #[error("Failed to evaluate Starlark script: {0}")]
    EvalError(String),
    #[error("Transform function not found in script. Define: def transform(event): ...")]
    TransformFunctionNotFound,
    #[error("Transform function must return a list: {0}")]
    InvalidReturnType(String),
    #[error("Failed to convert value: {0}")]
    ConversionError(String),
}

// Keep the old name as an alias for backwards compatibility with tests
pub type StarlarkError = TransformError;

// ============================================================================
// Starlark Transformer
// ============================================================================

/// A compiled Starlark transformer that can be reused across invocations
pub struct StarlarkTransformer {
    frozen_module: FrozenModule,
    #[allow(dead_code)]
    globals: Globals,
}

impl StarlarkTransformer {
    /// Create a new StarlarkTransformer from a script string
    pub fn new(script: &str) -> Result<Self, TransformError> {
        debug!("Compiling Starlark transformation script");

        // Parse the script
        let ast = AstModule::parse("transform.star", script.to_owned(), &Dialect::Standard)
            .map_err(|e| TransformError::ParseError(e.to_string()))?;

        // Create globals with built-in functions
        let globals = GlobalsBuilder::standard().with(starlark_extras).build();

        // Evaluate to create the module with the transform function
        let module = Module::new();
        {
            let mut eval = Evaluator::new(&module);
            eval.eval_module(ast, &globals)
                .map_err(|e| TransformError::EvalError(e.to_string()))?;
        }

        // Freeze the module for thread-safe reuse
        let frozen_module = module
            .freeze()
            .map_err(|e| TransformError::EvalError(format!("{:?}", e)))?;

        // Verify transform function exists
        if frozen_module.get("transform").is_err() {
            return Err(TransformError::TransformFunctionNotFound);
        }

        debug!("Starlark transformer compiled successfully");
        Ok(Self {
            frozen_module,
            globals,
        })
    }

    /// Transform a single log entry using the Starlark script
    /// Returns a Vec of transformed log strings (can be 0, 1, or many)
    pub fn transform(&self, log: &str) -> Result<Vec<String>, TransformError> {
        // Parse the input as JSON
        let json_value: serde_json::Value =
            serde_json::from_str(log).unwrap_or_else(|_| serde_json::Value::String(log.to_string()));

        // Create a new module for this evaluation, importing from frozen
        let module = Module::new();
        module.import_public_symbols(&self.frozen_module);

        // Get the transform function
        let transform_fn = self
            .frozen_module
            .get("transform")
            .map_err(|_| TransformError::TransformFunctionNotFound)?;

        let mut eval = Evaluator::new(&module);

        // Convert JSON to Starlark value
        let heap = module.heap();
        let starlark_input =
            json_to_starlark(heap, &json_value).map_err(TransformError::ConversionError)?;

        // Call transform(event)
        let result = eval
            .eval_function(transform_fn.value(), &[starlark_input], &[])
            .map_err(|e| TransformError::EvalError(e.to_string()))?;

        // Convert result back to JSON strings
        starlark_to_json_strings(result)
    }

    /// Transform multiple logs, flattening the results
    pub fn transform_batch(&self, logs: Vec<String>) -> Result<Vec<String>, TransformError> {
        let mut results = Vec::new();
        for log in logs {
            match self.transform(&log) {
                Ok(transformed) => results.extend(transformed),
                Err(e) => {
                    warn!(
                        "Starlark transform failed for log, passing through unchanged: {}",
                        e
                    );
                    results.push(log);
                }
            }
        }
        Ok(results)
    }
}

// ============================================================================
// Starlark Built-in Functions
// ============================================================================

/// Extra built-in functions available in Starlark scripts
#[starlark_module]
fn starlark_extras(builder: &mut GlobalsBuilder) {
    /// Parse a JSON string into a Starlark value
    fn parse_json<'v>(s: &str, heap: &'v Heap) -> anyhow::Result<Value<'v>> {
        let json: serde_json::Value = serde_json::from_str(s)?;
        json_to_starlark(heap, &json).map_err(|e| anyhow::anyhow!(e))
    }

    /// Convert a Starlark value to a JSON string
    fn to_json(v: Value) -> anyhow::Result<String> {
        let json = starlark_to_json(v).map_err(|e| anyhow::anyhow!(e))?;
        Ok(serde_json::to_string(&json)?)
    }

    /// Print a debug message (useful for script debugging)
    /// Accepts any value - strings are printed directly, other values are converted to JSON
    fn print(v: Value) -> anyhow::Result<starlark::values::none::NoneType> {
        // If it's a string, print it directly
        if let Some(s) = v.unpack_str() {
            debug!("[Starlark] {}", s);
        } else {
            // For other types, convert to JSON for readable output
            match starlark_to_json(v) {
                Ok(json) => debug!("[Starlark] {}", json),
                Err(_) => debug!("[Starlark] {}", v.to_repr()),
            }
        }
        Ok(starlark::values::none::NoneType)
    }
}

// ============================================================================
// JSON <-> Starlark Conversion
// ============================================================================

/// Convert a serde_json::Value to a Starlark Value
fn json_to_starlark<'v>(heap: &'v Heap, json: &serde_json::Value) -> Result<Value<'v>, String> {
    match json {
        serde_json::Value::Null => Ok(Value::new_none()),
        serde_json::Value::Bool(b) => Ok(Value::new_bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                // Fits in i64 - common case
                Ok(heap.alloc(i))
            } else if let Some(u) = n.as_u64() {
                // Fits in u64 but not i64 (e.g., values between i64::MAX+1 and u64::MAX)
                Ok(heap.alloc(u))
            } else if let Some(f) = n.as_f64() {
                Ok(heap.alloc(f))
            } else {
                Err(format!("Unsupported number: {}", n))
            }
        }
        serde_json::Value::String(s) => Ok(heap.alloc_str(s).to_value()),
        serde_json::Value::Array(arr) => {
            let items: Result<Vec<Value>, String> =
                arr.iter().map(|v| json_to_starlark(heap, v)).collect();
            Ok(heap.alloc(items?))
        }
        serde_json::Value::Object(obj) => {
            let mut small_map: SmallMap<Value<'v>, Value<'v>> = SmallMap::new();
            for (k, v) in obj {
                let key = heap.alloc_str(k).to_value();
                let val = json_to_starlark(heap, v)?;
                small_map.insert_hashed(key.get_hashed().map_err(|e| e.to_string())?, val);
            }
            Ok(heap.alloc(starlark::values::dict::Dict::new(small_map)))
        }
    }
}

/// Convert a Starlark Value to serde_json::Value
fn starlark_to_json(value: Value) -> Result<serde_json::Value, String> {
    if value.is_none() {
        return Ok(serde_json::Value::Null);
    }

    if let Some(b) = value.unpack_bool() {
        return Ok(serde_json::Value::Bool(b));
    }

    if let Some(i) = value.unpack_i32() {
        return Ok(serde_json::Value::Number(i.into()));
    }

    // Handle floats explicitly to preserve numeric types
    // Check both downcast_ref and type string for robustness
    if let Some(float) = value.downcast_ref::<StarlarkFloat>() {
        let f = float.0;
        if f.is_finite() {
            return serde_json::Number::from_f64(f)
                .map(serde_json::Value::Number)
                .ok_or_else(|| format!("Cannot represent float as JSON number: {}", f));
        } else {
            return Err(format!("Cannot represent {} as JSON number", f));
        }
    }
    // Fallback: check type string in case downcast_ref doesn't work
    if value.get_type() == "float" {
        // Try to_json_value() which should handle floats correctly
        if let Ok(json_val) = value.to_json_value() {
            if let serde_json::Value::Number(_) = json_val {
                return Ok(json_val);
            }
        }
        // If to_json_value doesn't work, try parsing from string representation
        if let Ok(f) = value.to_string().parse::<f64>() {
            if f.is_finite() {
                return serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| format!("Cannot represent float as JSON number: {}", f));
            }
        }
    }

    // Handle large integers (beyond i32 range)
    if value.get_type() == "int" {
        // First try to_json_value() - it may return a Number that preserves precision
        if let Ok(json_val) = value.to_json_value() {
            match json_val {
                serde_json::Value::Number(n) => {
                    // Check if the Number supports as_u64() or as_i64()
                    // If it does, use it directly (preserves precision)
                    if n.as_u64().is_some() || n.as_i64().is_some() {
                        return Ok(serde_json::Value::Number(n));
                    }
                    // Number doesn't preserve precision, reconstruct from string
                }
                serde_json::Value::String(_) => {
                    // Very large integer returned as string, will handle below
                }
                _ => {
                    // Unexpected type, continue with manual handling
                }
            }
        }
        // Reconstruct from string representation to ensure precision
        let int_str = value.to_string();
        // Try to parse as u64 first (handles u64::MAX and positive i64 values)
        if let Ok(parsed_u64) = int_str.parse::<u64>() {
            // Create Number from u64 using Number::from for better precision handling
            return Ok(serde_json::Value::Number(serde_json::Number::from(parsed_u64)));
        }
        // Try i64 for negative numbers
        if let Ok(parsed_i64) = int_str.parse::<i64>() {
            return Ok(serde_json::Value::Number(serde_json::Number::from(parsed_i64)));
        }
        // If parsing fails, use to_json_value() result (will be a string for very large integers)
        if let Ok(json_val) = value.to_json_value() {
            return Ok(json_val);
        }
    }

    if let Some(s) = value.unpack_str() {
        return Ok(serde_json::Value::String(s.to_string()));
    }

    if let Some(list) = ListRef::from_value(value) {
        let items: Result<Vec<serde_json::Value>, String> =
            list.iter().map(starlark_to_json).collect();
        return Ok(serde_json::Value::Array(items?));
    }

    if let Some(dict) = DictRef::from_value(value) {
        let mut map = serde_json::Map::new();
        for (k, v) in dict.iter() {
            let key = k
                .unpack_str()
                .ok_or_else(|| format!("Dict key must be a string, got: {:?}", k))?
                .to_string();
            let val = starlark_to_json(v)?;
            map.insert(key, val);
        }
        return Ok(serde_json::Value::Object(map));
    }

    // Fallback: represent as string
    Ok(serde_json::Value::String(value.to_string()))
}

/// Convert a Starlark result (expected to be a list) to JSON strings.
fn starlark_to_json_strings(value: Value) -> Result<Vec<String>, TransformError> {
    if let Some(list) = ListRef::from_value(value) {
        let mut results = Vec::new();
        for item in list.iter() {
            let json = starlark_to_json(item).map_err(TransformError::ConversionError)?;
            // Return plain strings directly to preserve original log content;
            // only JSON-serialize structured values.
            let serialized = match json {
                serde_json::Value::String(s) => s,
                other => serde_json::to_string(&other)
                    .map_err(|e| TransformError::ConversionError(e.to_string()))?,
            };
            results.push(serialized);
        }
        Ok(results)
    } else {
        Err(TransformError::InvalidReturnType(format!(
            "got {} (type: {}), expected a list. Example: return [event] or return []",
            value, value.get_type()
        )))
    }
}
