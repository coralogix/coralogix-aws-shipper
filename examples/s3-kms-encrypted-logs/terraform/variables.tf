variable "aws_region" {
  type        = string
  description = "Region for KMS key and S3 bucket."
}

variable "name_prefix" {
  type        = string
  description = "Prefix for resource names (lowercase letters, digits, hyphens)."
  default     = "coralogix-shipper-ex"
}

variable "common_tags" {
  type        = map(string)
  description = "Optional tags for created resources."
  default     = {}
}

variable "shipper_lambda_execution_role_arn" {
  type        = string
  description = "IAM execution role ARN of the shipper Lambda — when set, the KMS key policy grants kms:Decrypt for SSE-KMS reads."
  default     = ""
}
