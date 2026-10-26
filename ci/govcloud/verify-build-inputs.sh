#!/usr/bin/env bash
# Assert the GovCloud build resolves and compiles what it is supposed to, for one
# release target.
#
# Run with the GovCloud inputs in place, once per target:
#   cp ci/govcloud/config.toml .cargo/config.toml
#   cp ci/govcloud/Cargo.lock Cargo.lock
#   TARGET=aarch64-unknown-linux-gnu ci/govcloud/verify-build-inputs.sh
#
# This checks build INPUTS. Behaviour comes from ci/govcloud/test-vendored-webpki.sh
# (upstream's regression suite, including distrusted_spki) and the AL2023 runtime
# tests. A digest proves provenance, not correctness.
#
# Four bypasses found in review, each of which this now avoids:
#   - grepping for identifiers such as DISTRUSTED_SPKIS: both production checks can be
#     deleted while the const, the struct field and the tests still match. Verified.
#   - resolving without --locked: verification repairs a stale lockfile, then blesses
#     what it just produced.
#   - reading features from cargo metadata: its feature sets are unified across normal,
#     build and dev contexts, so a dev-only feature appears on a runtime-reachable
#     package. Verified here -- metadata reports test-util on aws-smithy-http-client,
#     the release compilation unit does not.
#   - matching crates by name parsed out of a package id: path dependencies are spelled
#     `path+file:///...#0.103.13`, with no name at all, so rustls-webpki silently looked
#     absent. Package ids come from metadata and are matched exactly.
#
# Building outside CI may need PROTOFETCH_GIT_PROTOCOL=https: cx_sdk_otlp's build
# script fetches protobuf definitions with protofetch, which defaults to SSH. The
# definitions are public, so HTTPS works and avoids needing a key in the container.
set -euo pipefail

TARGET=${TARGET:-$(rustc -vV | awk '/^host:/{print $2}')}
here=$(cd "$(dirname "$0")/../.." && pwd)
cd "$here"

# Digest of vendor/rustls-webpki/src -- the reviewed source in full. Any edit anywhere
# in it, including removing a check while leaving its identifiers behind, changes this.
# Recompute deliberately when updating the vendored crate:
#   find vendor/rustls-webpki/src -type f | LC_ALL=C sort | xargs shasum -a 256 | shasum -a 256
EXPECTED_SRC_DIGEST=a4d18e21c4ddc67a0464fc4d7e133e15f37684e17e558777853f76ef11fd19e6

