# GovCloud Compatibility Matrix

This document is the non-regression contract between `template.yaml` (commercial) and
`template-govcloud.yaml` (GovCloud). Every parameter, condition, rule, environment
variable, and custom-resource side-effect is classified as:

- **same** – identical semantics, no change needed
- **govcloud-adjusted** – same intent, different value/ARN/principal to fit `aws-us-gov` partition
- **unsupported** – feature not available in GovCloud (must fail-fast or be omitted)

FIPS scope is limited to AWS SDK FIPS endpoint resolution and the AWS-LC FIPS HTTP
client for AWS SDK traffic only. This matrix does not claim full end-to-end
Lambda runtime or Coralogix egress FIPS compliance.

---

## A1 – Parameter Contract

| Parameter | Type | Default | Allowed Values | GovCloud Status | Notes |
| --- | --- | --- | --- | --- | --- |
| `CustomDomain` | String | `''` | — | **same** | Required for GovCloud |
| `ApiKey` | String | — | — | **same** | Supports direct key or Secrets Manager ARN |
| `ApplicationName` | String | — | — | **same** | |
| `SubsystemName` | String | `''` | — | **same** | |
| `NewlinePattern` | String | `''` | — | **same** | |
| `AddMetadata` | String | `''` | — | **same** | |
| `CustomMetadata` | String | `''` | — | **same** | |
| `BlockingPattern` | String | `''` | — | **same** | |
| `LogStreamFilter` | String | `''` | — | **same** | |
| `SamplingRate` | Number | `1` | ≥1 | **same** | |
| `LogLevel` | String | `WARN` | INFO,WARN,ERROR,DEBUG | **same** | |
| `S3BucketName` | String | `none` | pattern `^([0-9A-Za-z\.\-_]+)(,[0-9A-Za-z\.\-_]+)*$` | **same** | |
| `CSVDelimiter` | String | `,` | MaxLength 1 | **same** | |
| `S3KeyPrefix` | String | `''` | MaxLength 1024 | **same** | |
| `S3KeySuffix` | String | `''` | MaxLength 1024 | **same** | |
| `S3BucketKMSKeyARN` | String | `''` | — | **same** | |
| `FunctionMemorySize` | Number | `1024` | 128–10240 | **same** | |
| `FunctionTimeout` | Number | `300` | 30–900 | **same** | |
| `FunctionRunTime` | String | `provided.al2023` | provided.al2023,provided.al2 | **same** | |
| `FunctionArchitectures` | String | `arm64` | arm64,x86_64 | **same** | |
| `LambdaAssumeRoleARN` | String | `''` | — | **same** | |
| `SNSTopicArn` | String | `arn:aws-us-gov:sns:us-gov-west-1:123456789012:placeholder` | — | **govcloud-adjusted** | GovCloud placeholder sentinel |
| `SQSTopicArn` | String | `arn:aws-us-gov:sqs:us-gov-west-1:123456789012:placeholder` | — | **govcloud-adjusted** | GovCloud placeholder sentinel |
| `SQSIntegrationTopicArn` | String | `arn:aws-us-gov:sqs:us-gov-west-1:123456789012:placeholder` | — | **govcloud-adjusted** | GovCloud placeholder sentinel |
| `SNSIntegrationTopicArn` | String | `arn:aws-us-gov:sns:us-gov-west-1:123456789012:placeholder` | — | **govcloud-adjusted** | GovCloud placeholder sentinel |
| `KinesisStreamArn` | String | `''` | — | **same** | |
| `IntegrationType` | String | `S3` | S3,CloudTrail,CloudWatch,VpcFlow,S3Csv,Sns,Sqs,Kinesis,CloudFront,Kafka,MSK,EcrScan | **same** | |
| `CloudWatchLogGroupName` | String | `''` | — | **same** | |
| `CloudWatchLogGroupPrefix` | String | `''` | — | **same** | |
| `EnableLogGroupTags` | String | `false` | true,false | **same** | |
| `LogGroupTagsCacheTTLSeconds` | Number | `300` | ≥0 | **same** | |
| `LambdaLogRetention` | Number | `5` | ≥1 | **same** | |
| `NotificationEmail` | String | `''` | MaxLength 320 | **same** | |
| `StoreAPIKeyInSecretsManager` | String | `true` | true,false | **same** | |
| `LambdaSubnetID` | String | `''` | — | **same** | |
| `LambdaSecurityGroupID` | String | `''` | — | **same** | |
| `UsePrivateLink` | String | `false` | true,false | **same** | |
| `MSKClusterArn` | String | `''` | — | **same** | |
| `KafkaTopic` | String | `''` | — | **same** | |
| `KafkaBatchSize` | Number | `100` | 1–10000 | **same** | |
| `KafkaBrokers` | CommaDelimitedList | `''` | — | **same** | |
| `KafkaSubnets` | CommaDelimitedList | `''` | — | **same** | |
| `KafkaSecurityGroups` | CommaDelimitedList | `''` | — | **same** | |
| `EnableDLQ` | String | `false` | true,false | **same** | |
| `DLQRetryLimit` | Number | `3` | 0–5 | **same** | |
| `DLQRetryDelay` | Number | `900` | 0–900 | **same** | |
| `DLQS3Bucket` | String | `none` | — | **same** | |
| `ReservedConcurrentExecutions` | Number | `0` | — | **same** | |
| `ExecutionRoleARN` | String | `''` | — | **same** | |
| `TelemetryMode` | String | `logs` | logs,metrics | **govcloud-adjusted** | `metrics` mode uses `streams.metrics.cloudwatch.amazonaws.com`; GovCloud support remains verification-needed |
| `MetricsFilter` | String | `''` | — | **same** | |
| `ExcludeMetricsFilters` | String | `''` | — | **same** | |
| `BatchMetrics` | String | `false` | true,false | **same** | |
| `MetricsBatchMaxSize` | Number | `4` | — | **same** | |
| `LambdaCodeBucket` | String | — | — | **govcloud-adjusted** | NEW parameter for GovCloud – shipper Lambda artifact bucket |
| `LambdaCodeKey` | String | — | — | **govcloud-adjusted** | NEW parameter for GovCloud – shipper Lambda artifact key |
| `CustomResourceCodeBucket` | String | — | — | **govcloud-adjusted** | NEW parameter for GovCloud – custom-resource artifact bucket |
| `CustomResourceCodeKey` | String | — | — | **govcloud-adjusted** | NEW parameter for GovCloud – custom-resource artifact key |

