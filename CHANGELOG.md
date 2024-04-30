# Changelog
## v1.0.6 / 2024-04-25
### 🧰 Bug fixes 🧰
- Fixed Issue with S3 files naming containing "+"

## v1.0.5 / 2024-04-25
### 🧰 Bug fixes 🧰
- Fix runtime bug affecting Amazon Linux 2 by updating build runtime to Amazon Linux 2023

## v1.0.4 / 2024-04-24
### 🧰 Bug fixes 🧰
- Fix bug when deploying cloudwatch integration using log groups with "/" get an error

### 💡 Enhancements 💡
- Added support for DLQ

## v1.0.3 / 2024-04-09
### 💡 Enhancements 💡
- Support multiple topics for msk integration

### 🚀 New components 🚀
- Custom Metadata can be added to the log messages.
- Added Support for Custom CSV Header

### 🧰 Bug fixes 🧰
- Update CloudWatch custom lambda, so you will be able to see log group as trigger in the UI
- Update cloudwatch integration to delete log group subscription after the lambda deletion

## v1.0.2 / 2024-03-21
### 🧰 Bug fixes 🧰
- Ecr integration lambda trigger bug fix

## v1.0.1 / 2024-03-06
- Update dependencies to fix security vulnerabilities
    - https://github.com/coralogix/coralogix-aws-shipper/security/dependabot/2

## v1.0.1 / 2024-03-01
### 🚀 New components 🚀
- Added LogGroup Name to CloudWatch Metadata
- Added X86 support

## v1.0.0 🎉 / 2024-02-04
- GA release of Coralogix AWS Shipper

## v0.0.14 Beta / 2024-02-03
### 🚀 New components 🚀
- Added support for ECR Image Scan

## v0.0.13 Beta / 2024-02-01
### 🧰 Bug fixes 🧰
- Fix bug causing non-kafka events to show up as kafka event

## v0.0.12 Beta / 2024-01-31
### 🧰 Bug fixes 🧰
- Update aws_lambda_events 

## v0.0.11 Beta / 2024-01-25
### 🚀 New components 🚀
- Added support for AWS MSK and Kafka Integration type
- Updated Cloudformation template with resources for MSK and Kafka

## v0.0.10 Beta / 2024-01-17
### 🧰 Bug fixes 🧰
- Fix typo in Coralogix region selection US --> US1

## v0.0.9 Beta / 2024-01-16
### 🚀 New components 🚀
- Added support for Cloudfront Access logs

### 💡 Enhancements 💡
- Support for adding metadata to logs (bucket_name, key_name, stream_name)

## v0.0.8 Beta / 2024-01-16
### 🧰 Bug fixes 🧰
- Fix issue with decompression of some gzip files

## v0.0.7 Beta / 2024-01-08
### 🚀 New components 🚀
- Added support for kinesis Text and Cloudwatch Logs

## v0.0.6 Beta / 2024-01-04
### 🧰 Bug fixes 🧰
- Allow to choose Sqs as integrationType
  
## v0.0.5 Beta / 2024-01-03
### 🧰 Bug fixes 🧰
- Bug Fix in SNS email notification

## v0.0.4 Beta / 2024-01-03
### 🚀 New components 🚀
- Added support for sqs for s3 and sqs messages

### 🧰 Bug fixes 🧰
- Fix Sns and Sqs space in key bug

## v0.0.3 Beta / 2023-12-26
### 🛑 Breaking changes 🛑
- Update the CoralogixRegion param list to be the same as the list in the [website](https://coralogix.com/docs/coralogix-domain/)

### 💡 Enhancements 💡
- Moved internal logic to lib.rs and Added Integration tests
- Added s3_key variable for app and subsystem name

### 🧰 Bug fixes 🧰
- Fixed readme badge link for version
- Reduce Secret Manage IAM permissions
- Added default App or Subsystem name.

## v0.0.2 Beta / 2023-12-15
### 🧰 Bug fixes 🧰
- Lambda fail on empty empty gzip file for ELB logs.
- Change LogLevel to WARN

### 💡 Enhancements 💡
- Added key_path to ungzip error log for reference.
- added Variables BATCHES_MAX_SIZE and BATCHES_MAX_CONCURRENCY

## v0.0.1 Beta / 2023-12-12

- First release of Coralogix AWS Shipper
- Updated template version
