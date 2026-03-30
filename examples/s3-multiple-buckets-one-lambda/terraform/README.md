# Terraform — two S3 buckets

Independent state for this example only. Creates two buckets (public access blocked) to use with a single shipper stack via comma-separated `S3BucketName`.

## Usage

```bash
cd examples/s3-multiple-buckets-one-lambda/terraform
cp terraform.tfvars.example terraform.tfvars

terraform init
terraform apply
```

Use output `bucket_names_csv` as `S3BucketName` when deploying the shipper.
