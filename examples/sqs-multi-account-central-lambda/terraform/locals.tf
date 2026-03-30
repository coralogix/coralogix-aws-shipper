locals {
  spoke_queue_arn = aws_sqs_queue.spoke.arn
  spoke_queue_url = aws_sqs_queue.spoke.url

  # IAM "role" field for aws_iam_role_policy (path allowed, e.g. service-role/foo).
  shipper_execution_role_identifier = try(
    regex("^arn:aws:iam::\\d{12}:role/(.+)$", var.shipper_lambda_execution_role_arn)[0],
    ""
  )
}