`template-govcloud.yaml` defines 58 parameters. It adds 4 artifact-location
parameters and intentionally omits commercial-only region mapping and Starlark
script parameters.

### GovCloud-only environment variables

| Variable | Source | Default/Value | GovCloud Status | Notes |
| --- | --- | --- | --- | --- |
| `AWS_USE_FIPS_ENDPOINT` | Environment | `true` | **govcloud-adjusted** | Forces AWS SDK endpoint resolution to FIPS endpoints where supported |
| `ENABLE_AWS_FIPS` | Environment | `true` | **govcloud-adjusted** | Enables AWS-LC FIPS HTTP client for AWS SDK traffic at runtime |

---

## A2 – Conditions and Rules Contract

### Conditions

| Condition | Logic | GovCloud Status | Notes |
| --- | --- | --- | --- |
| `DLQEnabled` | `EnableDLQ == 'true'` | **same** | |
| `IsKafkaIntegration` | `IntegrationType == 'Kafka'` | **same** | |
| `BlockPatternNotSet` | `BlockingPattern == ''` | **same** | |
| `LogStreamFilterNotSet` | `LogStreamFilter == ''` | **same** | |
| `CSVDelimiterUse` | `IntegrationType == 'S3Csv'` | **same** | |
| `NewlinePatternNotSet` | `NewlinePattern == ''` | **same** | |
| `AddMetadataNotSet` | `AddMetadata == ''` | **same** | |
| `CustomMetadataNotSet` | `CustomMetadata == ''` | **same** | |
| `IsSNSIntegration` | `IntegrationType == 'Sns'` | **same** | |
| `UseECRScan` | `IntegrationType == 'EcrScan'` | **same** | |
| `IsSQSIntegration` | `IntegrationType == 'Sqs'` | **same** | |
| `IsNotificationEnabled` | `NotificationEmail != ''` | **same** | |
| `S3KeyPrefixIsSet` | `S3KeyPrefix != ''` | **same** | |
| `S3SuffixIsSet` | `S3KeySuffix != ''` | **same** | |
| `IsApiKeyNotArn` | `ApiKey == split(':')[0]` | **same** | |
| `ApiKeyIsArn` | `!IsApiKeyNotArn` | **same** | |
| `UseAWSDefaultPrefix` | `VpcFlow OR CloudTrail` | **same** | |
| `UseAWSDefaultVpcFlowSuffix` | `IntegrationType == 'VpcFlow'` | **same** | |
| `UseAWSDefaultCloudTrailSuffix` | `IntegrationType == 'CloudTrail'` | **same** | |
| `StoreAPIKeyInSecretsManager` | `StoreAPIKeyInSecretsManager == 'true' AND IsApiKeyNotArn` | **same** | |
| `IsPrivateLink` | `UsePrivateLink == 'true' OR TelemetryMode == 'metrics'` | **same** | |
| `SNSIsSet` | `SNSTopicArn != placeholder OR SNSIntegrationTopicArn != placeholder` | **govcloud-adjusted** | Placeholder sentinel must use `arn:aws-us-gov:` |
| `SQSIsSet` | `SQSTopicArn != placeholder OR SQSIntegrationTopicArn != placeholder` | **govcloud-adjusted** | Same as above |
| `UseKinesisStreamARN` | `KinesisStreamArn != '' AND CWLogGroupName == '' AND IntegrationType == 'Kinesis' AND TelemetryMode == logs` | **same** | |
| `UseVpcConfig` | `LambdaSubnetID != '' AND LambdaSecurityGroupID != ''` | **same** | |
| `UseMSK` | `MSKClusterArn != '' AND KafkaTopic != '' AND IntegrationType == 'MSK' AND TelemetryMode == logs` | **same** | |
| `S3BucketNameIsSet` | `S3BucketName != 'none'` | **same** | |
| `IsLambdaAssumeRoleEnable` | `LambdaAssumeRoleARN != ''` | **same** | |
| `ExecutionRoleARNIsSet` | `ExecutionRoleARN != ''` | **same** | |
| `ReservedConcurrentExecutionsIsSet` | `ReservedConcurrentExecutions != 0` | **same** | |
| `S3BucketKMSKeyARNIsSet` | `S3BucketKMSKeyARN != ''` | **same** | |
| `TelemetryModeIsMetrics` | `TelemetryMode == metrics` | **same** | |
| `EnableLogGroupTagsIsTrue` | `EnableLogGroupTags == 'true'` | **same** | |

