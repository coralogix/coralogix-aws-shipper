# Terraform — VPC, NAT, Coralogix PrivateLink endpoint, S3 for Firehose

Independent state for this example. Provisions networking and storage **only**; you deploy the Coralogix AWS Shipper stack separately (SAM, SAR, or CloudFormation).

## Before `apply`

1. From [Coralogix AWS PrivateLink endpoints](https://coralogix.com/docs/coralogix-amazon-web-services-aws-privatelink-endpoints/), copy the **service name** for your Coralogix region and AWS region into `coralogix_privatelink_service_name` in `terraform.tfvars`.
2. Set `aws_region` to the same region where you will deploy the shipper and metric stream.

## Usage

```bash
cd examples/firehose-metrics-private-link/terraform
cp terraform.tfvars.example terraform.tfvars
# Edit terraform.tfvars — service name is required.

terraform init
terraform apply
```

Use outputs **`s3_bucket_name`**, **`lambda_subnet_id`**, and **`lambda_security_group_id`** in [`../parameters.example.json`](../parameters.example.json) (replace `REPLACE_WITH_*` placeholders).

## Deploy the shipper (latest SAR release)

1. In the AWS console, open the Serverless Application Repository and search for **coralogix-aws-shipper**, or use the [application page](https://serverlessrepo.aws.amazon.com/applications/arn:aws:serverlessrepo:us-east-1:532726568985:applications~coralogix-aws-shipper) (deploy into your **target** region, e.g. the same as `aws_region` above).
2. Choose the **latest semantic version**, then under **Application settings** supply the same parameter keys as in [`../parameters.example.json`](../parameters.example.json) (after replacing `REPLACE_WITH_*` with Terraform outputs and your API key).

Alternatively, from a clone of this repo at a release tag:

```bash
sam build --use-container
sam deploy --guided
```

Pass the same parameters as in `parameters.example.json`.

## CloudFormation CLI parameters file

The JSON array in `parameters.example.json` matches `aws cloudformation create-stack --parameters file://parameters.json`.

For `aws cloudformation deploy`, convert overrides, for example:

```bash
jq -r '.[] | "\(.ParameterKey)=\(.ParameterValue)"' ../parameters.json | paste -sd' ' -
# Paste as: aws cloudformation deploy ... --parameter-overrides Key1=Val1 Key2=Val2 ...
```

Or use `--parameters file://` with `create-stack` / `update-stack` if you prefer the array format unchanged.
