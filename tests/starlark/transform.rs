use coralogix_aws_shipper::logs::transform::{StarlarkError, StarlarkTransformer};
use proptest::prelude::*;
use serde_json;

// =============================================================================
// Happy Path
// =============================================================================

#[test]
fn test_starlark_simple_passthrough() {
    let script = include_str!("../fixtures/starlark/passthrough.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/simple_passthrough.log").trim();
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("hello"));
}

#[test]
fn test_starlark_unnest_array() {
    let script = include_str!("../fixtures/starlark/unnest.star");
    let transformer = StarlarkTransformer::new(script).unwrap();

    let input = include_str!("../fixtures/starlark/unnest_array.log").trim();
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 3);
    assert!(result[0].contains("log1"));
    assert!(result[1].contains("log2"));
    assert!(result[2].contains("log3"));
}

#[test]
fn test_starlark_filter_logs() {
    let script = include_str!("../fixtures/starlark/filter_logs.star");
    let transformer = StarlarkTransformer::new(script).unwrap();

    let lines: Vec<&str> = include_str!("../fixtures/starlark/filter_logs.log").lines().collect();
    let debug_log = lines[0].trim();
    let result = transformer.transform(debug_log).unwrap();
    assert_eq!(result.len(), 0);

    let info_log = lines[1].trim();
    let result = transformer.transform(info_log).unwrap();
    assert_eq!(result.len(), 1);
}

#[test]
fn test_starlark_enrich() {
    let script = include_str!("../fixtures/starlark/enrich.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/enrich.log").trim();
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("processed"));
    assert!(result[0].contains("aws-shipper"));
}

// =============================================================================
// Contract
// =============================================================================

#[test]
fn test_starlark_missing_transform_function() {
    let script = include_str!("../fixtures/starlark/missing_transform.star");
    let result = StarlarkTransformer::new(script);
    assert!(matches!(result, Err(StarlarkError::TransformFunctionNotFound)));
}

#[test]
fn test_starlark_non_json_passthrough() {
    let script = include_str!("../fixtures/starlark/passthrough.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/non_json_passthrough.log").trim();
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "plain text log message");
}

#[test]
fn test_starlark_batch_transform() {
    let script = include_str!("../fixtures/starlark/unnest.star");
    let transformer = StarlarkTransformer::new(script).unwrap();

    let logs: Vec<String> = include_str!("../fixtures/starlark/batch_transform.log")
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let result = transformer.transform_batch(logs).unwrap();
    assert_eq!(result.len(), 3); // 2 from first + 1 from second
}

// =============================================================================
// Built-in Functions
// =============================================================================

#[test]
fn test_starlark_parse_json_builtin() {
    let script = include_str!("../fixtures/starlark/parse_json_builtin.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/parse_json_builtin.log").trim();
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.get("parsed").is_some());
    assert_eq!(parsed["parsed"]["key"], "value");
}

#[test]
fn test_starlark_to_json_builtin() {
    let script = include_str!("../fixtures/starlark/to_json_builtin.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/to_json_builtin.log").trim();
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.get("serialized").is_some());
    let serialized: serde_json::Value = serde_json::from_str(parsed["serialized"].as_str().unwrap()).unwrap();
    assert_eq!(serialized["key"], "value");
    assert_eq!(serialized["num"], 42);
    assert_eq!(serialized["bool"], true);
}

#[test]
fn test_starlark_print_builtin() {
    let script = include_str!("../fixtures/starlark/print_builtin.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/print_builtin.log").trim();
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 1);
}

#[test]
fn test_starlark_parse_json_invalid() {
    let script = include_str!("../fixtures/starlark/parse_json_invalid.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{"msg": "test"}"#);
    assert!(result.is_err());
    assert!(matches!(result, Err(StarlarkError::EvalError(_))));
}

// =============================================================================
// Unhappy Path - Runtime
// =============================================================================

#[test]
fn test_starlark_batch_one_fails() {
    let script = include_str!("../fixtures/starlark/batch_one_fails.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let failing_log = r#"{"fail": true, "msg": "bad"}"#.to_string();
    let ok_log = r#"{"msg": "ok"}"#.to_string();
    let logs = vec![failing_log.clone(), ok_log];
    let result = transformer.transform_batch(logs).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0], failing_log);
    assert!(result[1].contains("processed"));
    assert!(result[1].contains("ok"));
}

