# Rand Remediation Status - 2026-06-01

Source alerts: GitHub Dependabot alerts #35 and #36 for `rand` in `Cargo.lock`.

Branch base: `codex/fix-aws-lc-fips-build` on top of `origin/govcloud`.

## Initial Active Graph

Before remediation, two vulnerable `rand` versions were active:

- `rand 0.8.5` through:
  - `backoff -> cx_sdk_core`
  - `backoff -> cx_sdk_rest_logs`
  - `tower -> tonic -> opentelemetry-proto`
- `rand 0.9.2` through:
  - `opentelemetry_sdk -> opentelemetry-proto`
  - `proptest` dev dependency

Both alerts were for `GHSA-cq8v-f236-94qc`.

## Fix Applied

Applied the narrow lockfile-only update:

```bash
cargo update -p rand@0.8.5 -p rand@0.9.2
```

Result:

- `rand 0.8.5 -> 0.8.6`
- `rand 0.9.2 -> 0.9.4`

No `Cargo.toml` or SDK git revision changes were required.

## Current Graph Evidence

After remediation:

```text
rand v0.8.6
rand v0.9.4
```

Absent vulnerable versions:

```bash
cargo tree -i rand@0.8.5 --target all
cargo tree -i rand@0.9.2 --target all
```

Both commands fail to match any package because only `rand 0.8.6` and `rand 0.9.4` remain.

## Validation Run

Commands run from `/Users/juangimenez/workspace/coralogix-aws-shipper/.worktrees/fix-aws-lc-fips-build`:

```bash
cargo tree -i rand@0.8.6 --target all
cargo tree -i rand@0.9.4 --target all
cargo fmt --check
git diff --check
cfn-lint --template template.yaml --region us-east-1
cargo test --all -- --nocapture
cargo clippy --all-targets -- -D warnings
```

Results:

- `rand 0.8.6` and `rand 0.9.4` are selected in the active graph.
- `rand 0.8.5` and `rand 0.9.2` are absent from the selected graph.
- `cargo fmt --check` passed.
- `git diff --check` passed.
- `cfn-lint --template template.yaml --region us-east-1` passed.
- `cargo test --all -- --nocapture` passed: 174 tests passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings` passed.

## Residual Risk

The affected paths still include `coralogix-sdk-rust`, but the vulnerable `rand` versions were remediated through Cargo's compatible semver update without changing the SDK commit. No separate SDK PR is required for these two rand alerts unless the SDK repository has its own lockfile alerts.
