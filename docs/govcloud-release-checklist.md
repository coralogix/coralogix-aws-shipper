# GovCloud Release Checklist – coralogix-aws-shipper

Use this checklist for every GovCloud release. Complete all items before publishing
artifacts to production GovCloud S3.

---

## Pre-release: Build & Lint

- [ ] `cargo build` passes with no errors.
- [ ] `cargo clippy --all-targets -- -D warnings` passes with no warnings.
- [ ] `cargo test --all -- --nocapture` passes.
- [ ] Build environment has native prerequisites for `aws-lc-fips-sys`, including CMake and Go.
- [ ] `cfn-lint --template template-govcloud.yaml --region us-gov-west-1` exits 0.
- [ ] `cfn-lint --template template-govcloud.yaml --region us-gov-east-1` exits 0 (if in scope).
- [ ] No `arn:aws:` hardcoded strings in `template-govcloud.yaml` (search: `grep -n 'arn:aws:' template-govcloud.yaml`).
- [ ] No `arn:aws:` hardcoded strings in `custom-resource-govcloud/index.py`.
- [ ] `custom-resource-govcloud/index.py` Python syntax check: `python3 -m py_compile custom-resource-govcloud/index.py`.

---

## Artifact Packaging

- [ ] Build shipper Lambda binary for target architecture (arm64 or x86_64).

  ```bash
  RUSTFLAGS="" RUST_ARCH=arm64 cargo lambda build --release --target aarch64-unknown-linux-gnu.2.17
  ```

- [ ] GovCloud shipper Lambda uses the same standard build artifact as commercial AWS.
- [ ] Package shipper Lambda from the direct `cargo lambda` output:

  ```bash
  cd target/lambda/coralogix-aws-shipper
  zip ../../../bootstrap.zip bootstrap
  cd -
  ```

- [ ] GovCloud FIPS mode is enabled by template environment variables at runtime, not by a separate Cargo feature.
- [ ] Package the GovCloud custom resource from `custom-resource-govcloud/index.py`:

  ```bash
  (cd custom-resource-govcloud && zip ../custom-resource-govcloud.zip index.py)
  ```

- [ ] Verify zip contents:
  - `bootstrap.zip` contains `bootstrap` (ELF binary for target arch).
  - `custom-resource-govcloud.zip` contains `index.py`.

> **Note**: The `CustomResourceFunction` in `template-govcloud.yaml` uses
> `Handler: index.lambda_handler`, so the custom resource zip must contain `index.py`.

---

## GovCloud FIPS Template Verification

- [ ] `template-govcloud.yaml` sets `AWS_USE_FIPS_ENDPOINT=true`.
- [ ] `template-govcloud.yaml` sets `ENABLE_AWS_FIPS=true`.
- [ ] Runtime logs include `AWS FIPS mode enabled for SDK HTTP client` on cold start.
- [ ] FIPS scope is documented as AWS SDK FIPS endpoint resolution and AWS-LC FIPS HTTP client for AWS SDK traffic only.
- [ ] Commercial `template.yaml` does not set FIPS environment variables.

---

## Artifact Upload to GovCloud S3

- [ ] Upload `bootstrap.zip` to GovCloud S3 bucket.
- [ ] Upload `custom-resource-govcloud.zip` to GovCloud S3 bucket.
- [ ] Verify S3 object checksums match local files.
- [ ] Verify S3 objects are accessible from the target GovCloud account/region.

```bash
GOVCLOUD_BUCKET="my-govcloud-artifacts-bucket"
GOVCLOUD_REGION="us-gov-west-1"
VERSION="$(grep '^version' Cargo.toml | head -1 | awk -F'"' '{print $2}')"

aws s3 cp bootstrap.zip \
  s3://${GOVCLOUD_BUCKET}/coralogix-aws-shipper/${VERSION}/bootstrap.zip \
  --region ${GOVCLOUD_REGION}

aws s3 cp custom-resource-govcloud.zip \
  s3://${GOVCLOUD_BUCKET}/coralogix-aws-shipper/${VERSION}/custom-resource-govcloud.zip \
  --region ${GOVCLOUD_REGION}
```