fail=0
check() { # check <description> <0|1> [detail]
  if [ "$2" = 0 ]; then printf '  ok    %s\n' "$1"
  else printf '  FAIL  %s%s\n' "$1" "${3:+ -- $3}"; fail=1; fi
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "verifying GovCloud build inputs for $TARGET"

# ---- 1. the prepared lockfile is the committed one ------------------------------
# Checked before resolving, so a mismatch is reported rather than silently repaired.
check "Cargo.lock matches ci/govcloud/Cargo.lock" \
  "$(cmp -s Cargo.lock ci/govcloud/Cargo.lock && echo 0 || echo 1)" \
  "prepare it with: cp ci/govcloud/Cargo.lock Cargo.lock"

# ---- 2. the vendored source is exactly what was reviewed -------------------------
actual_digest=$(find vendor/rustls-webpki/src -type f | LC_ALL=C sort | xargs shasum -a 256 | shasum -a 256 | cut -d' ' -f1)
check "vendored rustls-webpki source matches the reviewed digest" \
  "$([ "$actual_digest" = "$EXPECTED_SRC_DIGEST" ] && echo 0 || echo 1)" \
  "got $actual_digest"

# ---- 3. locked resolution, restricted to this target's runtime graph -------------
if ! cargo metadata --format-version 1 --locked --filter-platform "$TARGET" \
     >"$tmp/meta.json" 2>"$tmp/meta.err"; then
  echo "::error::cargo metadata --locked failed for $TARGET:" >&2
  cat "$tmp/meta.err" >&2
  exit 1
fi

report=$(python3 "$here/ci/govcloud/_resolve_report.py" "$here" "$tmp/meta.json")

q() { python3 -c "
import json,sys
v = json.loads(sys.argv[1])[sys.argv[2]].get(sys.argv[3])
print('' if v is None else v)" "$report" "$1" "$2"; }

for c in rustls-webpki aws-lc-rs aws-lc-fips-sys rustls; do
  n=$(q "$c" count)
  check "exactly one runtime $c" "$([ "$n" = 1 ] && echo 0 || echo 1)" "found $n"
done

check "rustls-webpki resolves from a local path, not the registry" \
  "$([ "$(q rustls-webpki source)" = local ] && echo 0 || echo 1)" "source=$(q rustls-webpki source)"
check "rustls-webpki is vendor/rustls-webpki" \
  "$([ "$(q rustls-webpki manifest)" = "$here/vendor/rustls-webpki/Cargo.toml" ] && echo 0 || echo 1)" \
  "$(q rustls-webpki manifest)"

# ---- 4. crypto versions match the GovCloud lockfile, in the runtime graph --------
locked() { grep -A1 "^name = \"$1\"\$" ci/govcloud/Cargo.lock | grep '^version' | head -1 | cut -d'"' -f2; }
for c in aws-lc-rs aws-lc-fips-sys; do
  want=$(locked "$c"); got=$(q "$c" version)
  check "$c is $want at runtime" "$([ "$want" = "$got" ] && echo 0 || echo 1)" "resolved $got"
done

# ---- 5. FIPS features, read off release compilation units ------------------------
echo "  -- release compilation units --"
if ! cargo check --release --locked --target "$TARGET" --message-format=json \
     >"$tmp/units.json" 2>"$tmp/units.err"; then
  echo "::error::release check failed for $TARGET:" >&2
  tail -20 "$tmp/units.err" >&2
  exit 1
fi

units=$(python3 "$here/ci/govcloud/_unit_report.py" "$tmp/units.json" "$report" "$TARGET")

ufeat() { python3 -c "
import json,sys
d = json.loads(sys.argv[1])['crates'].get(sys.argv[2], {})
print('yes' if sys.argv[3] in d.get('features', []) else 'no')" "$units" "$1" "$2"; }
ucount() { python3 -c "
import json,sys
print(json.loads(sys.argv[1])['crates'].get(sys.argv[2], {}).get('artifacts', 0))" "$units" "$1"; }

check "the release build reported success" \
  "$(python3 -c "import json,sys;print(0 if json.loads(sys.argv[1])['finished_ok'] else 1)" "$units")"

# Exactly one library artifact per crate for this target: zero would make the feature
# assertions below vacuously pass, more than one would make them ambiguous.
for c in rustls rustls-webpki aws-lc-rs aws-lc-fips-sys; do
  n=$(ucount "$c")
  check "exactly one $c library artifact for $TARGET" \
    "$([ "$n" = 1 ] && echo 0 || echo 1)" "found $n"
done

check "rustls built with fips"                  "$([ "$(ufeat rustls fips)" = yes ] && echo 0 || echo 1)"
check "rustls-webpki built with aws-lc-rs-fips" "$([ "$(ufeat rustls-webpki aws-lc-rs-fips)" = yes ] && echo 0 || echo 1)"
check "aws-lc-rs built with fips"               "$([ "$(ufeat aws-lc-rs fips)" = yes ] && echo 0 || echo 1)"
# The RUSTSEC-2026-0348 exception in ci/govcloud/deny.toml depends on this staying off.
check "rustls built without brotli (RUSTSEC-2026-0348 exception)" \
  "$([ "$(ufeat rustls brotli)" = no ] && echo 0 || echo 1)"

[ "$fail" = 0 ] || { echo "::error::GovCloud build inputs are not what they should be for $TARGET" >&2; exit 1; }
echo "all GovCloud build inputs verified for $TARGET"
echo "note: inputs only -- behaviour is covered by ci/govcloud/test-vendored-webpki.sh"