### Rules

| Rule | Condition | Assertions | GovCloud Status | Notes |
| --- | --- | --- | --- | --- |
| `ValidateDLQ` | `EnableDLQ == 'true'` | `DLQS3Bucket != 'none'` | **same** | |
| `ValidateCloudWatchLogs` | `CloudWatchLogGroupName != ''` | `CloudWatchLogGroupName != '' AND S3BucketName == 'none'` | **same** | |
| `ValidatePrivateLinkConfig` | `UsePrivateLink == 'true'` | `LambdaSubnetID != '' AND LambdaSecurityGroupID != ''` | **same** | |
| `ValidationS3Integrations` | S3/CloudTrail/VpcFlow/S3Csv/CloudFront | `S3BucketName != 'none'` | **same** | |
| `ValidateKafkaIntegrationParams` | `IntegrationType == 'Kafka'` | `KafkaTopic != ''` | **same** | |
| `ValidateMSKIntegrationParams` | `IntegrationType == 'MSK'` | `MSKClusterArn != '' AND KafkaTopic != ''` | **same** | |
| `ValidateSNSIntegrationParams` | `IntegrationType == 'Sns'` | `SNSIntegrationTopicArn != placeholder` | **govcloud-adjusted** | Placeholder sentinel must use `arn:aws-us-gov:` |
| `ValidateSQSIntegrationParams` | `IntegrationType == 'Sqs'` | `SQSIntegrationTopicArn != placeholder` | **govcloud-adjusted** | Same as above |