#[test]
fn test_starlark_helpers() {
    let script = include_str!("../fixtures/starlark/helpers.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let lines: Vec<&str> = include_str!("../fixtures/starlark/helpers.log").lines().collect();
    let result = transformer.transform(lines[0].trim()).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert_eq!(parsed["flagged"], true);

    let result2 = transformer.transform(lines[1].trim()).unwrap();
    let parsed2: serde_json::Value = serde_json::from_str(&result2[0]).unwrap();
    assert!(parsed2.get("flagged").is_none());
}

#[test]
fn test_starlark_numeric_precision() {
    let script = include_str!("../fixtures/starlark/passthrough.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    
    // Test epoch milliseconds (large i64 value)
    let epoch_ms = 1706745600000i64;
    let input = format!(r#"{{"timestamp": {}}}"#, epoch_ms);
    let result = transformer.transform(&input).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.get("timestamp").is_some());
    assert_eq!(parsed["timestamp"].as_i64(), Some(epoch_ms));
    
    // Test large i64 value (near i32::MAX)
    let large_num = i64::from(i32::MAX) + 1000;
    let input = format!(r#"{{"large_num": {}}}"#, large_num);
    let result = transformer.transform(&input).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.get("large_num").is_some());
    assert_eq!(parsed["large_num"].as_i64(), Some(large_num));
    
    // Test negative large values
    let negative_large = -1706745600000i64;
    let input = format!(r#"{{"negative": {}}}"#, negative_large);
    let result = transformer.transform(&input).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert_eq!(parsed["negative"].as_i64(), Some(negative_large));
}

#[test]
fn test_starlark_float_precision() {
    let script = include_str!("../fixtures/starlark/passthrough.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/float_precision.log").trim();
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    
    // Verify floats remain as numbers (not strings)
    assert!(parsed.get("pi").is_some());
    assert!(parsed["pi"].is_f64());
    assert_eq!(parsed["pi"].as_f64(), Some(3.141592653589793));
    
    assert!(parsed.get("small").is_some());
    assert!(parsed["small"].is_f64());
    assert_eq!(parsed["small"].as_f64(), Some(0.0000001));
}

// =============================================================================
// Return Type Enforcement Tests
// =============================================================================

#[test]
fn test_transform_must_return_list_none() {
    let script = include_str!("../fixtures/starlark/return_none.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/return_list.log").trim();
    let result = transformer.transform(input);
    assert!(result.is_err());
    assert!(matches!(result, Err(StarlarkError::InvalidReturnType(_))));
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("must return a list"));
    assert!(error_msg.contains("NoneType") || error_msg.contains("none"));
}

#[test]
fn test_transform_must_return_list_string() {
    let script = include_str!("../fixtures/starlark/return_string.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/return_list.log").trim();
    let result = transformer.transform(input);
    assert!(result.is_err());
    assert!(matches!(result, Err(StarlarkError::InvalidReturnType(_))));
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("must return a list"));
    assert!(error_msg.contains("string"));
}

#[test]
fn test_transform_must_return_list_dict() {
    let script = include_str!("../fixtures/starlark/return_dict.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/return_list.log").trim();
    let result = transformer.transform(input);
    assert!(result.is_err());
    assert!(matches!(result, Err(StarlarkError::InvalidReturnType(_))));
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("must return a list"));
    assert!(error_msg.contains("dict"));
}

#[test]
fn test_transform_must_return_list_int() {
    let script = include_str!("../fixtures/starlark/return_int.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/return_list.log").trim();
    let result = transformer.transform(input);
    assert!(result.is_err());
    assert!(matches!(result, Err(StarlarkError::InvalidReturnType(_))));
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("must return a list"));
}

#[test]
fn test_transform_must_return_list_bool() {
    let script = include_str!("../fixtures/starlark/return_bool.star");
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = include_str!("../fixtures/starlark/return_list.log").trim();
    let result = transformer.transform(input);
    assert!(result.is_err());
    assert!(matches!(result, Err(StarlarkError::InvalidReturnType(_))));
    let error_msg = format!("{}", result.unwrap_err());
    assert!(error_msg.contains("must return a list"));
}

// =============================================================================
// Property-Based Tests
// =============================================================================

proptest! {
    #[test]
    fn passthrough_preserves_valid_json(json in prop::collection::hash_map("[a-z]+", 0i32..100, 0..10)) {
        let script = include_str!("../fixtures/starlark/passthrough.star");
        let transformer = StarlarkTransformer::new(script).unwrap();
        let input = serde_json::to_string(&json).unwrap();
        let result = transformer.transform(&input).unwrap();
        assert_eq!(result.len(), 1);
        let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
        assert_eq!(parsed.as_object().unwrap().len(), json.len());
    }

    #[test]
    fn batch_transform_handles_any_size(size in 0usize..1000) {
        let script = include_str!("../fixtures/starlark/passthrough.star");
        let transformer = StarlarkTransformer::new(script).unwrap();
        let logs: Vec<String> = (0..size)
            .map(|i| format!(r#"{{"id": {}}}"#, i))
            .collect();

        let result = transformer.transform_batch(logs.clone()).unwrap();
        assert_eq!(result.len(), size);
    }
}
