output "central_account_id" {
  description = "Central (Lambda) account ID."
  value       = data.aws_caller_identity.current.account_id
}

output "spoke_account_id" {
  description = "Spoke account ID."
  value       = data.aws_caller_identity.spoke.account_id
}

output "primary_sqs_queue_arn" {
  description = "Central-account primary queue ARN — use as SQSIntegrationTopicArn."
  value       = aws_sqs_queue.primary.arn
}

output "primary_sqs_queue_url" {
  description = "Central-account primary queue URL."
  value       = aws_sqs_queue.primary.url
}

output "additional_sqs_queue_arn" {
  description = "Spoke-account queue ARN (second event source)."
  value       = local.spoke_queue_arn
}

output "additional_sqs_queue_url" {
  description = "Spoke-account queue URL — send test messages here."
  value       = local.spoke_queue_url
}
