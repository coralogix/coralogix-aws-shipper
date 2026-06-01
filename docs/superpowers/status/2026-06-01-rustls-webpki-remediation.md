# Rustls WebPKI Remediation Status - 2026-06-01

Source alerts: GitHub Dependabot alerts #31, #33, #34, and #42 for `rustls-webpki` in `Cargo.lock`.

Branch base: `origin/govcloud`

## Initial Active Graph

Before remediation, two vulnerable `rustls-webpki` versions were active:

- `rustls-webpki 0.103.9` through the modern Rustls path used by `aws-smithy-http-client`, `reqwest`, and `cx_sdk_rest_logs`.
- `rustls-webpki 0.101.7` through the legacy `rustls 0.21.12` path pulled by AWS Smithy runtime `tls-rustls` and `test-util` features.

## Fix Applied

- Updated `rustls-webpki 0.103.9 -> 0.103.13`.
- Disabled AWS SDK default HTTPS/Rustls features that pulled the legacy Hyper 0.14 Rustls stack.
- Configured the shipper's AWS SDK runtime to always provide an explicit `aws-smithy-http-client` HTTPS client:
  - `CryptoMode::AwsLc` for normal mode.
  - `CryptoMode::AwsLcFips` when `ENABLE_AWS_FIPS` is truthy.
- Replaced test usage of `aws_smithy_runtime::client::http::test_util` with `aws_smithy_http_client::test_util`.
- Added explicit empty replay clients to tests that build AWS clients without expecting AWS network calls.

## Current Graph Evidence

After remediation:

```text
rustls-webpki v0.103.13
```

Absent vulnerable versions:

```bash
cargo tree -i rustls-webpki@0.103.9 --target all
cargo tree -i rustls-webpki@0.101.7 --target all
```

Both commands fail to match any package because only `rustls-webpki 0.103.13` remains.

## Validation Run

Commands run from `/Users/juangimenez/workspace/coralogix-aws-shipper/.worktrees/fix-aws-lc-fips-build`:

```bash
cargo tree -i rustls-webpki@0.103.13 --target all
cargo fmt --check
git diff --check
cfn-lint --template template.yaml --region us-east-1
make -n build-LambdaFunction RUST_ARCH=arm64 ARTIFACTS_DIR=/tmp/artifacts
make -n build-LambdaFunction RUST_ARCH=x86-64 ARTIFACTS_DIR=/tmp/artifacts
cargo test --all -- --nocapture
cargo clippy --all-targets -- -D warnings
```

Results:

- `rustls-webpki 0.103.13` is the only selected `rustls-webpki` package.
- `rustls-webpki 0.103.9` and `0.101.7` are absent from the selected graph.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cfn-lint --template template.yaml --region us-east-1` passed.
- Lambda Makefile dry-runs still select the cargo backend for both arm64 and x86-64.
- `cargo test --all -- --nocapture` passed: 174 tests passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings` passed.

## Residual Risk

This removes the active `rustls-webpki` alerts from the GovCloud dependency graph. The GitHub Dependabot UI may continue to show the alerts until these changes are pushed and scanned on the branch Dependabot is evaluating.
