use serde_json::Value;
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogSeverity {
    Verbose,
    Debug,
    Info,
    Warn,
    Error,
    Critical,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProcessedLog {
    pub application_name: String,
    pub subsystem_name: String,
    pub body: Value,
    pub severity: LogSeverity,
    pub timestamp: OffsetDateTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processed_log_is_protocol_neutral() {
        let timestamp = OffsetDateTime::UNIX_EPOCH;
        let log = ProcessedLog {
            application_name: "app".to_string(),
            subsystem_name: "sub".to_string(),
            body: serde_json::json!({"message": "hello"}),
            severity: LogSeverity::Info,
            timestamp,
        };

        assert_eq!(log.severity, LogSeverity::Info);
        assert_eq!(log.timestamp, timestamp);
    }
}