---

## A3 – Custom Resource Side Effects

### `ConfigureS3Integration` (IntegrationType: S3, S3Csv, VpcFlow, CloudTrail, CloudFront)

| Lifecycle | Side Effects |
| --- | --- |
| **Create** | `lambda:AddPermission` (Principal: `s3.amazonaws.com`, SourceArn: `arn:${partition}:s3:::bucket`) + `s3:PutBucketNotification` (append LambdaFunctionConfiguration) |
| **Update** | Delete then Create (with 15s sleep) |
| **Delete** | `lambda:RemovePermission` + `s3:PutBucketNotification` (remove shipper entry, preserve others) |
| **Skip condition** | Skipped if `SNSTopicArn` or `SQSTopicArn` is not the GovCloud placeholder and not empty |

**GovCloud status**: `custom-resource-govcloud/index.py` builds S3 `SourceArn`
with the supplied partition and uses GovCloud placeholder sentinels.

### `ConfigureKafkaIntegration` (IntegrationType: Kafka, MSK)

| Lifecycle | Side Effects |
| --- | --- |
| **Create (Kafka)** | `lambda:CreateEventSourceMapping` with `SelfManagedEventSource` + VPC subnet/sg source access configs |
| **Create (MSK)** | `lambda:CreateEventSourceMapping` with `EventSourceArn=MSKClusterArn`, one per topic |
| **Update** | Delete then Create (with 15s sleep) |
| **Delete** | Disable then delete all event source mappings for the function |

**GovCloud gap**: None – uses Lambda API calls only, no ARN partition hardcoding.

### `ConfigureCloudwatchIntegration` (IntegrationType: CloudWatch)

| Lifecycle | Side Effects |
| --- | --- |
| **Create** | `lambda:AddPermission` (Principal: `logs.{region}.amazonaws.com`, SourceArn: `arn:${partition}:logs:region:account:log-group:prefix*:*`) + `logs:PutSubscriptionFilter` per log group |
| **Update** | Remove stale subscription filters + re-create; updates custom-resource env var `log_groups` |
| **Delete** | `logs:DeleteSubscriptionFilter` + `lambda:RemovePermission` per log group |

**GovCloud status**: `custom-resource-govcloud/index.py` builds CloudWatch Logs
`SourceArn` values with the supplied partition.

### `ConfigureMetricsIntegration` (TelemetryMode: metrics)

| Lifecycle | Side Effects |
| --- | --- |
| **Create** | `cloudwatch:PutMetricStream` with FirehoseArn + RoleArn |
| **Update** | Delete then Create (with 30s sleep) |
| **Delete** | `cloudwatch:DeleteMetricStream` |

**GovCloud gap**: `streams.metrics.cloudwatch.amazonaws.com` principal – verify availability in GovCloud.  
**GovCloud gap**: `FirehoseAccessRole` AssumeRole principal `streams.metrics.cloudwatch.amazonaws.com` – must verify GovCloud form.

### Kinesis/MSK/SQS/SNS/EcrScan (native CloudFormation resources)

| Integration | Mechanism | GovCloud Status |
| --- | --- | --- |
| Kinesis | `AWS::Lambda::EventSourceMapping` | **same** |
| MSK | `AWS::Lambda::EventSourceMapping` | **same** |
| SQS | `AWS::Lambda::EventSourceMapping` + `AWS::Lambda::Permission` (Principal: `sns.amazonaws.com`) | **govcloud-adjusted** – Principal is correct but `SourceArn` in Permission uses the SQS ARN |
| SNS | `AWS::SNS::Subscription` + `AWS::Lambda::Permission` (Principal: `sns.amazonaws.com`) | **same** |
| EcrScan | `AWS::Events::Rule` (source: `aws.ecr`) + `AWS::Lambda::Permission` (Principal: `events.amazonaws.com`) | **verification-needed** – template uses `aws.ecr`, but GovCloud event source support must be verified |

### `configure_dlq` (EnableDLQ: true)

