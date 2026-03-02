# Changelog

## v1.4.4 / 2026-03-02
### 🧰 Bug fixes 🧰
- Fixed `AccessDeniedException` when enabling DLQ on an existing stack. `lambda:ListEventSourceMappings` is a List-type IAM action that requires `Resource: "*"` — it cannot be scoped to specific function or event-source-mapping ARNs. Separated it into its own policy statement with wildcard resource while keeping the remaining CRUD actions scoped to specific ARNs.

## v1.4.3 / 2026-02-26
### 🧰 Bug fixes 🧰
- Fixed DLQ deployment failure when the generated SQS queue name exceeded the 80-character AWS limit. Removed the hardcoded `QueueName` (which used `${AWS::StackName}` as a suffix) so CloudFormation auto-generates a unique name, consistent with the Terraform module behavior.

## v1.4.2 / 2026-02-24
### 🧰 Bug fixes 🧰
- Fixed issue when deploying CloudWatch integration where `PutSubscriptionFilter` would fail because the Lambda invoke permission was not created for log groups not covered by a prefix-based permission. Permissions are now added individually for uncovered log groups and propagation delay increased from 1s to 15s.

### 💡 Enhancements 💡
- Added `Outputs` section to SAR template exposing `LambdaArn` for programmatic consumption

## v1.4.1 / 2026-02-23
### 🧰 Bug fixes 🧰
- Replaced custom `print()` Starlark helper with Starlark's native `LibraryExtension::Print`. The `print()` built-in now writes directly to stderr, which is captured by Lambda and visible in CloudWatch Logs without requiring `LogLevel=DEBUG`.

## v1.4.0 / 2026-02-11
### 💡 Enhancements 💡
- Add Starlark scripting support for log transformation. Users can define a `transform(event)` function in a Starlark script to unnest, filter, enrich, or modify logs before shipping. Scripts can be loaded from S3, HTTP/HTTPS URLs, base64-encoded strings, or inline. Includes built-in `parse_json`, `to_json`, and `print` helpers. Fail-open behavior ensures original logs are preserved on transform errors and on script resolution/compilation failures (transient S3/HTTP issues, IAM, or invalid script).

## v1.3.15 / 2026-02-09
### 💡 Enhancements 💡
- Update Rust dependencies and lockfile to address security advisories.

## v1.3.14 / 2026-01-09
### 💡 Enhancements 💡
- Add support for CloudWatch log group tags. Tags are automatically fetched and included in log metadata as `cw.tags`. Includes configurable TTL-based caching to optimize API calls. Errors are not cached, allowing immediate retry on transient failures.

## v1.3.13 / 2025-11-06
### 💡 Enhancements 💡
- Add batching support for metrics processing
- Lower logging to kinesis processing.

## v1.3.12 / 2025-10-07
### 💡 Enhancements 💡
- Update CoralogixRegionMap to use new standardized endpoint format

## v1.3.11 / 2025-08-11
### 💡 Enhancements 💡
- Add support for S3 bucket KMS key using `S3BucketKMSKeyARN`

## v1.3.10 / 2025-08-04
### 🧰 Bug fixes 🧰
- Fixed issue when deploying S3 integration with bucket names containing periods (e.g., `my-bucket.example.com`). The custom resource now sanitizes bucket names by replacing periods with underscores in Lambda permission statement IDs to comply with AWS naming constraints.

## v1.3.9 / 2025-02-08
### 🧰 Bug fixes 🧰
- Fixed dynamic metadata fallback behavior to use defaults when template references missing metadata keys instead of falling back to unrelated metadata

## v1.3.8 / 2025-02-07
### 🧰 Bug fixes 🧰
- Remove MaxLength for the `S3BucketName` parameter, as it could accept a comma-separated list of buckets

## v1.3.7 / 2025-6-9
### 💡 Enhancements 💡
- Add support for AP3 region

## v1.3.6 / 2025-05-01
### 🧰 Bug fixes 🧰
- Update package version for openssl and tokio to fix security vulnerabilities

### 💡 Chore 💡
- Update package dependencies to most recent compatible versions
- Removed unused imports
- Fixed deprecation warnings in the codebase


## v1.3.6 / 2025-04-02
### 🧰 Bug fixes 🧰
- Remove all special characters from the statement ID for CloudWatch integration to avoid AWS validation error

## v1.3.5 / 2025-02-20
### 💡 Enhancements 💡
- Added support for metrics filter for Cloudwatch Metric Streams, with CloudWatch private link integration

## v1.3.4 / 2025-02-19
### 🧰 Bug fixes 🧰
- Fixed issue when deploying S3 integration with bucket name longer than 40 characters.

## v1.3.3 / 2025-02-13
### 🧰 Bug fixes 🧰
- Fixed issue when updating an existing CF stack with S3, CloudTrail, VpcFlow or S3Csv integration type.

