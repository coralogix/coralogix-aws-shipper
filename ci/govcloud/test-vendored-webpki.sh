#!/usr/bin/env bash
# Run upstream's regression suite against our patched rustls-webpki, on the exact
# FIPS module GovCloud ships.
#
# The crates.io package excludes src/test_utils.rs and tests/ (2.7 MB of fixtures),
# so the vendored copy cannot run its own tests in place -- including the
# distrusted_spki test that comes with the backported fix. Rather than commit that
# corpus, this borrows upstream's tree at a pinned commit and substitutes our src/
# into it. Upstream's manifest is kept, so its fixtures, dev-dependencies and
# autodiscovered integration tests still build; only the aws-lc-rs floor is relaxed,
# and the crypto crates are pinned to the versions in ci/govcloud/Cargo.lock.
#
# Pinning aws-lc-rs alone is not enough: it accepts aws-lc-fips-sys ^0.13.1, and that
# is the crate carrying the AWS-LC FIPS module, so an unpinned run can test a
# different module revision than the one that ships.
#
# Note: because upstream's manifest is retained, the harness lockfile records its
# version string (0.103.16), not our vendored 0.103.13. The code under test is ours --
# only src/ is substituted -- so the version there is cosmetic.
#
# A failure here means our backport diverges from upstream behaviour, or does so on
# the shipping module.
set -euo pipefail

# Pinned, not a moving branch: a drifting harness reports breakage only after the
# fact. Bump deliberately and re-review when upstream changes matter.
WEBPKI_COMMIT=${WEBPKI_COMMIT:-8ecdcc76f56b68fda67a6adf7976f5eed5006a4c}
TEST=verify_cert::tests::distrusted_spki

# Two modes, deliberately separate. Without --refresh nothing resolves: the committed
# harness lockfile is the input under test. With it, the pins are re-resolved and
# saved for review -- the only way the committed resolution changes.
REFRESH=0
case "${1:-}" in
  --refresh) REFRESH=1 ;;
  "") ;;
  *) echo "usage: $0 [--refresh]" >&2; exit 2 ;;
esac

here=$(cd "$(dirname "$0")/../.." && pwd)
vendored="$here/vendor/rustls-webpki"
govlock="$here/ci/govcloud/Cargo.lock"
harness_lock="$here/ci/govcloud/webpki-harness.lock"
[ -d "$vendored/src" ] || { echo "no vendored crate at $vendored" >&2; exit 1; }
[ -f "$govlock" ] || { echo "no GovCloud lockfile at $govlock" >&2; exit 1; }

locked_version() {
  grep -A1 "^name = \"$1\"\$" "$govlock" | grep '^version' | head -1 | cut -d'"' -f2
}
AWS_LC_RS=$(locked_version aws-lc-rs)
AWS_LC_FIPS_SYS=$(locked_version aws-lc-fips-sys)
if [ -z "$AWS_LC_RS" ] || [ -z "$AWS_LC_FIPS_SYS" ]; then
  echo "::error::could not read crypto versions from $govlock" >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

git clone -q --filter=blob:none https://github.com/rustls/webpki.git "$work/webpki"
git -C "$work/webpki" checkout -q "$WEBPKI_COMMIT"
echo "upstream harness @ $(git -C "$work/webpki" rev-parse HEAD)"

# Upstream supplies the fixtures and the test helper; we supply the code under test.
helper="$work/test_utils.rs"
cp "$work/webpki/src/test_utils.rs" "$helper"
rm -rf "$work/webpki/src"
cp -R "$vendored/src" "$work/webpki/src"
cp "$helper" "$work/webpki/src/test_utils.rs"

cd "$work/webpki"

# Relax only the aws-lc-rs floor, to our vendored crate's own requirement, so the
# v3-compatible version can resolve. Everything else in upstream's manifest stays.
floor=$(grep -A1 '^\[dependencies.aws-lc-rs\]' "$vendored/Cargo.toml" | grep '^version' | cut -d'"' -f2)
sed -i.bak -E "s|^aws-lc-rs = \{ version = \"[^\"]+\"|aws-lc-rs = { version = \"$floor\"|" Cargo.toml
grep -q "aws-lc-rs = { version = \"$floor\"" Cargo.toml || {
  echo "::error::could not relax the aws-lc-rs requirement" >&2; exit 1; }

assert_locked() {
  got=$(grep -A1 "^name = \"$1\"\$" Cargo.lock | grep '^version' | head -1 | cut -d'"' -f2)
  [ "$got" = "$2" ] || { echo "::error::$1 is ${got:-none} in the harness lockfile, expected $2" >&2
                         echo "::error::run '$0 --refresh', review the result, and commit it" >&2
                         exit 1; }
  echo "  $1 $got"
}

if [ "$REFRESH" = 1 ]; then
  # The only place resolution may change. No `|| true`: a failed pin would leave the
  # saved lockfile describing something other than what GovCloud ships.
  [ -f "$harness_lock" ] && cp "$harness_lock" Cargo.lock
  cargo update -p aws-lc-rs --precise "$AWS_LC_RS"
  cargo update -p aws-lc-fips-sys --precise "$AWS_LC_FIPS_SYS"
  echo "crypto pinned to the GovCloud lockfile:"
  assert_locked aws-lc-rs "$AWS_LC_RS"
  assert_locked aws-lc-fips-sys "$AWS_LC_FIPS_SYS"
  cp Cargo.lock "$harness_lock"
  echo "refreshed $harness_lock -- review the diff and commit it"
  exit 0
fi

# Normal runs never resolve. The committed lockfile is the input under test, so it is
# checked as-is: if the GovCloud pins moved and nobody refreshed this, that is a
# failure rather than something the run silently fixes for itself.
[ -f "$harness_lock" ] || {
  echo "::error::missing $harness_lock -- run '$0 --refresh' and commit the result" >&2
  exit 1; }
cp "$harness_lock" Cargo.lock
echo "harness lockfile agrees with the GovCloud lockfile:"
assert_locked aws-lc-rs "$AWS_LC_RS"
assert_locked aws-lc-fips-sys "$AWS_LC_FIPS_SYS"

features=aws-lc-rs,aws-lc-rs-fips,alloc

# --exact controls matching, not whether anything matched: a renamed or disabled test
# would otherwise be a silent zero-test pass. Assert it exists before running it.
if ! cargo test --locked --features "$features" -- --list > "$work/tests.list"; then
  echo "::error::could not enumerate tests" >&2; exit 1
fi
grep -qF "$TEST: test" "$work/tests.list" || {
  echo "::error::$TEST not found -- renamed or removed upstream" >&2
  grep -c ': test$' "$work/tests.list" | xargs echo "tests listed:" >&2
  exit 1; }

cargo test --locked --features "$features"
cargo test --locked --features "$features" -- --exact "$TEST"
