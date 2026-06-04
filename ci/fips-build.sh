#!/usr/bin/env bash
# Build the FIPS-enabled coralogix-aws-shipper Lambda inside the AL2023 builder.
#
# Runs the NATIVE cargo compiler (no zig) so aws-lc-fips-sys compiles, and asserts
# the resulting bootstrap requires no glibc symbol newer than the provided.al2023
# runtime (2.34). Intended to run inside ci/Dockerfile.builder with the repo mounted
# at /work. Resolves the private cx_sdk_rest_logs crate via a forwarded ssh-agent.
set -euo pipefail

WORKDIR="${WORKDIR:-/work}"
ARTIFACT_DIR="${ARTIFACT_DIR:-${WORKDIR}/.build-artifacts}"
MAX_GLIBC="${MAX_GLIBC:-2.34}"   # provided.al2023 runtime glibc
BIN_NAME="coralogix-aws-shipper"

export RUSTUP_HOME="${RUSTUP_HOME:-/opt/rustup}"
export CARGO_HOME="${CARGO_HOME:-/usr/local/cargo}"
export PATH="${CARGO_HOME}/bin:${PATH}"

# Use the git CLI + accept new host keys so the forwarded ssh-agent can fetch the
# private dependency without a pre-seeded known_hosts inside the container.
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=accept-new"

# Install the toolchain pinned by rust-toolchain.toml (only if not already cached).
if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain none --no-modify-path
fi

cd "${WORKDIR}"
rustup show

echo "=== building FIPS lambda (native cargo, no zig) ==="
# Direct cargo-lambda invocation (native arm64). We deliberately do NOT go through
# `make build-LambdaFunction`, whose cargo path injects an x86->arm64 cross env
# (sysroot /usr/aarch64-linux-gnu) that does not exist on a native arm64 host.
cargo lambda build --release --compiler cargo

mkdir -p "${ARTIFACT_DIR}"
cp "./target/lambda/${BIN_NAME}/bootstrap" "${ARTIFACT_DIR}/bootstrap"

echo "=== verifying bootstrap requires glibc <= ${MAX_GLIBC} (provided.al2023) ==="
max_req="$(objdump -T "${ARTIFACT_DIR}/bootstrap" \
  | grep -oE 'GLIBC_[0-9]+\.[0-9]+' \
  | sed 's/GLIBC_//' \
  | sort -uV \
  | tail -1 || true)"
echo "max GLIBC required by bootstrap: ${max_req:-<none>}"

if [ -n "${max_req}" ]; then
  highest="$(printf '%s\n%s\n' "${MAX_GLIBC}" "${max_req}" | sort -V | tail -1)"
  if [ "${highest}" != "${MAX_GLIBC}" ]; then
    echo "ERROR: bootstrap requires GLIBC ${max_req} > ${MAX_GLIBC}."
    echo "       It would fail at runtime on the provided.al2023 Lambda runtime."
    exit 1
  fi
fi

file "${ARTIFACT_DIR}/bootstrap"
echo "=== FIPS BUILD OK (glibc <= ${MAX_GLIBC}) ==="
