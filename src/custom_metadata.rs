//! Shared parsing for `CUSTOM_METADATA` (comma-separated `key=value` pairs), used by logs and metrics.

use std::collections::HashMap;
use tracing::error;

/// Parse `CUSTOM_METADATA` in the same format as logs ingestion: comma-separated pairs, each `key=value`.
/// Values may contain `=`; only the first `=` splits key from value. Empty pairs are skipped.
pub fn parse_custom_metadata_str(s: &str) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    for pair in s.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some((key, value)) = pair.split_once('=') {
            let key = key.trim();
            if key.is_empty() {
                error!("CUSTOM_METADATA pair has empty key: {:?}", pair);
                continue;
            }
            metadata.insert(key.to_string(), value.trim().to_string());
        } else {
            error!("Failed to split key-value pair (expected key=value): {}", pair);
        }
    }
    metadata
}

/// Load custom metadata from `CUSTOM_METADATA` env when set and non-empty.
pub fn custom_metadata_from_env() -> HashMap<String, String> {
    match std::env::var("CUSTOM_METADATA") {
        Ok(s) if !s.trim().is_empty() => parse_custom_metadata_str(&s),
        _ => HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multiple_pairs() {
        let m = parse_custom_metadata_str("a=1,b=two, c = spaced ");
        assert_eq!(m.get("a").map(String::as_str), Some("1"));
        assert_eq!(m.get("b").map(String::as_str), Some("two"));
        assert_eq!(m.get("c").map(String::as_str), Some("spaced"));
    }

    #[test]
    fn value_may_contain_equals() {
        let m = parse_custom_metadata_str("k=a=b");
        assert_eq!(m.get("k").map(String::as_str), Some("a=b"));
    }
}