| Lifecycle | Side Effects |
| --- | --- |
| **Create** | `lambda:UpdateFunctionConfiguration` (DeadLetterConfig) + `lambda:CreateEventSourceMapping` |
| **Update** | Remove existing DLQ mapping + recreate |
| **Delete** | No-op (skipped) |

**GovCloud gap**: None – uses Lambda API only.

---

## A4 – Compatibility Classification Summary

### Items requiring GovCloud adjustment

| Item | Category | Required Change |
| --- | --- | --- |
| `SNSTopicArn` default/placeholder | Parameter | Use `arn:aws-us-gov:sns:us-gov-west-1:123456789012:placeholder` |
| `SQSTopicArn` default/placeholder | Parameter | Use `arn:aws-us-gov:sqs:us-gov-west-1:123456789012:placeholder` |
| `SQSIntegrationTopicArn` default/placeholder | Parameter | Use `arn:aws-us-gov:sqs:us-gov-west-1:123456789012:placeholder` |
| `SNSIntegrationTopicArn` default/placeholder | Parameter | Use `arn:aws-us-gov:sns:us-gov-west-1:123456789012:placeholder` |
| `AWS_USE_FIPS_ENDPOINT` | Environment | Set to `true` in `template-govcloud.yaml` |
| `ENABLE_AWS_FIPS` | Environment | Set to `true` in `template-govcloud.yaml`; enables the AWS-LC FIPS HTTP client at runtime |
| `SNSIsSet` condition | Condition | Compare against GovCloud placeholder |
| `SQSIsSet` condition | Condition | Compare against GovCloud placeholder |
| `ValidateSNSIntegrationParams` rule | Rule | Assert against GovCloud placeholder |
| `ValidateSQSIntegrationParams` rule | Rule | Assert against GovCloud placeholder |
| S3 `SourceArn` in `add_permission` | Custom Resource | Uses `arn:${partition}:s3:::bucket` in `custom-resource-govcloud/index.py` |
| CloudWatch Logs `SourceArn` in `add_permission` | Custom Resource | Uses `arn:${partition}:logs:...` in `custom-resource-govcloud/index.py` |
| `arn:aws:logs:...` in IAM policy | Template | Use `!Sub arn:${AWS::Partition}:logs:...` |
| `arn:aws:sns:...` in IAM policy | Template | Use `!Sub arn:${AWS::Partition}:sns:...` |
| `arn:aws:s3:::*` in IAM policy | Template | Use `arn:aws-us-gov:s3:::*` or `!Sub arn:${AWS::Partition}:s3:::*` |
| `arn:aws:ecr:...` in IAM policy | Template | Use `!Sub arn:${AWS::Partition}:ecr:...` |
| `arn:aws:cloudwatch:...` in IAM policy | Template | Use `!Sub arn:${AWS::Partition}:cloudwatch:...` |
| `arn:aws:lambda:...` in IAM policy | Template | Use `!Sub arn:${AWS::Partition}:lambda:...` |
| `arn:aws:logs:...` in IAM policy | Template | Use `!Sub arn:${AWS::Partition}:logs:...` |
| `arn:aws:s3:::${S3BucketName}` in Firehose role | Template | Use `!Sub arn:${AWS::Partition}:s3:::${S3BucketName}` |
| `arn:aws:logs:...` in Firehose role | Template | Use `!Sub arn:${AWS::Partition}:logs:...` |
| `streams.metrics.cloudwatch.amazonaws.com` | Template principal | Verification-needed for GovCloud Metric Streams support and principal form |
| `firehose.amazonaws.com` | Template principal | Verification-needed for GovCloud metrics-mode deployment path |
| `lambda.amazonaws.com` | Template principal | Same in GovCloud |
| `events.amazonaws.com` | Template principal | Same in GovCloud |
| `logs.{region}.amazonaws.com` | Custom Resource principal | GovCloud custom resource uses regional CloudWatch Logs principal |
| `s3.amazonaws.com` | Custom Resource principal | Same in GovCloud |
| Lambda `CodeUri: .` / SAM transform | Template | GovCloud template uses `AWS::Lambda::Function` with explicit S3 code location |
| `AWS::Serverless::Function` transform | Template | Replace with `AWS::Lambda::Function` for GovCloud (no SAM transform in GovCloud SAR) |
| `AWS::ServerlessRepo::Application` metadata | Template | Remove (not applicable to GovCloud) |
| `ECRScanTrigger` source `aws.ecr` | Template | Verify GovCloud EventBridge source name for ECR scan events |

