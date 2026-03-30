data "aws_caller_identity" "current" {}

data "aws_caller_identity" "spoke" {
  provider = aws.spoke
}
