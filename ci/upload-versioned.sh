#!/usr/bin/env bash
# Upload a release artifact under its versioned key, plus a <key>.sha256 checksum file
# (sha256sum format, so `sha256sum -c <key>.sha256` verifies a download).
#
# Versioned keys are immutable. Every write is conditional on the key not already
# existing (S3 If-None-Match), so a rebuild of an already-published version cannot
# replace what customers pinned even if two publish runs overlap. A missing checksum
# file is still filled in, which is how releases published before checksums existed get
# one.
#
# Usage: upload-versioned.sh <source: local path or s3:// URI> <bucket> <key>
set -euo pipefail

src=$1 bucket=$2 key=$3
name=$(basename "$key")

# Writes the object only if the key is absent. Returns 1 on a lost race so the caller
# can decide what that means; anything else fails loudly.
put_if_absent() {
  local body=$1 target=$2 out
  if out=$(aws s3api put-object \
    --bucket "$bucket" --key "$target" --body "$body" \
    --checksum-algorithm SHA256 --if-none-match '*' 2>&1); then
    return 0
  fi
  if grep -qE 'PreconditionFailed|At least one of the pre-conditions' <<<"$out"; then
    return 1
  fi
  echo "$out" >&2
  exit 1
}

if out=$(aws s3api head-object --bucket "$bucket" --key "$key" 2>&1); then
  if aws s3api head-object --bucket "$bucket" --key "$key.sha256" >/dev/null 2>&1; then
    echo "::warning::s3://$bucket/$key already exists; versioned artifacts are immutable, skipping."
    exit 0
  fi
  echo "::warning::s3://$bucket/$key exists without a checksum; uploading the sidecar only."
  # Checksum the published object, not the source: a rebuild of an old release is not
  # byte-identical, so hashing it would produce a sidecar that fails verification.
  src="s3://$bucket/$key"
  artifact_exists=true
elif grep -qE '\(404\)|Not Found' <<<"$out"; then
  artifact_exists=false
else
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

# The checksum goes first and doubles as the lock: whichever run creates it owns this
# version. A run that loses the race stops here rather than uploading an artifact that
# would not match the winner's checksum.
if ! put_if_absent "$tmp/$name.sha256" "$key.sha256"; then
  echo "::warning::s3://$bucket/$key.sha256 already exists; another run published this version, skipping."
  exit 0
fi

if [ "$artifact_exists" = false ]; then
  if ! put_if_absent "$tmp/$name" "$key"; then
    echo "::warning::s3://$bucket/$key already exists; versioned artifacts are immutable, skipping."
  fi
fi
