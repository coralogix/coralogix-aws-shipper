# Terraform — extra SQS queue + optional event source mapping

Independent state for this example only. Creates an **additional** SQS queue (besides the one you pass as `SQSIntegrationTopicArn` in CloudFormation) and optionally attaches it to the existing shipper Lambda.

## Usage

```bash
cd examples/sqs-multi-account-central-lambda/terraform
cp terraform.tfvars.example terraform.tfvars
# Edit aws_region, shipper_lambda_execution_role_arn, etc.

terraform init
terraform apply
```

1. Apply with `create_event_source_mapping = false` and a non-empty `shipper_lambda_execution_role_arn` → queue + policy.
2. Deploy the shipper stack with `SQSIntegrationTopicArn` pointing at your **primary** queue.
3. Set `shipper_lambda_function_name` and `create_event_source_mapping = true`, then `terraform apply` again.

## Cross-account

For a queue in another account, adapt the queue policy principal to the central shipper execution role and create the `aws_lambda_event_source_mapping` in the **central** account (as this module does), with `event_source_arn` set to the spoke queue ARN.
