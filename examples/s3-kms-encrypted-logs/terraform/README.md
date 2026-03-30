# Terraform — KMS key + SSE-KMS S3 bucket

Independent state for this example only. Provisions a customer-managed key and an encrypted bucket for logs consumed by the shipper.

## Bootstrap order

1. **`terraform apply`** with `shipper_lambda_execution_role_arn` left empty (or unset in `terraform.tfvars`). Note outputs **`kms_key_arn`** and **`logs_bucket_name`**.
2. **Deploy the shipper** CloudFormation stack with `S3BucketName`, `S3BucketKMSKeyARN` (from step 1), and your Coralogix settings. The stack attaches `kms:Decrypt` on that key to the function’s execution role.
3. **(Optional)** Copy the execution role ARN from the stack outputs, set `shipper_lambda_execution_role_arn` in `terraform.tfvars`, and run **`terraform apply`** again to add the explicit shipper principal to the key policy.

## Usage

```bash
cd examples/s3-kms-encrypted-logs/terraform
cp terraform.tfvars.example terraform.tfvars

terraform init
terraform apply
```

Use outputs `kms_key_arn` and `logs_bucket_name` in the shipper CloudFormation parameters.
