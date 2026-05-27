# GovCloud Deployment Guide – coralogix-aws-shipper

This guide covers deploying the Coralogix AWS Shipper in AWS GovCloud (US) regions
(`us-gov-west-1` and `us-gov-east-1`) using the dedicated `template-govcloud.yaml`.

---

## Overview

GovCloud uses a separate CloudFormation template (`template-govcloud.yaml`) because:

1. **No AWS Serverless Application Repository (SAR)** – SAR is not available in GovCloud.
   The template uses native `AWS::Lambda::Function` resources instead of `AWS::Serverless::Function`.
2. **Partition-safe ARNs** – All IAM policies and resource references use `${AWS::Partition}`
   which evaluates to `aws-us-gov` in GovCloud regions.
3. **Explicit artifact parameters** – Lambda deployment packages must be uploaded to a
   GovCloud S3 bucket and referenced via `LambdaCodeBucket`/`LambdaCodeKey` and
   `CustomResourceCodeBucket`/`CustomResourceCodeKey` parameters.
4. **Custom domain required** – There is no pre-defined Coralogix region mapping for GovCloud.
   You must always set `CustomDomain` to your Coralogix ingress endpoint.

---

## Prerequisites

- AWS CLI configured for a GovCloud account (`aws-us-gov` partition).
- A GovCloud S3 bucket to store Lambda deployment packages.
- A Coralogix ingress endpoint (e.g. `cx123.coralogix.com`).
- Your Coralogix Send Your Data API key.
- Build tooling for the Rust Lambda target architecture, plus `zip` for packaging.

The normal single Lambda build includes `aws-smithy-http-client` with
`rustls-aws-lc-fips`, which pulls in `aws-lc-fips-sys`. Build environments need
native prerequisites such as CMake and Go because AWS-LC FIPS is compiled as
part of the Rust dependency graph. GovCloud enables FIPS behavior only at
runtime through template environment variables.

### FIPS scope

This implementation enables AWS SDK FIPS endpoint resolution and the AWS-LC FIPS
HTTP client for AWS SDK traffic only. It does not assert full end-to-end Lambda
runtime or Coralogix egress FIPS compliance.

---

## Step 1 – Build Lambda Artifacts

### Shipper Lambda (Rust binary)

Build the shipper Lambda binary with the standard release command. The same
binary is used for commercial AWS and GovCloud; GovCloud enables FIPS at runtime
through template environment variables:

```bash
RUSTFLAGS="" RUST_ARCH=arm64 cargo lambda build --release --target aarch64-unknown-linux-gnu.2.17
```

That command writes the Lambda bootstrap to
`target/lambda/coralogix-aws-shipper/bootstrap`. Package that exact output:

```bash
cd target/lambda/coralogix-aws-shipper
zip ../../../bootstrap.zip bootstrap
```

GovCloud FIPS behavior is selected by `template-govcloud.yaml` at runtime via
`AWS_USE_FIPS_ENDPOINT=true` and `ENABLE_AWS_FIPS=true`, not by a separate Cargo
feature or build artifact.

### Custom Resource Lambda (Python)

The worktree includes the GovCloud custom resource at
`custom-resource-govcloud/index.py`. Package that file separately:

```bash
(cd custom-resource-govcloud && zip ../custom-resource-govcloud.zip index.py)
```

---

## Step 2 – Upload Artifacts to GovCloud S3

```bash
GOVCLOUD_BUCKET="my-govcloud-artifacts-bucket"
GOVCLOUD_REGION="us-gov-west-1"
VERSION="1.4.5"

# Upload shipper Lambda
aws s3 cp bootstrap.zip \
  s3://${GOVCLOUD_BUCKET}/coralogix-aws-shipper/${VERSION}/bootstrap.zip \
  --region ${GOVCLOUD_REGION}

# Upload custom resource Lambda
aws s3 cp custom-resource-govcloud.zip \
  s3://${GOVCLOUD_BUCKET}/coralogix-aws-shipper/${VERSION}/custom-resource-govcloud.zip \
  --region ${GOVCLOUD_REGION}
```

---

## Step 3 – Deploy the Stack

### Minimal example (S3 integration)

