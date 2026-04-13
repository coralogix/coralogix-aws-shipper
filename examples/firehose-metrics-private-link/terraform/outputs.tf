output "vpc_id" {
  description = "ID of the created VPC."
  value       = aws_vpc.main.id
}

output "s3_bucket_name" {
  description = "Pass to CloudFormation parameter S3BucketName (Firehose extended S3 destination)."
  value       = aws_s3_bucket.metrics_firehose.id
}

output "lambda_subnet_id" {
  description = "Pass to CloudFormation parameter LambdaSubnetID (private subnet with NAT)."
  value       = aws_subnet.private[0].id
}

output "lambda_security_group_id" {
  description = "Pass to CloudFormation parameter LambdaSecurityGroupID."
  value       = aws_security_group.lambda.id
}

output "coralogix_vpc_endpoint_id" {
  description = "Interface VPC endpoint for Coralogix PrivateLink."
  value       = aws_vpc_endpoint.coralogix.id
}

output "secretsmanager_vpc_endpoint_id" {
  description = "Interface VPC endpoint for Secrets Manager (null if create_secrets_manager_interface_endpoint is false)."
  value       = length(aws_vpc_endpoint.secretsmanager) > 0 ? aws_vpc_endpoint.secretsmanager[0].id : null
}

output "s3_gateway_vpc_endpoint_id" {
  description = "Gateway VPC endpoint for S3 (null if create_s3_gateway_endpoint is false)."
  value       = length(aws_vpc_endpoint.s3) > 0 ? aws_vpc_endpoint.s3[0].id : null
}

output "private_subnet_ids" {
  description = "All private subnets (e.g. if you extend the template to multiple subnets later)."
  value       = aws_subnet.private[*].id
}
