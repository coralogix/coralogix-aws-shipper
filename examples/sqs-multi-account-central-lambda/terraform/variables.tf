variable "aws_region" {
  type        = string
  description = "Region for the central (Lambda) account — primary queue and event source mapping."
}

variable "spoke_region" {
  type        = string
  description = "Region for the spoke-account SQS queue. If empty, uses aws_region (same region, different account)."
  default     = ""
}

variable "name_prefix" {
  type        = string
  description = "Prefix for SQS queue names (lowercase letters, digits, hyphens)."
  default     = "coralogix-shipper-ex"
}

variable "common_tags" {
  type        = map(string)
  description = "Optional tags for created resources."
  default     = {}
}

variable "shipper_lambda_execution_role_arn" {
  type        = string
  description = "Central account IAM execution role ARN of the shipper Lambda — used for the spoke queue policy and for an inline IAM policy on that role (ReceiveMessage/DeleteMessage/GetQueueAttributes on the spoke queue only). Requires iam:PutRolePolicy on the Terraform principal."
  default     = ""
}

variable "shipper_lambda_function_name" {
  type        = string
  description = "Lambda function name or ARN in the central account — set when create_event_source_mapping is true."
  default     = ""
}

variable "create_event_source_mapping" {
  type        = bool
  description = "After the shipper stack exists, set true to attach the spoke queue to the Lambda (mapping is created in the central account)."
  default     = false
}

variable "sqs_visibility_timeout_seconds" {
  type        = number
  description = "SQS visibility timeout — should be >= the shipper Lambda timeout."
  default     = 360
}
