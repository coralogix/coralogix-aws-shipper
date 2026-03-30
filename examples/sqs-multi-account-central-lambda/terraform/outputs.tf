output "additional_sqs_queue_arn" {
  description = "ARN of the extra SQS queue."
  value       = aws_sqs_queue.additional.arn
}

output "additional_sqs_queue_url" {
  description = "URL of the extra SQS queue."
  value       = aws_sqs_queue.additional.url
}