---

## Integration Lifecycle Matrix

Run create → update → delete for each integration type. Mark pass (✓) or fail (✗).

| Integration | Create | Update | Delete | Notes |
| --- | --- | --- | --- | --- |
| S3 | | | | |
| CloudTrail | | | | |
| VpcFlow | | | | |
| S3Csv | | | | |
| CloudFront | | | | |
| CloudWatch (name list) | | | | |
| CloudWatch (prefix) | | | | |
| Sns | | | | |
| Sqs | | | | |
| Kinesis | | | | |
| MSK | | | | |
| Kafka | | | | |
| EcrScan | | | | |
| metrics mode | | | | |

---

## Key Condition Combinations

- [ ] `StoreAPIKeyInSecretsManager=true` (default) – secret created, Lambda reads from SM.
- [ ] `StoreAPIKeyInSecretsManager=false` – API key in plain-text env var.
- [ ] `ApiKey` as Secrets Manager ARN – existing secret, no new secret created.
- [ ] `EnableDLQ=true` – DLQ queue created, Lambda DLQ configured.
- [ ] `UsePrivateLink=true` – endpoint uses `ingress.private.` prefix, VPC config applied.
- [ ] `ExecutionRoleARN` set – custom execution role used, no `LambdaExecutionRole` created.
- [ ] `LambdaAssumeRoleARN` set – `sts:AssumeRole` permission added.
- [ ] `TelemetryMode=metrics` – Firehose, metric stream, and access role created.

---

## End-to-End Ingestion Verification

For each integration, inject a sample event and verify logs/metrics reach Coralogix.

- [ ] S3: Upload a sample log file to the watched bucket. Verify ingestion in Coralogix.
- [ ] CloudWatch: Write a log event to the subscribed log group. Verify ingestion.
- [ ] SNS: Publish a message to the SNS topic. Verify ingestion.
- [ ] SQS: Send a message to the SQS queue. Verify ingestion.
- [ ] Kinesis: Put a record to the Kinesis stream. Verify ingestion.
- [ ] metrics: Verify CloudWatch Metric Stream data reaches Coralogix metrics endpoint.
- [ ] metrics: Verify the target GovCloud region accepts the `streams.metrics.cloudwatch.amazonaws.com` principal for Metric Streams.
- [ ] EcrScan: Verify EventBridge delivers ECR scan events with source `aws.ecr` in the target GovCloud region.

---

## Partition / Principal Validation

- [ ] No `arn:aws:` in CloudFormation stack events or Lambda logs after deployment.
- [ ] IAM policy ARNs in deployed role show `arn:aws-us-gov:` prefix.
- [ ] `lambda:AddPermission` calls succeed (no `InvalidParameterException`).
- [ ] S3 bucket notification `SourceArn` shows `arn:aws-us-gov:s3:::bucket`.
- [ ] CloudWatch Logs `SourceArn` shows `arn:aws-us-gov:logs:...`.

---

## Idempotency Verification

- [ ] Run `create → update (same params) → update (changed params) → delete → delete` sequence.
- [ ] No fatal errors on second delete (ResourceNotFoundException handled gracefully).
- [ ] No duplicate S3 notification configurations after repeated updates.
- [ ] No duplicate CloudWatch subscription filters after repeated updates.

---

## Sign-off

| Item | Status | Notes |
| --- | --- | --- |
| Lint clean (both GovCloud regions) | | |
| Integration lifecycle matrix complete | | |
| Key condition combinations verified | | |
| FIPS runtime/template verification complete | | |
| End-to-end ingestion verified | | |
| Partition/principal validation passed | | |
| Idempotency verified | | |
| Deployment guide reviewed | | |

**Release decision**: Go / No-Go

**Approved by**: ___________________  **Date**: ___________________
