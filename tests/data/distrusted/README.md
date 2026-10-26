# Distrusted certificate fixtures

Material for the GovCloud distrust runtime tests (`tests/govcloud_distrust.rs`).

## affected-chain.pem

A published chain containing an eMudhra intermediate whose private key was
compromised (CVE-2026-111943 / RUSTSEC-2026-0382).

  source: https://raw.githubusercontent.com/DataDog/saluki/main/lib/saluki-tls/tests/data/distrusted/dv-g3b-emudhra.pem
  fetched: 2026-10-26
  sha256: see below

## emudhra-intermediate.der

Certificate index 1 extracted from the chain above. Its SubjectPublicKeyInfo matches
`EMDVTLSCAG3B.spki.der`, one of the eight keys pinned in
`vendor/rustls-webpki/src/data/distrusted/`.

`fixture_still_carries_a_distrusted_key` asserts that match on every run, so replacing
or re-fetching this file cannot quietly leave the distrust tests passing against
material the patch does not block.

## Digests

Recompute with: `shasum -a 256 tests/data/distrusted/*`
ab6483de0cb01d53d59275baf74d2766f3599e57a48d3319e4a5536c268ac696  tests/data/distrusted/affected-chain.pem
42c4accd12c4f59597925e1a08a7a90102608543809308d50ec427e118ba1493  tests/data/distrusted/emudhra-intermediate.der