```bash
aws cloudformation deploy \
  --template-file template-govcloud.yaml \
  --stack-name coralogix-shipper-s3 \
  --region us-gov-west-1 \
  --capabilities CAPABILITY_IAM CAPABILITY_NAMED_IAM \
  --parameter-overrides \
    LambdaCodeBucket="${GOVCLOUD_BUCKET}" \
    LambdaCodeKey="coralogix-aws-shipper/${VERSION}/bootstrap.zip" \
    CustomResourceCodeBucket="${GOVCLOUD_BUCKET}" \
    CustomResourceCodeKey="coralogix-aws-shipper/${VERSION}/custom-resource-govcloud.zip" \
    CustomDomain="cx123.coralogix.com" \
    ApiKey="YOUR_CORALOGIX_API_KEY" \
    ApplicationName="my-app" \
    IntegrationType="S3" \
    S3BucketName="my-log-bucket"
```

### CloudWatch Logs integration

```bash
aws cloudformation deploy \
  --template-file template-govcloud.yaml \
  --stack-name coralogix-shipper-cloudwatch \
  --region us-gov-west-1 \
  --capabilities CAPABILITY_IAM CAPABILITY_NAMED_IAM \
  --parameter-overrides \
    LambdaCodeBucket="${GOVCLOUD_BUCKET}" \
    LambdaCodeKey="coralogix-aws-shipper/${VERSION}/bootstrap.zip" \
    CustomResourceCodeBucket="${GOVCLOUD_BUCKET}" \
    CustomResourceCodeKey="coralogix-aws-shipper/${VERSION}/custom-resource-govcloud.zip" \
    CustomDomain="cx123.coralogix.com" \
    ApiKey="YOUR_CORALOGIX_API_KEY" \
    ApplicationName="my-app" \
    IntegrationType="CloudWatch" \
    CloudWatchLogGroupName="/aws/lambda/my-function,/aws/lambda/my-other-function"
```

### SNS integration

```bash
aws cloudformation deploy \
  --template-file template-govcloud.yaml \
  --stack-name coralogix-shipper-sns \
  --region us-gov-west-1 \
  --capabilities CAPABILITY_IAM CAPABILITY_NAMED_IAM \
  --parameter-overrides \
    LambdaCodeBucket="${GOVCLOUD_BUCKET}" \
    LambdaCodeKey="coralogix-aws-shipper/${VERSION}/bootstrap.zip" \
    CustomResourceCodeBucket="${GOVCLOUD_BUCKET}" \
    CustomResourceCodeKey="coralogix-aws-shipper/${VERSION}/custom-resource-govcloud.zip" \
    CustomDomain="cx123.coralogix.com" \
    ApiKey="YOUR_CORALOGIX_API_KEY" \
    ApplicationName="my-app" \
    IntegrationType="Sns" \
    SNSIntegrationTopicArn="arn:aws-us-gov:sns:us-gov-west-1:123456789012:my-topic"
```

### Metrics mode

```bash
aws cloudformation deploy \
  --template-file template-govcloud.yaml \
  --stack-name coralogix-shipper-metrics \
  --region us-gov-west-1 \
  --capabilities CAPABILITY_IAM CAPABILITY_NAMED_IAM \
  --parameter-overrides \
    LambdaCodeBucket="${GOVCLOUD_BUCKET}" \
    LambdaCodeKey="coralogix-aws-shipper/${VERSION}/bootstrap.zip" \
    CustomResourceCodeBucket="${GOVCLOUD_BUCKET}" \
    CustomResourceCodeKey="coralogix-aws-shipper/${VERSION}/custom-resource-govcloud.zip" \
    CustomDomain="cx123.coralogix.com" \
    ApiKey="YOUR_CORALOGIX_API_KEY" \
    ApplicationName="my-app" \
    TelemetryMode="metrics" \
    S3BucketName="my-firehose-backup-bucket"
```

---

## Parameter Reference

### Required parameters (all deployments)

| Parameter | Description |
| --- | --- |
| `LambdaCodeBucket` | GovCloud S3 bucket containing the shipper Lambda zip |
| `LambdaCodeKey` | S3 key of the shipper Lambda zip |
| `CustomResourceCodeBucket` | GovCloud S3 bucket containing the GovCloud custom-resource Lambda zip |
| `CustomResourceCodeKey` | S3 key of the GovCloud custom-resource Lambda zip |
| `CustomDomain` | Coralogix ingress domain (e.g. `cx123.coralogix.com`) |
| `ApiKey` | Coralogix Send Your Data API key (or Secrets Manager ARN) |
| `ApplicationName` | Application name for log metadata |
| `IntegrationType` | One of: S3, CloudTrail, VpcFlow, CloudWatch, S3Csv, Sns, Sqs, Kinesis, CloudFront, Kafka, MSK, EcrScan |

