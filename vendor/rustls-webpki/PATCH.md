# Vendored rustls-webpki 0.103.16, ML-DSA reverted

Source: crates.io `rustls-webpki` 0.103.16, unmodified except as below.
Licence: unchanged, see LICENSE.

0.103.16 carries the certificate-distrust fix for the compromised eMudhra
intermediates (rustls/webpki#546, RUSTSEC-2026-0382 / CVE-2026-111943). It also
requires aws-lc-rs ^1.18, which links the AWS-LC FIPS v4 module -- submitted to NIST
but not yet certified. GovCloud must stay on the validated v3 module (certificates
#5314 and #5298), so this copy reverts the one thing that forces 1.18.

## What is changed

Only ML-DSA, restored to the form it had in 0.103.13:

  src/aws_lc_rs_algs.rs   taken verbatim from 0.103.13
  src/lib.rs              ML-DSA re-exports and ALGORITHMS entries re-gated
  src/alg_tests.rs        the ML-DSA test module re-gated
  Cargo.toml              aws-lc-rs pinned to =1.17.0; aws-lc-rs-unstable passes
                          through aws-lc-rs/unstable again

The distrust fix itself is untouched upstream code -- this copy adds nothing to it.

## Why reverting ML-DSA is safe here

ML-DSA was already compiled out under `aws-lc-rs-fips` in 0.103.13, so the FIPS build
this repository ships has never contained it. Removing it from 0.103.16 preserves the
algorithms available in the existing GovCloud FIPS build. Its tests are likewise
FIPS-excluded upstream, so the regression suite is unaffected.

0.103.16 also dropped the per-algorithm `in_fips_submission` flag and simplified
`fips()` to `try_fips_mode().is_ok()`, which it could do because v4 brought ML-DSA into
FIPS scope. We restore the complete 0.103.13 algorithm implementation to preserve the
existing GovCloud configuration, including its FIPS reporting. ML-DSA was the only
algorithm outside v3's submission scope (3 of 23), so with it removed either form of
`fips()` would classify the remaining algorithms identically; taking the file whole
keeps the copy a plain revert rather than a new variant.

## Verifying this copy

    SRC=$(ls -d ~/.cargo/registry/src/*/rustls-webpki-0.103.16 | head -1)
    diff -r "$SRC" vendor/rustls-webpki      # expect only the files listed above

The pinned `=1.17.0` matters: a caret requirement lets `cargo update` resolve 1.18 and
pull FIPS v4 back in silently. `ci/govcloud/deny.toml` bans aws-lc-rs >= 1.18 as a
second guard.

Remove when GovCloud can run a mainline build that meets its validation requirements.
