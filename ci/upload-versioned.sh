#!/usr/bin/env bash
# Upload a release artifact under its versioned key, plus a <key>.sha256 checksum file
# (sha256sum format, so `sha256sum -c <key>.sha256` verifies a download).
#
# Versioned keys are immutable: every write is conditional on the key being absent
# (S3 If-None-Match), so a rebuild of an already-published version cannot replace what
# customers pinned, even if two publish runs overlap.
#
# The artifact is the lock, and the checksum is always derived from whatever is actually
# published. An interrupted run therefore recovers on the next attempt, and a release
# published before checksums existed picks one up the same way.
#
# Usage: upload-versioned.sh <source: local path or s3:// URI> <bucket> <key>
set -euo pipefail

src=$1 bucket=$2 key=$3
name=$(basename "$key")

# Writes the object only if the key is absent. Returns 1 if it already exists, and fails
# loudly on anything else.
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
  artifact_exists=true
elif grep -qE '\(404\)|Not Found' <<<"$out"; then
  artifact_exists=false
else
  echo "$out" >&2
  exit 1
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if [ "$artifact_exists" = false ]; then
  if [[ $src == s3://* ]]; then
    aws s3 cp "$src" "$tmp/$name"
  else
    cp "$src" "$tmp/$name"
  fi
  # The artifact is the lock: whichever run creates the key owns this version.
  if ! put_if_absent "$tmp/$name" "$key"; then
    echo "::warning::s3://$bucket/$key was published by another run; checksumming that instead."
    artifact_exists=true
  fi
fi

# Always checksum the PUBLISHED object, never the local source. A rebuild is not
# necessarily byte-identical -- zip stores mtimes, so a fresh checkout produces a
# different archive -- and a sidecar that disagrees with the object it names is worse
# than no sidecar at all.
if [ "$artifact_exists" = true ]; then
  aws s3 cp "s3://$bucket/$key" "$tmp/$name"
fi

(cd "$tmp" && sha256sum "$name" > "$name.sha256")

if ! put_if_absent "$tmp/$name.sha256" "$key.sha256"; then
  echo "::warning::s3://$bucket/$key.sha256 already exists, leaving it alone."
fi