### All other parameters

See the `Parameters` section of `template-govcloud.yaml` for the full list.
GovCloud mostly follows the commercial template contract, with known differences:

- `CoralogixRegion` is **removed** (not needed – always use `CustomDomain`).
- Starlark transform parameters and conditions, including `StarlarkScript` and
  `StarlarkScriptIsS3`, are **omitted** when they are absent from
  `template-govcloud.yaml`.
- `LambdaCodeBucket`, `LambdaCodeKey`, `CustomResourceCodeBucket`, `CustomResourceCodeKey`
  are **new** (required for artifact location).

---

## API Key in Secrets Manager

By default (`StoreAPIKeyInSecretsManager=true`), the API key is stored in AWS Secrets Manager.
The secret name follows the pattern:

```text
lambda/coralogix/{region}/coralogix-aws-shipper/{stack-id-suffix}
```

To use a pre-existing Secrets Manager secret, pass its ARN as the `ApiKey` parameter value.
The template detects ARN-format values and grants `secretsmanager:GetSecretValue` access
to the existing secret instead of creating a new one.

---

## PrivateLink

To use Coralogix PrivateLink:

```bash
--parameter-overrides \
  UsePrivateLink="true" \
  LambdaSubnetID="subnet-xxxxxxxx" \
  LambdaSecurityGroupID="sg-xxxxxxxx"
```

The endpoint URL will automatically use the `ingress.private.` prefix.

---

## Dead Letter Queue (DLQ)

```bash
--parameter-overrides \
  EnableDLQ="true" \
  DLQS3Bucket="my-dlq-bucket" \
  DLQRetryLimit="3" \
  DLQRetryDelay="900"
```

---

## Troubleshooting

### `AccessDenied` on Lambda `add_permission`

**Cause**: The custom-resource Lambda role does not have permission to call
`lambda:AddPermission` on the shipper function.

**Fix**: Verify that `CustomResourceFunctionRole` has the `LambdaAccess` policy
statement and that the stack deployed successfully.

### `InvalidParameterException: The provided principal was invalid`

**Cause**: A service principal used in `lambda:AddPermission` is incorrect.

**Fix**: In GovCloud, most event-source service principals remain the commercial
form, including `s3.amazonaws.com`, `events.amazonaws.com`, and
`sns.amazonaws.com`. CloudWatch Logs permissions are different in the GovCloud
custom resource: `custom-resource-govcloud/index.py` uses the regional principal
form `logs.{region}.amazonaws.com`.

### `arn:aws:` in error messages

**Cause**: A resource ARN was constructed with the commercial partition.

**Fix**: This should not occur with `template-govcloud.yaml` since all ARNs use
`${AWS::Partition}`. If you see this, check that you are using `template-govcloud.yaml`
and not `template.yaml`.

### Metrics mode: `ResourceNotFoundException` on metric stream

**Cause**: CloudWatch Metric Streams may not be available in all GovCloud regions.

**Fix**: Verify that `cloudwatch:PutMetricStream` is available in your target region.
`us-gov-west-1` supports CloudWatch Metric Streams. `us-gov-east-1` availability
should be verified before deploying with `TelemetryMode=metrics`.

### Stack stuck on `CREATE_IN_PROGRESS` for `ConfigureLambda`

**Cause**: The custom-resource Lambda failed to send a response to CloudFormation.

**Fix**: Check the custom-resource Lambda's CloudWatch log group
`/aws/lambda/{stack-name}-custom-resource` for error details.

---

## Known Limitations

- FIPS enablement applies to AWS SDK traffic only. It does not make a claim about
  full end-to-end Lambda runtime or Coralogix egress FIPS compliance.
- `CoralogixRegion` is not a parameter – GovCloud always requires `CustomDomain`.
- AWS Serverless Application Repository (SAR) is not supported in GovCloud.

---

## Cross-reference

- Commercial template: `template.yaml`
- GovCloud template: `template-govcloud.yaml`
- GovCloud custom resource: `custom-resource-govcloud/index.py`
- Compatibility matrix: `docs/govcloud-compatibility-matrix.md`
- Release checklist: `docs/govcloud-release-checklist.md`
