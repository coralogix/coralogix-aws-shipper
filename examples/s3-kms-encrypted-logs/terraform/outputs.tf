output "kms_key_arn" {
  description = "Pass to CloudFormation parameter S3BucketKMSKeyARN."
  value       = aws_kms_key.logs.arn
}

output "logs_bucket_name" {
  description = "Pass to CloudFormation parameter S3BucketName."
  value       = aws_s3_bucket.logs.id
}
