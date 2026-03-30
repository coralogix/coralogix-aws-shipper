variable "aws_region" {
  type        = string
  description = "Region where the SQS queue and shipper Lambda live."
}

variable "name_prefix" {
  type        = string
  description = "Prefix for the SQS queue name (lowercase letters, digits, hyphens)."
  default     = "coralogix-shipper-ex"
}

variable "common_tags" {
  type        = map(string)
  description = "Optional tags for created resources."
  default     = {}
}

variable "shipper_lambda_execution_role_arn" {
  type        = string
  description = "IAM execution role ARN of the shipper Lambda — required for the queue policy before the Lambda can consume messages."
  default     = ""
}

variable "shipper_lambda_function_name" {
  type        = string
  description = "Lambda function name or ARN — set when create_event_source_mapping is true."
  default     = ""
}

variable "create_event_source_mapping" {
  type        = bool
  description = "After the shipper stack exists, set true to attach this queue to the Lambda."
  default     = false
}

variable "sqs_visibility_timeout_seconds" {
  type        = number
  description = "SQS visibility timeout — should be >= the shipper Lambda timeout."
  default     = 360
}
