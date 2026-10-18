#!/usr/bin/env bash
# Update an unversioned key, without ever moving it backwards.
#
# These keys carry no version, so they mean "latest" to whoever fetches them. The object
# records the version it holds, and a write only lands if that record has not changed
# since it was read. Two releases publishing at once therefore cannot interleave, and a
# re-run of an older release cannot roll the key back to it -- neither of which a check
# against the published tags can rule out, because the newer release has not tagged yet.
#
# Usage: put-latest.sh <source: local path or s3:// URI> <bucket> <key> <version>
set -euo pipefail

src=$1 bucket=$2 key=$3 version=$4

# put-object takes a file, not a location: an s3:// source has to come down first. The
# callers pass the published versioned object, so that "latest" is byte-identical to the
# immutable artifact for the same version rather than a rebuild of it.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
body="$tmp/$(basename "$key")"
if [[ $src == s3://* ]]; then
  aws s3 cp "$src" "$body" >/dev/null
else
  cp "$src" "$body"
fi

case "$key" in
  *.zip) content_type=application/zip ;;
  *)     content_type=binary/octet-stream ;;
esac

# The greater of two versions, by version order rather than string order: 1.4.9 is older
# than 1.4.17, which a lexical comparison gets backwards.
newest() { printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1; }

for attempt in 1 2 3 4 5; do
  if head=$(aws s3api head-object --bucket "$bucket" --key "$key" 2>&1); then
    current=$(jq -r '.Metadata.version // ""' <<<"$head")

    # Keys written before they recorded a version have nothing to compare against, so
    # the first release to touch one claims it.
    if [ -n "$current" ] && [ "$current" != "$version" ] && [ "$(newest "$version" "$current")" != "$version" ]; then
      echo "::warning::s3://$bucket/$key holds $current, which is newer than $version; leaving it alone."
      exit 0
    fi

    # Write only if the object is still the one just read.
    condition=(--if-match "$(jq -r '.ETag' <<<"$head")")
  elif grep -qE '\(404\)|Not Found' <<<"$head"; then
    condition=(--if-none-match '*')
  else
    echo "$head" >&2
    exit 1
  fi

  if out=$(aws s3api put-object \
    --bucket "$bucket" --key "$key" --body "$body" \
    --content-type "$content_type" --checksum-algorithm SHA256 \
    --metadata "version=$version${GITHUB_SHA:+,commit=$GITHUB_SHA}" \
    "${condition[@]}" 2>&1); then
    exit 0
  fi

  if grep -qE 'PreconditionFailed|At least one of the pre-conditions|ConditionalRequestConflict' <<<"$out"; then
    if [ "$attempt" -lt 5 ]; then
      echo "::warning::s3://$bucket/$key changed while this write was in flight, retry $attempt of 5."
      sleep $((attempt * 2))
    fi
    continue
  fi

  echo "$out" >&2
  exit 1
done

echo "::error::Gave up updating s3://$bucket/$key after 5 attempts."
exit 1
