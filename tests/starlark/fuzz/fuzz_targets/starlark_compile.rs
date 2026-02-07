#![no_main]
use libfuzzer_sys::fuzz_target;
use coralogix_aws_shipper::logs::transform::StarlarkTransformer;

fuzz_target!(|data: &[u8]| {
    // Attempt to compile arbitrary input as Starlark script
    // Should never panic, even with malformed input
    if let Ok(script) = std::str::from_utf8(data) {
        let _ = StarlarkTransformer::new(script);
    }
});
