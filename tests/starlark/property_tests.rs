use coralogix_aws_shipper::logs::transform::StarlarkTransformer;
use proptest::prelude::*;

// =============================================================================
// Property-Based Tests (CDS-2349)
// =============================================================================

proptest! {
    #[test]
    fn json_roundtrip_doesnt_crash(s in "\\PC*") {
        let script = r#"
def transform(event):
    return [event]
"#;
        let transformer = StarlarkTransformer::new(script).unwrap();
        // Should not panic, even with arbitrary input
        let _ = transformer.transform(&s);
    }

    #[test]
    fn passthrough_preserves_valid_json(json in prop::collection::hash_map("[a-z]+", 0i32..100, 0..10)) {
        let script = r#"
def transform(event):
    return [event]
"#;
        let transformer = StarlarkTransformer::new(script).unwrap();
        let input = serde_json::to_string(&json).unwrap();
        let result = transformer.transform(&input).unwrap();
        assert_eq!(result.len(), 1);
        // Parse back - should work
        let parsed: serde_json::Value = serde_json::from_str(&result[0]).unwrap();
        // Should have same keys (though order may differ)
        assert_eq!(parsed.as_object().unwrap().len(), json.len());
    }

    #[test]
    fn transform_never_panics(
        script in r#"[a-zA-Z0-9\s\n:()\[\]{},=+\-*/"']*"#,
        input in r#"[a-zA-Z0-9\s\n:()\[\]{},=+\-*/"']*"#
    ) {
        // Try to compile script - may fail, but shouldn't panic
        if let Ok(transformer) = StarlarkTransformer::new(&script) {
            // If compilation succeeded, transform shouldn't panic
            let _ = transformer.transform(&input);
        }
    }

    #[test]
    fn batch_transform_handles_any_size(size in 0usize..1000) {
        let script = r#"
def transform(event):
    return [event]
"#;
        let transformer = StarlarkTransformer::new(script).unwrap();
        let logs: Vec<String> = (0..size)
            .map(|i| format!(r#"{{"id": {}}}"#, i))
            .collect();
        
        let result = transformer.transform_batch(logs.clone()).unwrap();
        assert_eq!(result.len(), size);
    }
}
