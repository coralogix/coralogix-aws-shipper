# Changelog
## v0.0.3 Beta / 2023-12-26

### 🛑 Breaking changes 🛑
- Update the CoralogixRegion param list to be the same as the list in the [website](https://coralogix.com/docs/coralogix-domain/)

## 💡 Enhancements 💡
- Moved internal logic to lib.rs and Added Integration tests
- Added s3_key variable for app and subsystem name

### 🧰 Bug fixes 🧰
- Fixed readme badge link for version
- Reduce Secret Manage IAM permissions
- Added default App or Subsystem name.

## v0.0.2 Beta / 2023-13-15

### 🧰 Bug fixes 🧰
- Lambda fail on empty empty gzip file for ELB logs.
- Change LogLevel to WARN

### 💡 Enhancements 💡
- Added key_path to ungzip error log for reference.
- added Variables BATCHES_MAX_SIZE and BATCHES_MAX_CONCURRENCY

## v0.0.1 Beta / 2023-12-12

- First release of Coralogix AWS Shipper
- Updated template version
