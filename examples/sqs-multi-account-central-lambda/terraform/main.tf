# Primary queue in the central account — pass its ARN as SQSIntegrationTopicArn.
resource "aws_sqs_queue" "primary" {
  name                       = "${var.name_prefix}-sqs-primary-${data.aws_caller_identity.current.account_id}"
  visibility_timeout_seconds = var.sqs_visibility_timeout_seconds
  tags                       = var.common_tags
}

# Additional queue in the spoke account — Lambda in central polls this via a second event source mapping.
resource "aws_sqs_queue" "spoke" {
  provider = aws.spoke

  name                       = "${var.name_prefix}-sqs-spoke-${data.aws_caller_identity.spoke.account_id}"
  visibility_timeout_seconds = var.sqs_visibility_timeout_seconds
  tags                       = var.common_tags
}

data "aws_iam_policy_document" "spoke_queue" {
  count = var.shipper_lambda_execution_role_arn != "" ? 1 : 0

  statement {
    sid    = "AllowShipperLambdaConsume"
    effect = "Allow"

    principals {
      type        = "AWS"
      identifiers = [var.shipper_lambda_execution_role_arn]
    }

    actions = [
      "sqs:ReceiveMessage",
      "sqs:DeleteMessage",
      "sqs:GetQueueAttributes",
    ]

    resources = [aws_sqs_queue.spoke.arn]
  }
}

resource "aws_sqs_queue_policy" "spoke" {
  count    = length(data.aws_iam_policy_document.spoke_queue) > 0 ? 1 : 0
  provider = aws.spoke

  queue_url = aws_sqs_queue.spoke.url
  policy    = data.aws_iam_policy_document.spoke_queue[0].json
}

# CloudFormation only grants sqs:ReceiveMessage on SQSIntegrationTopicArn — extra queues need role permission too.
resource "aws_iam_role_policy" "spoke_sqs_consume" {
  count = local.shipper_execution_role_identifier != "" ? 1 : 0

  name = "${var.name_prefix}-examples-spoke-sqs"
  role = local.shipper_execution_role_identifier

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Sid    = "CoralogixShipperExampleSpokeQueue"
      Effect = "Allow"
      Action = [
        "sqs:ReceiveMessage",
        "sqs:DeleteMessage",
        "sqs:GetQueueAttributes",
      ]
      Resource = aws_sqs_queue.spoke.arn
    }]
  })
}

resource "aws_lambda_event_source_mapping" "spoke" {
  count = var.create_event_source_mapping && var.shipper_lambda_function_name != "" && var.shipper_lambda_execution_role_arn != "" ? 1 : 0

  event_source_arn = aws_sqs_queue.spoke.arn
  function_name    = var.shipper_lambda_function_name
  batch_size       = 10

  depends_on = [
    aws_sqs_queue_policy.spoke,
    aws_iam_role_policy.spoke_sqs_consume,
  ]
}
