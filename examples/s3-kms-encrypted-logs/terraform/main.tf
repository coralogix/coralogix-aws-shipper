data "aws_iam_policy_document" "kms_base" {
  statement {
    sid    = "EnableAccountRootPermissions"
    effect = "Allow"

    principals {
      type        = "AWS"
      identifiers = ["arn:aws:iam::${data.aws_caller_identity.current.account_id}:root"]
    }

    actions   = ["kms:*"]
    resources = ["*"]
  }

  statement {
    sid    = "AllowS3ServiceUse"
    effect = "Allow"

    principals {
      type        = "Service"
      identifiers = ["s3.amazonaws.com"]
    }

    actions = [
      "kms:Decrypt",
      "kms:GenerateDataKey",
      "kms:DescribeKey",
    ]

    resources = ["*"]

    condition {
      test     = "StringEquals"
      variable = "kms:ViaService"
      values   = ["s3.${var.aws_region}.amazonaws.com"]
    }
  }
}

data "aws_iam_policy_document" "kms_shipper" {
  count = var.shipper_lambda_execution_role_arn != "" ? 1 : 0

  statement {
    sid    = "AllowShipperDecrypt"
    effect = "Allow"

    principals {
      type        = "AWS"
      identifiers = [var.shipper_lambda_execution_role_arn]
    }

    actions = [
      "kms:Decrypt",
      "kms:DescribeKey",
    ]

    resources = ["*"]
  }
}

data "aws_iam_policy_document" "kms_combined" {
  source_policy_documents = concat(
    [data.aws_iam_policy_document.kms_base.json],
    [for d in data.aws_iam_policy_document.kms_shipper : d.json]
  )
}

resource "aws_kms_key" "logs" {
  description             = "Coralogix shipper example — SSE-KMS for S3 log bucket"
  deletion_window_in_days = 7
  enable_key_rotation     = true
  tags                    = var.common_tags
  policy                  = data.aws_iam_policy_document.kms_combined.json
}

resource "aws_kms_alias" "logs" {
  name          = "alias/${var.name_prefix}-logs-kms"
  target_key_id = aws_kms_key.logs.key_id
}

resource "aws_s3_bucket" "logs" {
  bucket = "${var.name_prefix}-kms-logs-${data.aws_caller_identity.current.account_id}"
  tags   = var.common_tags
}

resource "aws_s3_bucket_public_access_block" "logs" {
  bucket = aws_s3_bucket.logs.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_server_side_encryption_configuration" "logs" {
  bucket = aws_s3_bucket.logs.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm     = "aws:kms"
      kms_master_key_id = aws_kms_key.logs.arn
    }
    bucket_key_enabled = true
  }
}
