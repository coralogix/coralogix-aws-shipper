#!/usr/bin/env bash
# Upload a release artifact under its versioned key, plus a <key>.sha256 checksum file
# (sha256sum format, so `sha256sum -c <key>.sha256` verifies a download).
#
# Versioned keys are immutable: if the key already exists the upload is skipped, so a
# rebuild of an already-published version never replaces what customers pinned.
#
# Usage: upload-versioned.sh <source: local path or s3:// URI> <bucket> <key>
set -euo pipefail

src=$1 bucket=$2 key=$3
name=$(basename "$key")

if out=$(aws s3api head-object --bucket "$bucket" --key "$key" 2>&1); then
  echo "::warning::s3://$bucket/$key already exists; versioned artifacts are immutable, skipping."
  exit 0
elif ! grep -qE '\(404\)|Not Found' <<<"$out"; then
  echo "$out" >&2
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
if [[ $src == s3://* ]]; then
  aws s3 cp "$src" "$tmp/$name"
else
  cp "$src" "$tmp/$name"
fi
(cd "$tmp" && sha256sum "$name" > "$name.sha256")

# Checksum file first: if the job dies in between, the artifact is still missing and a
# rerun uploads both.
aws s3 cp "$tmp/$name.sha256" "s3://$bucket/$key.sha256" --checksum-algorithm SHA256
aws s3 cp "$tmp/$name" "s3://$bucket/$key" --checksum-algorithm SHA256
