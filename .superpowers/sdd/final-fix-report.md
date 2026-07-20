# Final Whole-Branch Review Fix Report

## Status

All requested final-review changes were implemented. The focused suites, full
library suite, logs integration suite, formatting check, Clippy, and whitespace
check pass.

## Decisions and implementation

### Startup and failure observability

- Exporter construction emits one structured startup event with:
  - `protocol`: `coralogix_rest` or `otlp_grpc`
  - `destination_type`: `coralogix` or `collector`
  - `endpoint_authority`: sanitized authority only
- Startup diagnostics never include endpoint paths, query strings, request
  bodies, API keys, or metadata. URI userinfo is stripped defensively even for
  routes where legacy configuration might contain it.
- A final failed OTLP export emits structured `sdk_status`, `grpc_status`,
  `attempt_count`, and `elapsed_ms` fields. The logged status is classification
  and status code only; the server message is not logged.
- Successful OTLP delivery diagnostics now include the same truthful
  per-export `attempt_count`.

### SDK attempt-count investigation

The pinned `cx_sdk_otlp` revision
`52aee3bf014ce18b2ac25771ecefbfd25e73bb11` supports
`OtlpExporterGrpcBuilder::listener`. Its `RequestListener` is invoked from
inside the SDK's `backoff::future::retry` closure after every actual gRPC
attempt, including SDK-internal retries.

The shipper installs a listener and scopes a task-local counter around each
SDK export. This provides a truthful count for that export without mixing
concurrent exports. There is no pinned-SDK limitation preventing per-export
attempt counting. The listener reports completed attempts only, which matches
the SDK retry attempts that produced an outcome.

### Oversized records and encoded boundary

- The existing full preflight remains unchanged: all partitions are built and
  checked before the first send.
- A single oversized encoded record fails the whole batch with
  `OversizedRecord`, preserving Lambda retry/DLQ behavior without record loss.
- The exact encoded-size boundary remains `encoded_len() >
  max_request_bytes`; a request exactly at the limit is accepted. No hidden
  headroom was added.

### Endpoint hardening

- Collector endpoint validation now rejects URI userinfo, including
  `http://user:password@collector.internal:4317`.
- Absolute HTTP/HTTPS origins without an explicit port remain valid; HTTPS can
  use its default port.

### Mapping and documentation

- Added focused mapper coverage for nested objects/arrays, JSON null,
  signed integer boundaries, unsigned overflow, floating-point values, all
  severity mappings, and event/observed timestamps.
- Documented that JSON `null` maps to OTLP string `"null"` because OTLP
  `AnyValue` has no null variant.
- Documented that direct Coralogix OTLP always targets public
  `ingress.<domain>:443`, does not inherit `UsePrivateLink`, and requires public
  egress from private-only subnets unless a reachable Collector is configured.
- No production API was added for the test-only synthetic `OtlpResponse`
  variant.

## Regression evidence

- The new userinfo test initially failed because the endpoint was accepted,
  then passed after validation was added.
- Startup-diagnostic and SDK-attempt-counter tests initially failed to compile
  because the requested behavior did not exist, then passed after the
  implementation.

## Verification results

- `cargo test destination_config_tests --lib`: 11 passed, 65 filtered out.
- `cargo test logs::exporter::otlp::tests --lib`: 14 passed, 62 filtered out.
- `cargo test logs::exporter::tests --lib`: 4 passed, 72 filtered out.
- `cargo test --test otlp_logs`: 2 passed.
- `cargo test --lib`: 76 passed.
- `cargo test --test logs`: 45 passed.
- `cargo fmt --all --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed with no issues.
- `git diff --check`: passed.

## Concerns

None. No deployment or live external Collector test was performed; wire
behavior is covered by the local Tonic OTLP integration tests.

## Propagated OTLP failure sanitization follow-up

### Security fix

- Replaced the raw `String` payload of `LogExportError::OtlpResponse` with an
  opaque `OtlpResponseError` containing only a controlled classification and
  sanitized gRPC status-code name.
- `SdkOtlpTransport` no longer calls `ResponseError::to_string()` when
  propagating failures. Raw SDK/server status messages are discarded after the
  dedicated structured event records `sdk_status`, `grpc_status`,
  `attempt_count`, and `elapsed_ms`.
- Both `Display` and `Debug` for the propagated error format only the sanitized
  classification and status code. This covers the outer pipeline's
  `?error`/Debug formatting path.
- Added a real local gRPC server regression that returns
  `PermissionDenied` with sentinel message
  `sentinel-server-controlled-secret`. The test verifies propagated Display
  and Debug contain `client_error` and `permission_denied`, and neither
  contains the sentinel.
- Moved the configured-destination startup event after successful exporter
  construction.
- Updated the Collector example to state that endpoint URI userinfo is
  forbidden.
- The test-only failing exporter now uses an existing generic failure variant;
  no production constructor was exposed solely for tests.

### Regression evidence

- Before the fix,
  `cargo test --test otlp_logs propagated_otlp_failure_excludes_server_message -- --nocapture`
  failed (`0 passed, 1 failed`) against the raw propagated SDK error.
- After the fix, the same test passed (`1 passed, 2 filtered out`), and the
  focused unit propagation test passed (`1 passed, 75 filtered out`).

### Verification results

- `cargo test logs::exporter::otlp::tests --lib`: 14 passed, 62 filtered out.
- `cargo test destination_config_tests --lib`: 11 passed, 65 filtered out.
- `cargo test logs::exporter::tests --lib`: 4 passed, 72 filtered out.
- `cargo test --test otlp_logs`: 3 passed.
- `cargo test --lib`: 76 passed.
- `cargo test --test logs`: 45 passed.
- `cargo fmt --all --check`: passed.
- `cargo clippy --all-targets -- -D warnings`: passed with no issues.
- `git diff --check`: passed.

Oversized-record behavior, authentication precedence, the exact encoded-size
boundary, and external-gate status were not changed.
