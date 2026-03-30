output "bucket_names_csv" {
  description = "Comma-separated names for CloudFormation S3BucketName (no spaces)."
  value       = join(",", [aws_s3_bucket.a.id, aws_s3_bucket.b.id])
}

output "bucket_a_id" {
  value = aws_s3_bucket.a.id
}

output "bucket_b_id" {
  value = aws_s3_bucket.b.id
}
