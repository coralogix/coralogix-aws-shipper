# Central shipper Lambda with multiple SQS queues (incl. cross-account)

## How the shipper handles SQS

The [template `SQSEventSourceMapping`](../../template.yaml) wires **one** SQS ARN from `SQSIntegrationTopicArn` (when `IntegrationType` is `Sqs`). That is enough for a single queue.

To read from **additional** queues with the **same** Lambda (for example queues in other AWS accounts), add one **Lambda event source mapping** per queue. The queue must trust the **shipper Lambda execution role** (the IAM role assumed when your function runs) for `sqs:ReceiveMessage`, `sqs:DeleteMessage`, and `sqs:GetQueueAttributes`.

## Recommended pattern

1. **Primary queue** — Deploy the CloudFormation/SAM stack with `IntegrationType=Sqs` and `SQSIntegrationTopicArn` set to this queue’s ARN (create the queue first if needed).
2. **More queues** — For every extra queue (same account or cross-account), attach a queue policy that allows the central Lambda role, then create an event source mapping on the central function.

For **cross-account** queues, the mapping is still created in the **central** account (where the Lambda runs); only the `event_source_arn` points at the foreign queue ARN. The queue policy on the spoke queue must allow the central account’s Lambda execution role.

## Terraform (self-contained central + spoke)

[`terraform/`](terraform/) always creates:

- A **primary** queue in the **central** account (for **`SQSIntegrationTopicArn`**).
- A **spoke** queue in a **second** account via **`aws.spoke`** in [`terraform/providers.tf`](terraform/providers.tf).

So you can test cross-account ingestion with only this Terraform + a shipper stack—no other queues required. Bootstrap and testing steps are in [`terraform/README.md`](terraform/README.md).

## Parameters

| Template parameter | Value |
|--------------------|--------|
| `IntegrationType` | `Sqs` |
| `SQSIntegrationTopicArn` | ARN of the **first** queue (`primary_sqs_queue_arn` from Terraform) |
| `S3BucketName` | `none` when using pure SQS ingestion (see template rules for other integration types) |

For pure SQS, set other product-specific parameters to their defaults or placeholders as required by your deployment path; follow the main [README](../../README.md) for required Coralogix fields (`ApiKey`, `ApplicationName`, etc.).

## Cross-account checklist (spoke account queue)

- Queue policy `Principal` = ARN of the **execution role** used by the shipper Lambda in the central account.
- Actions: `sqs:ReceiveMessage`, `sqs:DeleteMessage`, `sqs:GetQueueAttributes` (and `sqs:GetQueueUrl` if your org requires it).
- In the central account, create `aws_lambda_event_source_mapping` with `event_source_arn` = the spoke queue ARN.

Optional consolidation pattern: forward everything into **one** central queue (SNS fan-in, custom producers, etc.) and use only the single built-in mapping—no extra mappings required.
