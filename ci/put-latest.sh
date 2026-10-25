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
  if ! aws s3 cp "$src" "$body" >/dev/null; then
    echo "::error::$src does not exist, so $key cannot be updated. A GovCloud Release" \
         "mapping held back to a version that predates GovCloud artifact publishing" \
         "(1.4.18) lands here." >&2
    exit 1
  fi
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

    # What the key is allowed to move backwards past differs by caller, so the two modes
    # compare different things.
    #
    # Default: $version IS the release, so it orders directly. Refusing to move backwards
    # stops a re-run of an older release from reclaiming the key.
    #
    # Pinned (GovCloud): $version is the Release mapping from template-govcloud.yaml, a
    # value a human edits, so it does not order across commits -- re-holding the mapping
    # after an un-hold is a deliberate rollback the key must follow. Ordering therefore
    # comes from PUT_LATEST_PINNED, the release that is writing, which check_version
    # keeps strictly increasing. That still refuses a stale concurrent release: two runs
    # overlapping on master read different mappings, and without this the older one could
    # finish last and undo the newer one's write.
    #
    # Keys written before they recorded this have nothing to compare against, so the
    # first release to touch one claims it.
    if [ -n "${PUT_LATEST_PINNED:-}" ]; then
      ordering=$(jq -r '.Metadata.release // ""' <<<"$head")
      incoming=$PUT_LATEST_PINNED
      what="the release that wrote it"
    else
      ordering=$current
      incoming=$version
      what="which is newer than $version"
    fi

    if [ -n "$ordering" ] && [ "$ordering" != "$incoming" ] && [ "$(newest "$incoming" "$ordering")" != "$incoming" ]; then
      echo "::warning::s3://$bucket/$key holds $ordering ($what); leaving it alone."
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
    --metadata "version=$version${PUT_LATEST_PINNED:+,release=$PUT_LATEST_PINNED}${GITHUB_SHA:+,commit=$GITHUB_SHA}" \
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
