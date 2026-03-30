variable "aws_region" {
  type        = string
  description = "Region for S3 buckets."
}

variable "name_prefix" {
  type        = string
  description = "Prefix for bucket names (lowercase letters, digits, hyphens)."
  default     = "coralogix-shipper-ex"
}

variable "common_tags" {
  type        = map(string)
  description = "Optional tags for created resources."
  default     = {}
}
