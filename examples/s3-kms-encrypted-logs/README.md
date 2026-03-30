# S3 logs encrypted with KMS

## Shipper support

When objects in your bucket are encrypted with SSE-KMS (customer-managed or AWS-managed), the Lambda needs permission to **decrypt**. Set template parameter **`S3BucketKMSKeyARN`** to the **KMS key ARN** used for bucket encryption.

The stack adds `kms:Decrypt` on that key ARN to the execution role when `S3BucketKMSKeyARN` is non-empty (see [template IAM conditions](../../template.yaml)).

## Single key, one or more buckets

`S3BucketKMSKeyARN` accepts **one** ARN. That fits:

- One bucket encrypted with that key, or  
- Several buckets listed in `S3BucketName` **if they use the same CMK** for default encryption.

If each bucket uses a **different** customer-managed key, resolve that by using one CMK for all relevant buckets or by deploying separate shipper stacks per key.

## Terraform

[`terraform/`](terraform/) in this folder provisions a CMK plus an encrypted bucket. The key policy includes the **account root**, so you do **not** need the shipper Lambda (or its execution role ARN) before the first `apply`: use Terraform outputs, deploy the shipper, then optionally run `apply` again with `shipper_lambda_execution_role_arn` for an explicit decrypt statement on the key. See [`terraform/README.md`](terraform/README.md) for the full order.

1. Run `terraform init` / `apply` in [`terraform/`](terraform/) and note outputs (`kms_key_arn`, `logs_bucket_name`).
2. Deploy the shipper with `IntegrationType` matching your log format (`S3`, `CloudTrail`, `VpcFlow`, etc.), `S3BucketName`, and `S3BucketKMSKeyARN` from Terraform output.
3. Ensure S3 notifications or your pipeline writes objects with encryption using that key.

## Parameters (illustrative)

| Parameter | Example |
|-----------|---------|
| `IntegrationType` | `S3` |
| `S3BucketName` | `my-cmk-logs-bucket` |
| `S3BucketKMSKeyARN` | `arn:aws:kms:eu-west-1:111111111111:key/aaa-bbb-ccc` |
| `S3KeyPrefix` / `S3KeySuffix` | Match your object layout |

See [`parameters.example.json`](parameters.example.json) for a fuller list of keys required by the CLI.
