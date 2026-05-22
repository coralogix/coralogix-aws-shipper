# Coralogix AWS Shipper — configuration examples

These folders are **reference patterns** for frequently asked deployment scenarios. The shipper is configured through **CloudFormation / SAM parameters** (or the [Terraform module](https://github.com/coralogix/terraform-coralogix-aws/tree/master/modules/coralogix-aws-shipper)), not a standalone config file like an OpenTelemetry Collector YAML.

Each scenario includes:

- **README** — architecture notes, parameter mapping, and apply order.
- **parameters.example.json** — starter parameter file for `aws cloudformation deploy` (replace secrets and ARNs).

Optional **dependent AWS resources** for each scenario live in that folder’s **`terraform/`** subdirectory with its **own Terraform state** (run `init` / `apply` per example).

## Scenarios

| Folder | Topic |
|--------|--------|
| [firehose-metrics-private-link](firehose-metrics-private-link/) | Greenfield VPC (NAT + Coralogix interface endpoint), S3 for Firehose, and CloudFormation parameters for metrics over PrivateLink. |
| [sqs-multi-account-central-lambda](sqs-multi-account-central-lambda/) | Central + spoke SQS queues (two AWS accounts) and optional second event source on the shipper Lambda. |
| [s3-kms-encrypted-logs](s3-kms-encrypted-logs/) | S3 logs encrypted with a customer-managed KMS key and `S3BucketKMSKeyARN`. |
| [s3-multiple-buckets-one-lambda](s3-multiple-buckets-one-lambda/) | Comma-separated `S3BucketName` so one integration watches several buckets. |
