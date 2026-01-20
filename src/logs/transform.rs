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
use starlark::collections::SmallMap;
use starlark::environment::{FrozenModule, Globals, GlobalsBuilder, Module};
use starlark::eval::Evaluator;
use starlark::starlark_module;
use starlark::syntax::{AstModule, Dialect};
use starlark::values::dict::DictRef;
use starlark::values::list::ListRef;
use starlark::values::{Heap, Value};
use tracing::{debug, error, info, warn};

// ============================================================================
// Public API
// ============================================================================

/// Apply log transformation based on config.
///
/// If `config.starlark_script` is set, compiles and applies the transformation.
/// Otherwise, returns the logs unchanged.
///
/// This is the main entry point for the transformation pipeline.
pub fn transform_logs(logs: Vec<String>, config: &Config) -> Result<Vec<String>, TransformError> {
    let Some(ref script) = config.starlark_script else {
        return Ok(logs);
    };

    let transformer = StarlarkTransformer::new(script)?;

    info!("Applying Starlark transformation to {} logs", logs.len());
    let transformed = transformer.transform_batch(logs)?;
    info!(
        "Starlark transformation complete: {} logs after transformation",
        transformed.len()
    );

    Ok(transformed)
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

    /// Log a debug message (useful for script debugging)
    fn log_debug(msg: &str) -> anyhow::Result<starlark::values::none::NoneType> {
        debug!("[Starlark] {}", msg);
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
                Ok(heap.alloc(i as i32))
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

/// Convert a Starlark result (expected to be a list) to JSON strings
fn starlark_to_json_strings(value: Value) -> Result<Vec<String>, TransformError> {
    if let Some(list) = ListRef::from_value(value) {
        let mut results = Vec::new();
        for item in list.iter() {
            let json = starlark_to_json(item).map_err(TransformError::ConversionError)?;
            results.push(
                serde_json::to_string(&json)
                    .map_err(|e| TransformError::ConversionError(e.to_string()))?,
            );
        }
        Ok(results)
    } else {
        // If not a list, wrap in a list
        let json = starlark_to_json(value).map_err(TransformError::ConversionError)?;
        Ok(vec![serde_json::to_string(&json)
            .map_err(|e| TransformError::ConversionError(e.to_string()))?])
    }
}