### Items confirmed unsupported in GovCloud

| Item | Reason | Mitigation |
| --- | --- | --- |
| AWS Serverless Application Repository (SAR) | Not available in GovCloud | Deploy via direct CloudFormation with S3 artifact |
| `AWS::Serverless::*` transform | SAM transform not available without SAR in GovCloud | Use native `AWS::Lambda::Function` resources |

### Items confirmed same (no change)

All other parameters, conditions, rules, environment variables, and integration
logic present in `template-govcloud.yaml`, excluding verification-needed items
listed above, are confirmed **same**.

---

## Service Principal Map for GovCloud

| Service | Commercial Principal | GovCloud Principal | Status |
| --- | --- | --- | --- |
| Lambda execution | `lambda.amazonaws.com` | `lambda.amazonaws.com` | Same |
| CloudWatch Logs | `logs.amazonaws.com` | `logs.{region}.amazonaws.com` | GovCloud-adjusted in custom resource |
| EventBridge | `events.amazonaws.com` | `events.amazonaws.com` | Same |
| SNS | `sns.amazonaws.com` | `sns.amazonaws.com` | Same |
| Firehose | `firehose.amazonaws.com` | `firehose.amazonaws.com` | Verification-needed for metrics mode |
| CW Metric Streams | `streams.metrics.cloudwatch.amazonaws.com` | `streams.metrics.cloudwatch.amazonaws.com` | Verification-needed for metrics mode |
| S3 (bucket notification) | `s3.amazonaws.com` | `s3.amazonaws.com` | Same |

---

## ECR Scan EventBridge Source in GovCloud

The commercial template uses `source: ["aws.ecr"]` in the EventBridge rule pattern.
`template-govcloud.yaml` currently uses the same `aws.ecr` value, but GovCloud
ECR scan EventBridge delivery must remain a release verification item until
validated in a target GovCloud account.

---

## Known Bugs in Commercial Files (Do Not Fix Here)

- `custom-resource/index.py` line 161: `SourceArn=f'arn:aws:s3:::{bucket_name}'` – hardcoded partition.
- `custom-resource/index.py` lines 444, 465: `arn:aws:logs:...` – hardcoded partition.
- `template.yaml` line 720: `arn:aws:logs:...` in IAM policy – hardcoded partition.
- `template.yaml` line 758: `arn:aws:sns:...` in IAM policy – hardcoded partition.
- `template.yaml` line 767: `arn:aws:s3:::*` in IAM policy – hardcoded partition.
- `template.yaml` line 777: `arn:aws:s3:::*/*` in IAM policy – hardcoded partition.
- `template.yaml` line 876: `arn:aws:ecr:...` in IAM policy – hardcoded partition.
- `template.yaml` lines 902–903: `arn:aws:s3:::${DLQS3Bucket}` – hardcoded partition.
- `template.yaml` line 1361–1362: `arn:aws:s3:::${S3BucketName}` in Firehose role – hardcoded partition.
- `template.yaml` line 1368: `arn:aws:logs:...` in Firehose role – hardcoded partition.
- `template.yaml` line 1381–1382: `arn:aws:s3:::${S3BucketName}` in Firehose destination – hardcoded partition.
- `template.yaml` line 1390: `arn:aws:s3:::${S3BucketName}` in Firehose backup – hardcoded partition.
- `template.yaml` line 1486–1487: `arn:aws:lambda:...` in custom-resource policy – hardcoded partition.
- `template.yaml` line 1504: `arn:aws:cloudwatch:...` in custom-resource policy – hardcoded partition.
- `template.yaml` line 1511: `arn:aws:s3:::*` in custom-resource policy – hardcoded partition.
- `template.yaml` line 1518: `arn:aws:logs:...` in custom-resource policy – hardcoded partition.
