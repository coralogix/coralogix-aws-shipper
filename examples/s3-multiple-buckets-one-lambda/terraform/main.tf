resource "aws_s3_bucket" "a" {
  bucket = "${var.name_prefix}-multi-a-${data.aws_caller_identity.current.account_id}"
  tags   = var.common_tags
}

resource "aws_s3_bucket" "b" {
  bucket = "${var.name_prefix}-multi-b-${data.aws_caller_identity.current.account_id}"
  tags   = var.common_tags
}

resource "aws_s3_bucket_public_access_block" "a" {
  bucket = aws_s3_bucket.a.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_public_access_block" "b" {
  bucket = aws_s3_bucket.b.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}
