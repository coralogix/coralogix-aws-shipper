use coralogix_aws_shipper::logs::transform::{StarlarkError, StarlarkTransformer};
use serde_json;

// =============================================================================
// Starlark Transformation Tests
// =============================================================================

#[test]
fn test_starlark_simple_passthrough() {
    let script = r#"
def transform(event):
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{"msg": "hello"}"#).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("hello"));
}

#[test]
fn test_starlark_unnest_array() {
    let script = r#"
def transform(event):
    if "logs" in event and type(event["logs"]) == "list":
        return event["logs"]
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();

    // Test with nested array
    let input = r#"{"logs": [{"msg": "log1"}, {"msg": "log2"}, {"msg": "log3"}]}"#;
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 3);
    assert!(result[0].contains("log1"));
    assert!(result[1].contains("log2"));
    assert!(result[2].contains("log3"));
}

#[test]
fn test_starlark_filter_logs() {
    let script = r#"
def transform(event):
    if event.get("level") == "DEBUG":
        return []  # Filter out debug logs
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();

    // Debug log should be filtered
    let debug_log = r#"{"level": "DEBUG", "msg": "debug message"}"#;
    let result = transformer.transform(debug_log).unwrap();
    assert_eq!(result.len(), 0);

    // Info log should pass through
    let info_log = r#"{"level": "INFO", "msg": "info message"}"#;
    let result = transformer.transform(info_log).unwrap();
    assert_eq!(result.len(), 1);
}

#[test]
fn test_starlark_transform_and_enrich() {
    let script = r#"
def transform(event):
    event["processed"] = True
    event["source"] = "aws-shipper"
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{"msg": "hello"}"#).unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("processed"));
    assert!(result[0].contains("aws-shipper"));
}

#[test]
fn test_starlark_missing_transform_function() {
    let script = r#"
def process(event):
    return [event]
"#;
    let result = StarlarkTransformer::new(script);
    assert!(matches!(result, Err(StarlarkError::TransformFunctionNotFound)));
}

#[test]
fn test_starlark_syntax_error() {
    let script = r#"
def transform(event)  # Missing colon
    return [event]
"#;
    let result = StarlarkTransformer::new(script);
    assert!(matches!(result, Err(StarlarkError::ParseError(_))));
}

#[test]
fn test_starlark_non_json_passthrough() {
    let script = r#"
def transform(event):
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    // Non-JSON input should be wrapped as a string
    let result = transformer.transform("plain text log message").unwrap();
    assert_eq!(result.len(), 1);
    assert!(result[0].contains("plain text log message"));
}

#[test]
fn test_starlark_batch_transform() {
    let script = r#"
def transform(event):
    if "logs" in event and type(event["logs"]) == "list":
        return event["logs"]
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();

    let logs = vec![
        r#"{"logs": [{"a": 1}, {"b": 2}]}"#.to_string(),
        r#"{"msg": "standalone"}"#.to_string(),
    ];

    let result = transformer.transform_batch(logs).unwrap();
    assert_eq!(result.len(), 3); // 2 from first + 1 from second
}

#[test]
fn test_starlark_demo_unnest_with_output() {
    let script = r#"
def transform(event):
    if "logs" in event and type(event["logs"]) == "list":
        return event["logs"]
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();

    // Simulate the batched log from test-event.json (demo-004-batch)
    let batched_input = r#"{"logs": [{"level": "INFO", "message": "Batch log entry 1", "timestamp": "2026-01-19T09:00:00Z"}, {"level": "DEBUG", "message": "Batch log entry 2", "timestamp": "2026-01-19T09:00:01Z"}, {"level": "ERROR", "message": "Batch log entry 3 - Something went wrong!", "timestamp": "2026-01-19T09:00:02Z"}]}"#;

    println!("\n========== STARLARK TRANSFORMATION DEMO ==========");
    println!("\n📥 INPUT (1 batched JSON with nested 'logs' array):");
    println!("{}", batched_input);

    let result = transformer.transform(batched_input).unwrap();

    println!("\n📤 OUTPUT ({} individual log entries):", result.len());
    for (i, log) in result.iter().enumerate() {
        let parsed: serde_json::Value = serde_json::from_str(log).unwrap();
        println!(
            "  [{}] {}",
            i + 1,
            serde_json::to_string_pretty(&parsed)
                .unwrap()
                .replace('\n', "\n      ")
        );
    }
    println!("\n===================================================\n");

    assert_eq!(result.len(), 3);
}

// =============================================================================
// Built-in Function Tests (CDS-2349)
// =============================================================================

