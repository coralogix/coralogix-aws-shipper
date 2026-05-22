# CloudWatch metric streams → Firehose → Shipper → Coralogix (PrivateLink)

Reference layout for the **metrics + PrivateLink** path described in the [main README](../../README.md#cloudwatch-metrics-streaming-via-privatelink-beta): a CloudWatch metric stream delivers OpenTelemetry to Kinesis Data Firehose, the shipper Lambda transforms records and **POSTs to `https://ingress.private.<coralogix-domain>`** from inside your VPC.

## Prerequisites

1. **Coralogix PrivateLink** — In the AWS region where you deploy, obtain the **interface endpoint service name** (and whether **private DNS** is supported) from [Coralogix AWS PrivateLink endpoints](https://coralogix.com/docs/coralogix-amazon-web-services-aws-privatelink-endpoints/). Pass it to Terraform as `coralogix_privatelink_service_name`.
2. **Region alignment** — Use an AWS region that matches your Coralogix domain and the **PrivateLink service name** from [Coralogix PrivateLink endpoints](https://coralogix.com/docs/coralogix-amazon-web-services-aws-privatelink-endpoints/). Example: domain **EU2** (`eu2.coralogix.com`) is tied to a specific AWS region in that table—do not assume `eu-north-1` vs `eu-west-1` without checking the doc.
3. **Shipper artifact** — To try the **latest published release** first, deploy from the [AWS Serverless Application Repository](https://serverlessrepo.aws.amazon.com/applications/arn:aws:serverlessrepo:us-east-1:532726568985:applications~coralogix-aws-shipper) in your target region, or build and deploy from a release tag of this repository (`sam build && sam deploy`).

**CloudFormation parameters vs template version** — `parameters.json` includes **metrics tag enrichment** parameters (`MetricsTagEnrichmentEnabled`, `MetricsContinueOnResourceFailure`, `MetricsFileCache*`). They exist only in **this repo’s** `template.yaml` (and newer releases); the **Serverless Application Repository** app may be older and **reject** those keys until it ships the same template.

## Update stack from this repo (`template.yaml`)

`template.yaml` is **larger than 51,200 bytes**, so `aws cloudformation update-stack --template-body file://template.yaml` will fail. Use **SAM** (uploads artifacts to S3 and expands the transform) or upload the template to S3 and pass **`--template-url`**.

From the **repository root** (replace stack name and region):

```bash
cd /path/to/coralogix-aws-shipper

PARAM_FILE=examples/firehose-metrics-private-link/parameters.json
OVERRIDES=$(jq -r '.[] | "\(.ParameterKey)=\(.ParameterValue)"' "$PARAM_FILE" | paste -sd' ' -)

sam deploy \
  --template-file template.yaml \
  --stack-name YOUR_STACK_NAME \
  --region eu-north-1 \
  --capabilities CAPABILITY_IAM CAPABILITY_NAMED_IAM CAPABILITY_AUTO_EXPAND \
  --resolve-s3 \
  --no-confirm-changeset \
  --parameter-overrides $OVERRIDES
```

- **`--resolve-s3`**: SAM uploads the Lambda build / template payload to a managed bucket (required for large templates and `CodeUri: .`).
- **`sam build`** is required first if the packaged Lambda binary is missing or stale:

```bash
sam build
# then run sam deploy ... as above
```

Pure **CloudFormation** (no `sam build` for Rust): upload `template.yaml` to an S3 bucket in the account, then:

```bash
aws cloudformation update-stack \
  --stack-name YOUR_STACK_NAME \
  --region eu-north-1 \
  --template-url https://YOUR_BUCKET.s3.eu-north-1.amazonaws.com/coralogix-aws-shipper-template.yaml \
  --parameters file://examples/firehose-metrics-private-link/parameters.json \
  --capabilities CAPABILITY_IAM CAPABILITY_NAMED_IAM CAPABILITY_AUTO_EXPAND
```

That path does **not** rebuild the Rust `bootstrap`; it only updates what the template and parameters describe. For a **new binary**, run **`sam build`** before **`sam deploy`**.

## Alignment with Coralogix PrivateLink docs ([overview](https://coralogix.com/docs/integrations/aws/aws-privatelink/aws-privatelink/))

The [architecture diagram](https://coralogix.com/docs/integrations/aws/aws-privatelink/aws-privatelink/) shows **your workload (including Lambda)** sending traffic to **VPC endpoint ENIs**, then **PrivateLink**, then Coralogix. This example matches that for **telemetry to Coralogix**:

| Doc concept | This example |
|-------------|----------------|
| Lambda in a VPC | Shipper `LambdaSubnetID` / `LambdaSecurityGroupID` point at Terraform private subnet + Lambda SG. |
| **Interface** VPC endpoint to Coralogix | `aws_vpc_endpoint.coralogix` in **private** subnets, **private DNS** enabled (for `ingress.private.*`). |
| Security on endpoint ENIs | `vpc_endpoints` security group allows **TCP 443** from the Lambda security group only. |

The overview states that for **PrivateLink to Coralogix** specifically, internet, NAT, and public IPs are **not required**. This repo still provisions a **NAT gateway** so the Lambda can reach **other** AWS APIs not covered by the VPC endpoints in this example (and, on shipper builds that support it, optional calls such as Resource Groups Tagging for metrics tag enrichment). That is a deliberate tradeoff for a small example; you can add more interface endpoints and remove NAT later for stricter “all private” egress.

For **Lambda + PrivateLink**, Coralogix calls out [additional endpoints](https://coralogix.com/docs/integrations/aws/aws-privatelink/aws-privatelink/#lambda-specific-considerations-high-level): **Secrets Manager** (interface) when using Secrets Manager, and **S3** (gateway) when the integration uses S3. Terraform creates those by default—see the [Lambda configuration](https://coralogix.com/docs/integrations/aws/aws-privatelink/aws-privatelink-lambda-configuration/) page, which states you **must** add a Secrets Manager VPC endpoint for VPC Lambdas using that service.

## What Terraform creates

[`terraform/`](terraform/) provisions:

- VPC with **public** and **private** subnets (two AZs)
- **NAT gateway** (single AZ) for AWS APIs that are not fronted by a VPC endpoint in this example
- **Security groups** for the Lambda placement and for interface VPC endpoints
- **Interface VPC endpoint** for **Coralogix** PrivateLink (service name you provide)
- **Interface VPC endpoint** for **Secrets Manager** (on by default; matches Coralogix Lambda + PrivateLink guidance)
- **Gateway VPC endpoint** for **S3** (on by default; Firehose and shipper S3 usage)
- **S3 bucket** for Firehose delivery / backup prefixes used by the shipper stack

Nothing else is assumed (no existing VPC or endpoints).

## Apply order

1. Set `coralogix_privatelink_service_name` in `terraform/terraform.tfvars` (from Coralogix documentation).
2. Run `terraform init` / `apply` in [`terraform/`](terraform/). Note outputs **`s3_bucket_name`**, **`lambda_subnet_id`**, **`lambda_security_group_id`**, **`vpc_id`**.
3. Copy [`parameters.example.json`](parameters.example.json) to `parameters.json` (or merge into your pipeline). Replace **`REPLACE_WITH_*`** placeholders with Terraform outputs and your Coralogix API key (or secret ARN).
4. Deploy the shipper stack (SAM or CloudFormation) using those parameters. Required highlights:
   - `TelemetryMode` = `metrics`
   - `IntegrationType` = `S3` (required so `S3BucketName` is validated and the Firehose S3 destination is valid)
   - `S3BucketName` = Terraform `s3_bucket_name`
   - `LambdaSubnetID` / `LambdaSecurityGroupID` = Terraform outputs
   - `UsePrivateLink` = `true` (satisfies stack validation and matches the PrivateLink ingress URL used when metrics mode is on)
   - `CoralogixRegion` / `CustomDomain` per your tenant

5. After deploy, confirm the **CloudWatch metric stream** and **Firehose** resources exist (created by the shipper template and custom resource). **[`parameters.example.json`](parameters.example.json)** sets a **narrow `MetricsFilter`** (a few `AWS/EC2` metrics) so new deployments do not stream the whole region by default. For a one-off “see everything” test, clear **`MetricsFilter`** and **`ExcludeMetricsFilters`** in your copy of **`parameters.json`** (empty = no include/exclude filters on the stream; **high volume and cost**).

## Parameters (illustrative)

| Parameter | Example |
|-----------|---------|
| `TelemetryMode` | `metrics` |
| `UsePrivateLink` | `true` |
| `IntegrationType` | `S3` |
| `S3BucketName` | From Terraform output `s3_bucket_name` |
| `LambdaSubnetID` | From Terraform output `lambda_subnet_id` |
| `LambdaSecurityGroupID` | From Terraform output `lambda_security_group_id` |
| `CoralogixRegion` | `EU1`, `US2`, or `Custom` with `CustomDomain` |
| `MetricsFilter` / `ExcludeMetricsFilters` | **Example file:** scoped **`AWS/EC2`** include list to limit usage. Set both **empty** only for full-region streaming (**expensive**; testing only). |
| `MetricsTagEnrichmentEnabled` | `true` — Resource Groups Tagging API enrichment for metrics (requires IAM in template; needs VPC path to `tagging` or NAT). |
| `MetricsContinueOnResourceFailure` | `true` — On tagging errors, ship metrics without AWS tags instead of failing the invocation. |
| `MetricsFileCacheEnabled` / `MetricsFileCachePath` / `MetricsFileCacheExpiration` | `true`, `/tmp`, `1h` — on-Lambda file cache for tag discovery. |

See [`parameters.example.json`](parameters.example.json) for the full parameter set. Use **`sam deploy`** from the repo root as in [Update stack from this repo](#update-stack-from-this-repo-templateyaml) (not raw `update-stack --template-body`, which hits the size limit).

## Cost note

**NAT** avoids listing every AWS interface endpoint (tagging API, CloudWatch Logs for VPC Lambda control plane, etc.) but adds hourly and data-processing cost. **Interface endpoints** for Coralogix and Secrets Manager also have per-AZ cost; the S3 **gateway** endpoint has no hourly charge.

## Parameters and `CoralogixRegion` (e.g. EU2)

Set **`CoralogixRegion`** in the shipper stack to match your tenant (**`EU2`** for `eu2.coralogix.com`, etc.). The stack maps that to **`CORALOGIX_ENDPOINT`** (PrivateLink uses the `ingress.private.*` host when `UsePrivateLink` is true and/or metrics mode per template). That FQDN must resolve via the Coralogix interface endpoint’s **private DNS** in your VPC.
