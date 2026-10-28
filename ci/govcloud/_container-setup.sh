# shellcheck shell=bash
# Sourced, not executed. Prepares a Rust toolchain inside the AL2023 builder image.
#
# ci/Dockerfile.builder deliberately does not bake the toolchain in, so that it always
# honours rust-toolchain.toml; ci/fips-build.sh installs it at run time. The GovCloud
# verification jobs need the same, and duplicating it is how they drift apart.

export RUSTUP_HOME="${RUSTUP_HOME:-/opt/rustup}"
export CARGO_HOME="${CARGO_HOME:-/usr/local/cargo}"
export PATH="${CARGO_HOME}/bin:${PATH}"

# Use the git CLI and accept new host keys, so a forwarded ssh-agent can fetch the
# private dependency without a pre-seeded known_hosts inside the container.
export CARGO_NET_GIT_FETCH_WITH_CLI=true
export GIT_SSH_COMMAND="ssh -o StrictHostKeyChecking=accept-new"

# cx_sdk_otlp's build script fetches public protobuf definitions with protofetch,
# which defaults to SSH. HTTPS needs no key in the container.
export PROTOFETCH_GIT_PROTOCOL="${PROTOFETCH_GIT_PROTOCOL:-https}"

if ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain none --no-modify-path
fi

# rust-toolchain.toml already names the targets, so this installs the pinned
# toolchain and both of them.
rustup show

# ...but rustup resolves the toolchain from the directory it runs in, and the vendored
# webpki harness works inside a clone of upstream's repo, which has no
# rust-toolchain.toml. With --default-toolchain none that leaves cargo unresolvable, so
# pin the same channel as the default. ci/fips-build.sh never hits this because it
# stays in the workspace.
_channel=$(awk -F'"' '/^[[:space:]]*channel[[:space:]]*=/{print $2}' rust-toolchain.toml)
: "${_channel:?could not read channel from rust-toolchain.toml}"
rustup default "$_channel"
unset _channel
