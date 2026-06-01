# AWS-LC FIPS Lambda Build Status - 2026-06-01

Source failure: GitHub Actions `make build-LambdaFunction` during `sam build`.

Branch: `govcloud`

## Symptom

The release build for `aarch64-unknown-linux-gnu` failed while building `aws-lc-fips-sys 0.13.14`:

```text
error while parsing ".../aws-lc/crypto/fipsmodule/libbcm_c_generated_asm.a"
parse error near Unknown (line 1 symbol 1 - line 1 symbol 1): ""
gmake ... bcm-delocated.S Error 1
```

The failing command was using cargo-lambda's default Zig backend through `make build-LambdaFunction`.

## Dependency Path

The failure is from the GovCloud/FIPS TLS dependency graph, not from the direct `openssl` alert fix:

```text
aws-smithy-http-client
rustls-aws-lc-fips
rustls
aws-lc-rs
aws-lc-fips-sys
```

Local evidence:

```bash
cargo tree -i aws-lc-fips-sys --target aarch64-unknown-linux-gnu
```

## Fix Applied

Updated the Lambda build path to avoid the Zig backend for FIPS release builds:

- `Makefile` now defaults `CARGO_LAMBDA_COMPILER` to `cargo`.
- Arm64 release builds now target `aarch64-unknown-linux-gnu` when using the cargo backend.
- Arm64 builds set `clang`, `clang++`, `llvm-ar`, and `aarch64-linux-gnu-gcc` cross-build environment variables.
- Publish workflows install `clang`, `llvm`, `gcc-aarch64-linux-gnu`, and `g++-aarch64-linux-gnu`.
- Publish workflows set `CARGO_LAMBDA_COMPILER=cargo` for `sam build`.
- `Cargo.lock` bumps `aws-lc-rs 1.15.4 -> 1.17.0` and `aws-lc-sys 0.37.0 -> 0.41.0`.

## Validation Run

Commands run from `/Users/juangimenez/workspace/coralogix-aws-shipper/.worktrees/fix-aws-lc-fips-build`:

```bash
make -n build-LambdaFunction RUST_ARCH=arm64 ARTIFACTS_DIR=/tmp/artifacts
make -n build-LambdaFunction RUST_ARCH=x86-64 ARTIFACTS_DIR=/tmp/artifacts
cargo fmt --check
cfn-lint --template template.yaml --region us-east-1
cargo test --all -- --nocapture
cargo clippy --all-targets -- -D warnings
git diff --check
```

Results:

- Arm64 dry-run uses `cargo lambda build --compiler cargo --release --target aarch64-unknown-linux-gnu` with the aarch64 cross environment.
- x86-64 dry-run uses `cargo lambda build --compiler cargo --release --target x86_64-unknown-linux-gnu`.
- `cargo fmt --check` passed.
- `cfn-lint --template template.yaml --region us-east-1` passed.
- `cargo test --all -- --nocapture` passed: 174 tests passed, 0 failed.
- `cargo clippy --all-targets -- -D warnings` passed.
- `git diff --check` passed.

## Validation Gap

The exact Linux `sam build`/`cargo lambda build --compiler cargo --target aarch64-unknown-linux-gnu` path could not be fully executed on this macOS host because the fix depends on Linux cross toolchain packages installed in GitHub Actions.

The next validation loop is to run the GitHub Actions publish build on `govcloud` or after merging `govcloud` into the target branch.

## Follow-Up If CI Still Fails

1. Capture the new failing `cargo lambda` command line from GitHub Actions.
2. Confirm the installed Linux packages include `clang`, `llvm-ar`, `aarch64-linux-gnu-gcc`, and the aarch64 sysroot.
3. Run the failing command with `CARGO_LOG=cargo_lambda=debug`.
4. If the cargo backend still invokes the AWS-LC FIPS delocator incorrectly, isolate in a minimal `aws-lc-rs` reproducer and file/update the upstream issue.
