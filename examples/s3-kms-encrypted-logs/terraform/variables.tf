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
  description = "Optional. When set, adds an explicit kms:Decrypt statement for this role on the CMK. Same-account deploys work with this empty: the key policy includes the account root, so the shipper stack's IAM policy can authorize decrypt after you deploy."
  default     = ""
}
