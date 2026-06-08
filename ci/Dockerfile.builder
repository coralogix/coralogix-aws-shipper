# FIPS-capable AWS Lambda builder.
#
# Amazon Linux 2023 == the same glibc (2.34) as the `provided.al2023` Lambda runtime.
# Building `aws-lc-fips-sys` with the NATIVE compiler here:
#   - works (zig / cargo-zigbuild cannot build aws-lc-fips-sys: the FIPS delocate
#     step fails with "parse error near Unknown" on libbcm_c_generated_asm.a), and
#   - links glibc <= 2.34, so the artifact runs on provided.al2023 (no GLIBC_2.39 crash).
#
# The Rust toolchain is NOT baked in; it is installed at run time by ci/fips-build.sh
# so it always honors the repo's rust-toolchain.toml.
FROM amazonlinux:2023

COPY requirements.txt /tmp/requirements.txt

# hadolint ignore=DL3041
RUN dnf install -y --setopt=retries=25 --setopt=timeout=60 \
        gcc gcc-c++ make cmake golang perl perl-FindBin perl-IPC-Cmd \
        clang llvm protobuf-compiler openssl-devel \
        python3 python3-pip tar gzip git openssh-clients \
        findutils file binutils \
    && dnf clean all \
    && pip3 install --quiet --no-cache-dir -r /tmp/requirements.txt

ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:/opt/rustup/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
