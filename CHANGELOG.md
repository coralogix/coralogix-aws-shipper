# Changelog

## v1.4.18 / 2026-10-16

### 💡 Enhancements 💡

- **Post-metadata Starlark transformation:** Add the opt-in `StarlarkTransformAfterMetadata` parameter so scripts can inspect and modify configured AWS and custom metadata before export. The default remains the existing pre-metadata behavior.
- **Transformed log batching:** Recompute REST request batches from final transformed bodies when post-metadata transformation is enabled, preventing fan-out or expanded events from creating oversized requests.

### 🧰 Bug fixes 🧰

- **Send-path future Send compliance:** Rework the send pipeline so stream futures capture only owned data, restoring compilation under the `#[async_recursion]` handler's `Send` requirement; batches are converted, transformed, and sent lazily per stream item so memory stays bounded during fan-out.
- **Typed transform results:** Preserve result types through the post-metadata transform — a log whose content is a JSON string literal (e.g. `"123"`) keeps its string type after even an identity script, and deliberate `to_json(event)` stringification survives. Script failures restore the original body verbatim.

## v1.4.17 / 2026-10-14

### 🧰 Bug fixes 🧰

- **Clear dependency advisories (INFOSEC-9164):** updated the dependency tree to resolve
  eight reported advisories — `aws-smithy-json` (CVE-2026-18140), `lru`
  (RUSTSEC-2026-0253), `quinn-proto` (RUSTSEC-2026-0338/0339/0340/0341/0342), and `scc`
  (RUSTSEC-2026-0205), which leaves the tree entirely. This bumps the AWS SDK crates and
  raises the minimum supported Rust version to 1.94.1.
- **Hold the validated FIPS module:** `rustls`, `rustls-webpki` and `aws-lc-rs` stay
  pinned so the NIST-validated AWS-LC FIPS v3 module (certificates #5314 and #5298) is
  retained rather than the uncertified v4. `cargo-deny` now bans `aws-lc-rs >= 1.18`
  outright so the upgrade cannot return unnoticed.

## v1.4.16 / 2026-09-10