#[test]
fn test_parse_json_builtin() {
    let script = r#"
def transform(event):
    parsed = parse_json('{"nested": [1, 2, 3], "key": "value"}')
    return [{"original": event, "parsed": parsed}]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{"msg": "test"}"#).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.get("parsed").is_some());
    assert_eq!(parsed["parsed"]["key"], "value");
}

#[test]
fn test_to_json_builtin() {
    let script = r#"
def transform(event):
    json_str = to_json({"key": "value", "num": 42, "bool": True})
    return [{"serialized": json_str}]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{}"#).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.get("serialized").is_some());
    let serialized: serde_json::Value = serde_json::from_str(parsed["serialized"].as_str().unwrap()).unwrap();
    assert_eq!(serialized["key"], "value");
    assert_eq!(serialized["num"], 42);
    assert_eq!(serialized["bool"], true);
}

#[test]
fn test_log_debug_builtin() {
    let script = r#"
def transform(event):
    log_debug("Debug message from script")
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{"msg": "test"}"#).unwrap();
    assert_eq!(result.len(), 1);
}

// =============================================================================
// Edge Case Tests (CDS-2349)
// =============================================================================

#[test]
fn test_deep_nesting() {
    let script = r#"
def transform(event):
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let deep = r#"{"a":{"b":{"c":{"d":{"e":{"f":{"g":"deep"}}}}}}}"#;
    let result = transformer.transform(deep).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert_eq!(parsed["a"]["b"]["c"]["d"]["e"]["f"]["g"], "deep");
}

#[test]
fn test_large_array_unnest() {
    let script = r#"
def transform(event):
    if "items" in event:
        return event["items"]
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let items: Vec<_> = (0..1000).map(|i| format!(r#"{{"id":{}}}"#, i)).collect();
    let input = format!(r#"{{"items":[{}]}}"#, items.join(","));
    let result = transformer.transform(&input).unwrap();
    assert_eq!(result.len(), 1000);
    let first: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert_eq!(first["id"], 0);
    let last: serde_json::Value = serde_json::from_str(&result[999]).unwrap();
    assert_eq!(last["id"], 999);
}

#[test]
fn test_script_with_helpers() {
    let script = r#"
def is_important(event):
    return event.get("severity", "INFO") in ["ERROR", "CRITICAL"]

def transform(event):
    if is_important(event):
        event["flagged"] = True
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{"severity": "ERROR", "msg": "critical"}"#).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert_eq!(parsed["flagged"], true);
    
    let result2 = transformer.transform(r#"{"severity": "INFO", "msg": "normal"}"#).unwrap();
    let parsed2: serde_json::Value = serde_json::from_str(&result2[0]).unwrap();
    assert!(parsed2.get("flagged").is_none());
}

#[test]
fn test_unicode_handling() {
    let script = r#"
def transform(event):
    event["unicode_test"] = "测试 🚀 émoji"
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{"msg": "hello"}"#).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert_eq!(parsed["unicode_test"], "测试 🚀 émoji");
}

#[test]
fn test_empty_input() {
    let script = r#"
def transform(event):
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform("{}").unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.as_object().unwrap().is_empty());
}

#[test]
fn test_numeric_precision_i64() {
    let script = r#"
def transform(event):
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    // Test large i64 value (near i32::MAX)
    let large_num = i64::from(i32::MAX) + 1000;
    let input = format!(r#"{{"large_num": {}}}"#, large_num);
    let result = transformer.transform(&input).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    // Note: Starlark uses i32, so large i64 values may be converted
    assert!(parsed.get("large_num").is_some());
}

#[test]
fn test_float_precision() {
    let script = r#"
def transform(event):
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = r#"{"pi": 3.141592653589793, "small": 0.0000001}"#;
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.get("pi").is_some());
    assert!(parsed.get("small").is_some());
}

#[test]
fn test_special_characters_in_strings() {
    let script = r#"
def transform(event):
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let input = r#"{"special": "line1\nline2\ttab\"quote\\backslash"}"#;
    let result = transformer.transform(input).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.get("special").is_some());
}

#[test]
fn test_null_values() {
    let script = r#"
def transform(event):
    event["null_field"] = None
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{"msg": "test"}"#).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert!(parsed.get("null_field").is_some());
    assert!(parsed["null_field"].is_null());
}

#[test]
fn test_boolean_values() {
    let script = r#"
def transform(event):
    event["true_val"] = True
    event["false_val"] = False
    return [event]
"#;
    let transformer = StarlarkTransformer::new(script).unwrap();
    let result = transformer.transform(r#"{}"#).unwrap();
    assert_eq!(result.len(), 1);
    let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
    assert_eq!(parsed["true_val"], true);
    assert_eq!(parsed["false_val"], false);
}
