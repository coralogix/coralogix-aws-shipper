variable "aws_region" {
  type        = string
  description = "AWS region for VPC, S3, and the shipper stack (must align with Coralogix PrivateLink for that region)."
  default     = "eu-north-1"
}

variable "name_prefix" {
  type        = string
  description = "Prefix for resource names (lowercase letters, digits, hyphens)."
  default     = "cx-shipper-ex"
}

variable "common_tags" {
  type        = map(string)
  description = "Optional tags for created resources."
  default     = {}
}

variable "vpc_cidr" {
  type        = string
  description = "CIDR for the new VPC."
  default     = "10.42.0.0/16"
}

variable "az_count" {
  type        = number
  description = "Number of availability zones (2 recommended)."
  default     = 2

  validation {
    condition     = var.az_count >= 2 && var.az_count <= 6
    error_message = "az_count must be between 2 and 6."
  }
}

variable "coralogix_privatelink_service_name" {
  type        = string
  description = "AWS PrivateLink interface endpoint service name for Coralogix in this region (from Coralogix documentation)."
  default     = "com.amazonaws.vpce.eu-north-1.vpce-svc-041b21c87be842c08"
}

variable "create_secrets_manager_interface_endpoint" {
  type        = bool
  description = "Create an interface VPC endpoint for Secrets Manager. Required for private-only egress when using StoreAPIKeyInSecretsManager (see Coralogix PrivateLink Lambda configuration)."
  default     = true
}

variable "create_s3_gateway_endpoint" {
  type        = bool
  description = "Create a gateway VPC endpoint for S3. Coralogix recommends this when the integration uses Amazon S3 (Firehose writes to your bucket; shipper may use S3 APIs)."
  default     = true
}

variable "coralogix_privatelink_private_dns_enabled" {
  type        = bool
  description = "Set true if Coralogix documents private DNS for this endpoint (ingress.private.* resolution inside the VPC)."
  default     = true
}
