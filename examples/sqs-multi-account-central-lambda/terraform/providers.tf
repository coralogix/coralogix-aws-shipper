provider "aws" {
  region = var.aws_region
}

# Spoke account — must point at a different account than provider "aws" (profile, assume_role, etc.).
provider "aws" {
  alias  = "spoke"
  region = var.spoke_region

  # Example (uncomment / adjust):
  # profile = "spoke-account"
  # assume_role { role_arn = "arn:aws:iam::222222222222:role/YourCrossAccountRole" }
}
