#!/bin/bash

set -euo pipefail

CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc TARGET_CC=aarch64-linux-gnu-gcc cargo lambda build --extension --release --locked --target aarch64-unknown-linux-gnu.2.17

pushd target

rm -rf extensions
mkdir extensions

cp lambda/extensions/coralogix-aws-shipper extensions
zip -9 coralogix-aws-shipper-arm64.zip extensions/*

popd