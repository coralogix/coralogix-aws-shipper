# Multiple S3 buckets, one shipper Lambda

## Shipper support

Parameter **`S3BucketName`** accepts a **comma-separated** list of bucket names (no spaces). The [custom resource](../../custom-resource/index.py) iterates each name: it adds Lambda permission per bucket and attaches **S3 event notifications** so `s3:ObjectCreated:*` invokes your function (subject to prefix/suffix filters).

Use the **same** `S3KeyPrefix` / `S3KeySuffix` for all buckets in that list; if paths differ per bucket, use separate stacks or a single bucket with a common prefix layout.

If **`SNSTopicArn`** or **`SQSTopicArn`** is set (non-placeholder), S3 notification setup via the custom resource is **skipped** in favor of SNS/SQS-driven ingestion—see the main [README](../../README.md) for that path.

## Terraform

[`terraform/`](terraform/) creates two sample buckets (public access blocked). After `terraform apply` there, deploy the shipper with:

`S3BucketName=<first-bucket>,<second-bucket>` using the Terraform output `bucket_names_csv`.

## Parameters

| Parameter | Example |
|-----------|---------|
| `IntegrationType` | `S3` (or `CloudTrail`, `VpcFlow`, …) |
| `S3BucketName` | `bucket-a,bucket-b` |
| `S3KeyPrefix` | optional shared prefix |
| `S3BucketKMSKeyARN` | optional; use if buckets use SSE-KMS (one key ARN for all buckets using that key) |

See [`parameters.example.json`](parameters.example.json).
