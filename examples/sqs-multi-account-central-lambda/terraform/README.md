# Terraform — central + spoke SQS (self-contained example)

This module always builds:

- **Primary** SQS queue in the **central** account (`provider "aws"`) — use as **`SQSIntegrationTopicArn`** when deploying the shipper.
- **Spoke** SQS queue in the **spoke** account (`provider "aws.spoke"`) — optional second **`aws_lambda_event_source_mapping`** from the central Lambda to this queue.

Configure **`aws.spoke`** in `providers.tf` so it authenticates to the spoke account. Typical options:

1. **`profile`** — e.g. `profile = "spoke-account"`.
2. **`assume_role`** — central credentials assume a role in the spoke account.

```hcl
provider "aws" {
  alias  = "spoke"
  region = var.spoke_region != "" ? var.spoke_region : var.aws_region
  profile = "your-spoke-profile"
}
```

Terraform also attaches an **inline IAM policy** on the shipper execution role for **`sqs:ReceiveMessage` / `DeleteMessage` / `GetQueueAttributes`** on the spoke queue only (the CloudFormation stack only grants SQS on `SQSIntegrationTopicArn`). Your Terraform identity needs **`iam:PutRolePolicy`** (and usually `iam:DeleteRolePolicy` on destroy) on that role.

## Bootstrap

1. Configure **`aws.spoke`** in `providers.tf`. Set **`aws_region`** / **`spoke_region`** in `terraform.tfvars` if needed.
2. **`terraform apply`** with `create_event_source_mapping = false` (and optionally defer **`shipper_lambda_execution_role_arn`** until after the stack exists).
3. Deploy the shipper in the **central** account with **`SQSIntegrationTopicArn`** = **`primary_sqs_queue_arn`**.
4. Set **`shipper_lambda_execution_role_arn`** and **`shipper_lambda_function_name`**, set **`create_event_source_mapping = true`**, run **`terraform apply`** again.

## Test spoke ingestion

After the mapping exists:

```bash
QUEUE_URL="$(terraform output -raw additional_sqs_queue_url)"
aws sqs send-message --queue-url "$QUEUE_URL" --message-body '{"message":"spoke test"}' --profile your-spoke-profile
```

Check the shipper Lambda in the **central** account (logs / Coralogix).
