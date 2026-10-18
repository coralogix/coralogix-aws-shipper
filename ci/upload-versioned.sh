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
  local body=$1 target=$2 out attempt=0
  while :; do
    if out=$(aws s3api put-object \
      --bucket "$bucket" --key "$target" --body "$body" \
      --checksum-algorithm SHA256 --if-none-match '*' \
      ${GITHUB_SHA:+--metadata "commit=$GITHUB_SHA"} 2>&1); then
      return 0
    fi

    # 412: the key is already there, which is the immutability guarantee holding.
    if grep -qE 'PreconditionFailed|At least one of the pre-conditions' <<<"$out"; then
      return 1
    fi

    # 409: another request touched this key while ours was in flight. S3 documents this
    # as transient and retryable, unlike the 412 above which is a settled answer.
    if grep -q 'ConditionalRequestConflict' <<<"$out" && [ "$attempt" -lt 5 ]; then
      attempt=$((attempt + 1))
      echo "::warning::Conflict writing s3://$bucket/$target, retry $attempt of 5."
      sleep $((attempt * 2))
      continue
    fi

    echo "$out" >&2
    exit 1
  done
}

# Versioned keys are immutable, so whoever created one owns that version. Refuse to
# build a release on top of another commit's artifact: the upload would be skipped and
# this release would ship the earlier binaries under its own template and changelog.
assert_owner() {
  local target=$1 owner
  [ -n "${GITHUB_SHA:-}" ] || return 0
  owner=$(aws s3api head-object --bucket "$bucket" --key "$target" \
    --query 'Metadata.commit' --output text 2>/dev/null || true)
  if [ "$owner" = "None" ]; then
    owner=""
  fi
  if [ -n "$owner" ] && [ "$owner" != "$GITHUB_SHA" ]; then
    echo "::error::s3://$bucket/$target was published by $owner, not $GITHUB_SHA. Versioned artifacts are immutable, so this release cannot replace it. Bump the version instead of republishing." >&2
    exit 1
  fi
}

if out=$(aws s3api head-object --bucket "$bucket" --key "$key" 2>&1); then
  assert_owner "$key"
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
    assert_owner "$key"
    echo "::warning::s3://$bucket/$key was published by another run of this commit; checksumming that instead."
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
