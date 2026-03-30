# Terraform — KMS key + SSE-KMS S3 bucket

Independent state for this example only. Provisions a customer-managed key and an encrypted bucket for logs consumed by the shipper.

## Usage

```bash
cd examples/s3-kms-encrypted-logs/terraform
cp terraform.tfvars.example terraform.tfvars

terraform init
terraform apply
```

Set `shipper_lambda_execution_role_arn` so the key policy includes `kms:Decrypt` for the shipper (you can add it later and run `apply` again).

Use outputs `kms_key_arn` and `logs_bucket_name` in the shipper CloudFormation parameters.
