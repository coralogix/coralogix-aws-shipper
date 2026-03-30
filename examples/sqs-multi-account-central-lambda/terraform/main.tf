resource "aws_sqs_queue" "additional" {
  name                       = "${var.name_prefix}-sqs-additional-${data.aws_caller_identity.current.account_id}"
  visibility_timeout_seconds = var.sqs_visibility_timeout_seconds
  tags                       = var.common_tags
}

data "aws_iam_policy_document" "additional_queue" {
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

    resources = [aws_sqs_queue.additional.arn]
  }
}

resource "aws_sqs_queue_policy" "additional" {
  count = length(data.aws_iam_policy_document.additional_queue) > 0 ? 1 : 0

  queue_url = aws_sqs_queue.additional.id
  policy    = data.aws_iam_policy_document.additional_queue[0].json
}

resource "aws_lambda_event_source_mapping" "additional" {
  count = var.create_event_source_mapping && var.shipper_lambda_function_name != "" && var.shipper_lambda_execution_role_arn != "" ? 1 : 0

  event_source_arn = aws_sqs_queue.additional.arn
  function_name    = var.shipper_lambda_function_name
  batch_size       = 10

  depends_on = [aws_sqs_queue_policy.additional]
}
