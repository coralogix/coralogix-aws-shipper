# Vendored rustls-webpki 0.103.13 + upstream distrust patch

Source: crates.io `rustls-webpki` 0.103.13, unmodified except as below.
Licence: unchanged, see LICENSE.

Applied: the certificate-distrust change from rustls/webpki#546
(https://github.com/rustls/webpki/pull/546), released upstream as 0.103.16 and
tracked as RUSTSEC-2026-0382 / CVE-2026-111943.

Taken verbatim:
  src/end_entity.rs   src/error.rs   src/verify_cert.rs
  src/data/distrusted/*.spki.der   (8 files, fetched from rustls/webpki rel-0.103)

Nothing else is changed. The 0.103.16 release also raises the aws-lc-rs requirement
to ^1.18 (introduced in 0.103.14, unrelated to this security fix), which would move
the FIPS build to the uncertified AWS-LC v4 module. This copy exists only to take the
security fix without that.

Remove when GovCloud can run a mainline build that meets its validation requirements.