## v1.3.2 / 2025-02-12
### 💡 Chore 💡
- Update dependencies to fix security vulnerabilities
    - https://github.com/coralogix/coralogix-aws-shipper/security/dependabot/7

## v1.3.1 / 2025-02-04
### 🧰 Bug fixes 🧰
- Added support for dynamic allocation of Application and Subsystem names based on json key from log.

### v1.3.0 / 2025-01-20
### 💡 Enhancements 💡
- New intergration workflow added for ingesting Cloudwatch Stream Metrics via Firehose over PrivateLink
- Add Cloudwatch Metrics Stream creation to custom resource function

### v1.2.0 / 2025-01-7
### 🧰 Bug fixes 🧰
- Add permissions to custom lambda for `event-source-mapping`
### 💡 Enhancements 💡
- Add support to deploy 1 integration with multiple S3 buckets by passing comma seperated list to `S3BucketName` parameter

### v1.1.2 / 2025-12-31
### 🧰 Bug fixes 🧰
- cds-1756 - Restricted Lambda `EventSourceMapping` permissions used by custom resource function, so it won't have a wildcard/full resource access

### v1.1.1 / 2025-12-27
### 🧰 Bug fixes 🧰
- cds-1747 - Removed `iam:*` permissions from Shipper, as they were leftover from older versions as the Custom Resource use to be responsible for editing the policy directly

### v1.1.0 / 2025-12-11
### 💡 Enhancements (Breaking) 💡
- cds-1705 - updated support for dynamic value allocation of Application and Subsystem names based on internal metadata
- cds-1706 - updated how metadata is recorded and propagated throughout the function, including adding more metadata fields and updating the names of others.
    - stream_name --> cw.log.stream
    - bucket_name --> s3.bucket
    - key_name --> s3.object.key
    - topic_name --> kafka.topic
    - log_group_name --> cw.log.group

- [cds-1707] - Added new syntax for evaluating dynamic allocation fields. `{{ metadata | r'regex' }}`

## v1.0.16 / 2024-11-20
### 🧰 Bug fixes 🧰
- cds-1690 - Fixed a bug that when you update cloudwatch log group for an existing integraiotn from the CF the stack will fail.
- cds-1670 - Fixed a bug where Kinesis Integration was not correctly checking for Cloudwatch Formatted Logs in payload.

## v1.0.15 / 2024-11-09
### 💡 Enhancements 💡
- Add new parameter `LambdaAssumeRoleARN` which accept role arn, that the lambda will use for Execution role.
- Update internal code to support the new parameter `LambdaAssumeRoleARN`
- Add new parameter ReservedConcurrentExecutions to the lambda function.
- Removed circular dependency between DeadLetterQueue and CustomResourceFunction

## v1.0.14 / 2024-01-24
### 💡 Enhancements 💡
- Internal code refactoring to isolate logs workflow from additional telemetry workflows to come.

## v1.0.14 / 2024-01-10
### 🧰 Bug fixes 🧰
- Allow matches with arn of aws secretmanager in govcloud, previously only matched with public cloud secretmanager arn

## v1.0.13 / 2024-11-08
### 🧰 Bug fixes 🧰
- Allow the lambda to use the runtime `provided.al2`, by changing the binary build of cargo to a version that will support it in the Makefile. Add a parameter `FunctionRunTime` to allow users to choose the function runtime

## v1.0.12 / 2024-08-02
### 💡 Enhancements 💡
- Added support for CloudWatch over Kinesis Stream

## v1.0.11 / 2024-07-30
### 🧰 Bug fixes 🧰
- fix bug when trying to deploy CloudWatch integration. deploy with log group, with a name longer than 70 letters hit a limit with aws permission length, update the function so in case that the name is longer than 70 letters it will take the first 65 letters and the last 5.

## v1.0.10 / 2024-07-23
### 💡 Enhancements 💡
- Improved tamplate.yaml

## v1.0.9 / 2024-07-22
### 💡 Enhancements 💡
- Improved gzip process to support truncanted gzip files.

### 🚀 New components 🚀
- Added topic_name as option for Add_Metadata
- Allow user to pass S3 Object URL as topic name or CloudWatchLogGroup, the code will ge the parameter value from this file.

## v1.0.8 / 2024-05-13
### 💡 Enhancements 💡
- Disabled ANSI characters in tracing crate logs
- Minor documentation updates
  
### 🧰 Bug fixes 🧰
- Fix a bug with MSK integration - misssing command line in **LambdaTriggerMskTopic** custom lambda 

## v1.0.7 / 2024-05-13
### 💡 Enhancements 💡
- Added support for blocking pattern in CloudWatch integration
- Update dependencies

### 🧰 Bug fixes 🧰
- Fix duplication bug in CloudWatch Integration.

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
