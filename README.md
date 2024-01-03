# coralogix-aws-shipper

[![license](https://img.shields.io/github/license/coralogix/coralogix-aws-shipper.svg)](https://raw.githubusercontent.com/coralogix/coralogix-aws-shipper/master/LICENSE)
![publish workflow](https://github.com/coralogix/coralogix-aws-shipper/actions/workflows/publish.yaml/badge.svg)
![Dynamic TOML Badge](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fcoralogix%2Fcoralogix-aws-shipper%2Fmaster%2FCargo.toml&query=%24.package.version&label=version)
[![Rust Report Card](https://rust-reportcard.xuri.me/badge/github.com/coralogix/coralogix-aws-shipper)](https://rust-reportcard.xuri.me/report/github.com/coralogix/coralogix-aws-shipper)
![Static Badge](https://img.shields.io/badge/status-beta-purple)

## Overview
Coralogix provides a predefined AWS Lambda function to easily forward your logs to the Coralogix platform.

The `coralogix-aws-shipper` supports forwarding of logs for the following AWS Services:

* [Amazon CloudWatch](https://docs.aws.amazon.com/cloudwatch/)
* [AWS CloudTrail](https://docs.aws.amazon.com/awscloudtrail/latest/userguide/cloudtrail-log-file-examples.html)
* [Amazon VPC Flow logs](https://docs.aws.amazon.com/vpc/latest/userguide/flow-logs-s3.html)
* AWS Elastic Load Balancing access logs ([ALB](https://docs.aws.amazon.com/elasticloadbalancing/latest/application/load-balancer-access-logs.html), [NLB](https://docs.aws.amazon.com/elasticloadbalancing/latest/network/load-balancer-access-logs.html) and [ELB](https://docs.aws.amazon.com/elasticloadbalancing/latest/classic/access-log-collection.html))
* [Amazon CloudFront](https://docs.aws.amazon.com/AmazonCloudFront/latest/DeveloperGuide/AccessLogs.html)
* [AWS Network Firewall](https://docs.aws.amazon.com/network-firewall/latest/developerguide/logging-s3.html)
* [Amazon Redshift](https://docs.aws.amazon.com/redshift/latest/mgmt/db-auditing.html#db-auditing-manage-log-files)
* [Amazon S3 access logs](https://docs.aws.amazon.com/AmazonS3/latest/userguide/ServerLogs.html)
* [Amazon VPC DNS query logs](https://docs.aws.amazon.com/Route53/latest/DeveloperGuide/resolver-query-logs.html)
* [AWS WAF](https://docs.aws.amazon.com/waf/latest/developerguide/logging-s3.html)
* [AWS SNS](https://aws.amazon.com/sns/)
* [AWS SQS](https://aws.amazon.com/sqs/)

Additionally, you can ingest any generic text, JSON and csv logs stored in your S3 bucket

## Prerequisites

* AWS account (Your AWS user should have permissions to create lambdas and IAM roles)
* Coralogix account
* The application should be installed in the same AWS region as your resource are (i.e the S3 bucket you want to send the logs from)

## Deployment instructions

### Coralogix In Product Integration

Link To Coralogix Document (Work in Progress)

### AWS CloudFormation Application

Log into your AWS account and deploy the CloudFormation Stack with the button below
[![Launch Stack](https://s3.amazonaws.com/cloudformation-examples/cloudformation-launch-stack.png)](https://console.aws.amazon.com/cloudformation/home#/stacks/create/review?stackName=coralogix-aws-shipper&templateURL=https://cgx-cloudformation-templates.s3.amazonaws.com/aws-integrations/aws-shipper-lambda/template.yaml)

### AWS Serverless Application

The lambda can be deployed by clicking the link below and signing into your AWS account:
[Deployment link](https://serverlessrepo.aws.amazon.com/applications/eu-central-1/597078901540/coralogix-aws-shipper)
Please make sure you selecet the AWS region before you deploy

### Terraform

If you need to deploy using Terraform, you can use Coralogix Module located in:
https://github.com/coralogix/terraform-coralogix-aws/tree/master/modules/coralogix-aws-shipper

## Paramaters 

### Coralogix configuration
| Parameter | Description | Default Value | Required |
|---|---|---|---|
| Application name | The stack name of this application created via AWS CloudFormation. |   | :heavy_check_mark: |
| IntegrationType | The integration type. Can be one of: S3, CloudTrail, VpcFlow, CloudWatch, S3Csv, Sns' |  S3 | :heavy_check_mark: | 
| CoralogixRegion | The Coralogix location region, possible options are [Custom, EU1, EU2, AP1, AP2, US, US2] If this value is set to Custom you must specify the Custom Domain to use via the CustomDomain parameter |  Custom | :heavy_check_mark: | 
| CustomDomain | The Custom Domain. If set, will be the domain used to send telemetry (e.g. cx123.coralogix.com) |   |   |
| ApplicationName | The [name](https://coralogix.com/docs/application-and-subsystem-names/) of your application. For dynamically value check [Advanced section](#advanced)|   | :heavy_check_mark: | 
| SubsystemName | The [name](https://coralogix.com/docs/application-and-subsystem-names/) of your subsystem. For dynamic value from the check [Advanced section](#advanced). For Cloudwatch leave empty to use the loggroup name.|   |   |
| ApiKey | Your Coralogix Send Your Data - [API Key](https://coralogix.com/docs/send-your-data-api-key/) which is used to validate your authenticity, This value can be a Coralogix API Key or an AWS Secret Manager ARN that holds the API Key |   | :heavy_check_mark: |
| StoreAPIKeyInSecretsManager | Store the API key in AWS Secrets Manager.  If this option is set to false, the ApiKey will apeear in plain text as an environment variable in the lambda function console. | True  | :heavy_check_mark: |

### Integration S3/CloudTrail/VpcFlow/S3Csv configuration
| Parameter | Description | Default Value | Required |
|---|---|---|---|
| S3BucketName | The name of the AWS S3 bucket to watch |   | :heavy_check_mark: |
| S3KeyPrefix | The AWS S3 path prefix to watch. This value is ignored when the SNSTopicArn parameter is provided. | CloudTrail/VpcFlow 'AWSLogs/' |   |
| S3KeySuffix | The AWS S3 path suffix to watch. This value is ignored when the SNSTopicArn parameter is provided. | CloudTrail '.json.gz',  VpcFlow '.log.gz' |   |
| NewlinePattern | Regular expression to detect a new log line for multiline logs from S3 source, e.g., use expression \n(?=\d{2}\-\d{2}\s\d{2}\:\d{2}\:\d{2}\.\d{3}) |   |   |
| SNSTopicArn | The ARN for the SNS topic that contains the SNS subscription responsible for retrieving logs from Amazon S3 |   |   |
| SQSTopicArn | The ARN for the SQS queue that contains the SQS subscription responsible for retrieving logs from Amazon S3 |   |   |
| CSVDelimiter | Single Character for using as a Delimiter when ingesting CSV file with header line (This value is applied when the S3Csv integration type  is selected), e.g. "," or " " | , |   |

### Integration Cloudwatch configuration
| Parameter | Description | Default Value | Required |
|---|---|---|---|
| CloudWatchLogGroupName | A comma separated list of CloudWatch log groups names to watch  e.g, (log-group1,log-group2,log-group3) |   | :heavy_check_mark: | 

### Integration SNS configuration
| Parameter | Description | Default Value | Required |
|---|---|---|---|
| SNSIntegrationTopicARN | The ARN of SNS topic to subscribe to retrieving messages |   | :heavy_check_mark: | 

### Integration SQS configuration
| Parameter | Description | Default Value | Required |
|---|---|---|---|
| SQSIntegrationTopicARN | The ARN of SQS queue to subscribe to retrieving messages |   | :heavy_check_mark: |

### Integration Generic Config (Optional)
| Parameter | Description | Default Value | Required |
|---|---|---|---|
| NotificationEmail | Failure notification email address |   |   | 
| BlockingPattern | Regular expression to detect lines that should be excluded from sent to Coralogix, e.g., use expression MainActivity.java\:\d{3} to match all log that MainActivity ends with 3 digits, This will block a specific ipaddr in a json ' "srcaddr"\:"172\.31\.24\.253" '|  |   | 
| SamplingRate | Send messages with specific rate (1 out of N) e.g., put the value 10 if you want to send every 10th log | 1 | :heavy_check_mark: | 

### Lambda configuration (Optional)
| Parameter | Description | Default Value | Required |
|---|---|---|---|
| FunctionMemorySize | Memory size for lambda function in mb | 1024 | :heavy_check_mark: | 
| FunctionTimeout | Timeout for the lambda function in sec | 300 | :heavy_check_mark: | 
| LogLevel | Log level for the Lambda function. Can be one of: INFO, WARN, ERROR, DEBUG | WARN | :heavy_check_mark: | 
| LambdaLogRetention | CloudWatch log retention days for logs generated by the Lambda function | 5 | :heavy_check_mark: | 

### VPC configuration (Optional)
| Parameter | Description | Default Value | Required |
|---|---|---|---|
| LambdaSubnetID | ID of Subnet into which to deploy the integration |   | :heavy_check_mark: | 
| LambdaSecurityGroupID | ID of the SecurityGroup into which to deploy the integration |   | :heavy_check_mark: | 
| UsePrivateLink | Will you be using our PrivateLink? | false | :heavy_check_mark: | 

## Advanced

### AWS PrivateLink
To use privatelink please forllow the instruction in this [link](https://coralogix.com/docs/coralogix-amazon-web-services-aws-privatelink-endpoints/)

### Dynamic Application and Subsystem Name
- Json support: 
-- for dynamically value from the log you should use *$.my_log.field*
-- For cloudtrail use *$.eventSource* to use the trail source
- s3 folder: Use the following tag: {{s3_key.*value*}} where value is the folder level, for example:
    if the file path that triggers the event is AWSLogs/112322232/ELB1/elb.log or AWSLogs/112322232/ELB2/elb.log and you
    want ELB1 and ELB2 to be the subsystem, you subsystemName shoudl be {{s3_key.3}}

## Troubleshooting

- Look out for TimeOut Errors "Task timed out after", if you see them, please increase lambda timeout from Configuration -> General Configuration
- Look out for out of Memory logs "Task out of Memory" , if you see them, please increase lambda max memory from Configuration -> General Configuration
- To add more verbosity to Lambda logs, you can set RUST_LOG to DEBUG. Remamber to change it back to INFO once troubleshooting is done.
- set MAX_ELAPSED_TIME variable for default change ( default = 250 )
- Set BATCHES_MAX_SIZE (in MB) sets batch max size before sending to coralogix. This value is limited by the max payload accepted by Coralogix Endpoing. (default = 4)
- Set BATCHES_MAX_CONCURRENCY sets max amount of concurrect batches can be sent. 
