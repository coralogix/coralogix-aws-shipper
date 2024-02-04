# Forward AWS Logs via Lambda Shipper

[![license](https://img.shields.io/github/license/coralogix/coralogix-aws-shipper.svg)](https://raw.githubusercontent.com/coralogix/coralogix-aws-shipper/master/LICENSE) ![publish workflow](https://github.com/coralogix/coralogix-aws-shipper/actions/workflows/publish.yaml/badge.svg) ![Dynamic TOML Badge](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fcoralogix%2Fcoralogix-aws-shipper%2Fmaster%2FCargo.toml&query=%24.package.version&label=version) [![Rust Report Card](https://rust-reportcard.xuri.me/badge/github.com/coralogix/coralogix-aws-shipper)](https://rust-reportcard.xuri.me/report/github.com/coralogix/coralogix-aws-shipper) ![Static Badge](https://img.shields.io/badge/status-GA-brightgreen)

## Overview

This integration guide focuses on connecting your AWS environment to Coralogix using AWS Lambda functions. To complete this integration, you may either use the Coralogix platform UI, CloudFormation templates from AWS, AWS SAM applications, or a dedicated Terraform module from our [GitHub repository](https://github.com/coralogix/terraform-coralogix-aws/tree/master/modules/coralogix-aws-shipper).

We will show you how to complete our predefined Lambda function template to simplify the integration. Your task will be to provide specific configuration parameters, based on the service that you wish to connect. The reference list for these parameters is provided below.

> **Note:** As we improve `coralogix-aws-shipper`, we invite you to contribute, ask questions, and report issues in the repository.

## Supported Services

Although `coralogix-aws-shipper` handles all listed AWS product integrations, some of the parameters are product-specific. Consult the [Configuration Parameters](#configuration-parameters) for product-specific requirements.

### Amazon S3, CloudTrail, VPC Flow Logs and more

This integration is based on S3. Your Amazon S3 bucket can receive log files from all kinds of services, such as [CloudTrail](https://docs.aws.amazon.com/awscloudtrail/latest/userguide/cloudtrail-log-file-examples.html), [VPC Flow Logs](https://docs.aws.amazon.com/vpc/latest/userguide/flow-logs-s3.html), [Redshift](https://docs.aws.amazon.com/redshift/latest/mgmt/db-auditing.html#db-auditing-manage-log-files), [Network Firewall](https://docs.aws.amazon.com/network-firewall/latest/developerguide/logging-s3.html) or different types of load balancers ([ALB](https://docs.aws.amazon.com/elasticloadbalancing/latest/application/load-balancer-access-logs.html)/[NLB](https://docs.aws.amazon.com/elasticloadbalancing/latest/network/load-balancer-access-logs.html)/[ELB](https://docs.aws.amazon.com/elasticloadbalancing/latest/classic/access-log-collection.html)). This data is then sent to Coralogix for analysis.

You may also include SNS/SQS in the pipeline so that the integration triggers upon notification.

### Amazon SNS/SQS

A separate integration for SNS or SQS is available. You can receive messages directly from both services to your Coralogix subscription. You will need the ARN of the individual SNS/SQS topic.

### Amazon CloudWatch

Coralogix can be configured to receive data directly from your [CloudWatch](https://docs.aws.amazon.com/cloudwatch/) log group.

### Amazon Kinesis

Coralogix can be configured to receive data directly from your [Kinesis Stream](https://docs.aws.amazon.com/cloudwatch/).

### Amazon MSK & Kafka

Coralogix can be configured to receive data directly from your [MSK](https://docs.aws.amazon.com/msk/) or [Kafka](https://docs.aws.amazon.com/lambda/latest/dg/with-kafka.html) cluster.

### Amazon ECR Image Security Scan

Coralogix can be configured to recieve ECR [Image Scanning](https://docs.aws.amazon.com/AmazonECR/latest/userguide/image-scanning.html)

## Deployment Options

> **Important:** Before you get started, ensure that your AWS user has the permissions to create Lambda functions and IAM roles.

### Integrate using the Coralogix Platform (Recommended)

The fastest way to deploy your predefined Lambda function is from within the Coralogix platform. Fill out an integration form and confirm the integration from your AWS account. Product integrations can be by navigating to **Data Flow** > **Integrations** in your Coralogix toolbar. For detailed UI instructions, please read the [Integration Packages](https://coralogix.com/docs/integration-packages/) tutorial.

### Quick Create a CloudFormation Stack

You can always launch the CloudFormation stack by filling out a Quick Create template. This is done from within your AWS Management Console. Log into your AWS account and click the button below to deploy the CloudFormation stack.

[![Launch Stack](https://s3.amazonaws.com/cloudformation-examples/cloudformation-launch-stack.png)](https://console.aws.amazon.com/cloudformation/home#/stacks/create/review?stackName=coralogix-aws-shipper&templateURL=https://cgx-cloudformation-templates.s3.amazonaws.com/aws-integrations/aws-shipper-lambda/template.yaml)

If you are using AWS CLI, you can use a [CloudFormation template](https://github.com/coralogix/cloudformation-coralogix-aws/tree/master/aws-integrations/aws-shipper-lambda) from our repository.

### Deploy the AWS Serverless Application

Alternatively, you may use the [SAM deployment link](https://serverlessrepo.aws.amazon.com/applications/eu-central-1/597078901540/coralogix-aws-shipper). The procedure is very similar to filling out the Quick Create template.

### Terraform Module

If you are using Terraform to launch your infrastructure, you can access coralogix-aws-shipper it via our [Terraform Module](https://github.com/coralogix/terraform-coralogix-aws/tree/master/modules/coralogix-aws-shipper). Use the parameters defined in the repository README, as they better reflect the configuration process.

## Configuration Parameters

This document explains the basic config options for your template. You will need these values to launch your integration. For additional optional parameters, view our [Advanced Configuration](#advanced-configuration) options.

Use the tables below as a guide to properly configure your deployment. The provided configuration variables are for the Serverless or CloudFormation deployment options. The variable requirements are slightly different if you wish to deploy with Terraform. Please refer to the [Terraform Module](https://github.com/coralogix/terraform-coralogix-aws/tree/master/modules/coralogix-aws-shipper) for further details.

### Universal Configuration

Use an existing Coralogix [Send-Your-Data API key](https://coralogix.com/docs/send-your-data-management-api/) to make the connection or create one as you fill our pre-made template. Additionally, make sure your integration is [Region-specific](https://coralogix.com/docs/coralogix-domain/).

> **Note:** You should always deploy the AWS Lambda function in the same AWS Region as your resource (e.g. the S3 bucket).

| Parameter                   | Description                                                                                                                                                                                                                                                                                                                        | Default Value | Required           |
|-----------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------|--------------------|
| Application name            | This will also be the name of the CloudFormation stack that creates your integration. It can include letters (A–Z and a–z), numbers (0–9) and dashes (-).                                                                                                                                                                          |               | :heavy_check_mark: |
| IntegrationType             | Choose the AWS service that you wish to integrate with Coralogix. Can be one of: S3, CloudTrail, VpcFlow, CloudWatch, S3Csv, SNS, SQS, CloudFront, Kinesis, Kafka, MSK, EcrScan.                    | S3            | :heavy_check_mark: |
| CoralogixRegion             | Your data source should be in the same region as the integration stack. You may choose from one of [the default Coralogix regions](https://coralogix.com/docs/coralogix-domain/): [Custom, EU1, EU2, AP1, AP2, US1, US2]. If this value is set to Custom you must specify the Custom Domain to use via the CustomDomain parameter. | Custom        | :heavy_check_mark: |
| CustomDomain                | If you choose a custom domain name for your private cluster, Coralogix will send telemetry from the specified address (e.g. custom.coralogix.com).                                                                                                                                                                                 |               |                    |
| ApplicationName             | The name of the application for which the integration is configured. [Advanced Configuration](#advanced-configuration) specifies dynamic value retrieval options.                                                                                                                                                                  |               | :heavy_check_mark: |
| SubsystemName               | Specify the [name of your subsystem](https://coralogix.com/docs/application-and-subsystem-names/). For a dynamic value, refer to the Advanced Configuration section. For CloudWatch, leave this field empty to use the log group name.                                                                                             |               | :heavy_check_mark: |
| ApiKey                      | The Send-Your-Data [API Key](https://coralogix.com/docs/send-your-data-api-key/) validates your authenticity. This value can be a direct Coralogix API Key or an AWS Secret Manager ARN containing the API Key.                                                                                                                    |               | :heavy_check_mark: |
| StoreAPIKeyInSecretsManager | Enable this to store your API Key securely. Otherwise, it will remain exposed in plain text as an environment variable in the Lambda function console.                                                                                                                                                                             | True          | :heavy_check_mark: |

> **Note:** EcrScan doesn't need any extra configuration.

### S3/CloudTrail/VpcFlow/S3Csv Configuration

This is the most flexible type of integration, as it is based on receiving log files to Amazon S3. First, your bucket can receive log files from all kinds of other services, such as CloudTrail, VPC Flow Logs, Redshift, Network Firewall or different types of load balancers (ALB/NLB/ELB). Once the data is in the bucket, a pre-made Lambda function will then transmit it to your Coralogix account.

> **Tip:** The S3 integration supports generic data. You can ingest any generic text, JSON, and CSV data stored in your S3 bucket.

**Maintain S3 notifications via SNS or SQS:**

If you don’t want to send data directly as it enters S3, you can also use SNS/SQS to maintain notifications before any data is sent from your bucket to Coralogix. For this, you need to set the `SNSTopicArn` or `SQSTopicArn` parameters.

> **Note:** All resources, such as S3 or SNS/SQS, should be provisioned already. If you are using an S3 bucket as a resource, please make sure it is clear of any Lambda triggers located in the same AWS region as your new function.

| Parameter      | Description                                                                                                                                                                                       | Default Value                            | Required           |
|----------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------------------------------------------|--------------------|
| S3BucketName   | Specify the name of the AWS S3 bucket that you want to monitor.                                                                                                                                   |                                          | :heavy_check_mark: |
| S3KeyPrefix    | Specify the prefix of the log path within your S3 bucket. This value is ignored if you use the SNSTopicArn/SQSTopicArn parameter.                                                                 | CloudTrail/VpcFlow 'AWSLogs/'            |                    |
| S3KeySuffix    | Filter for the suffix of the file path in your S3 bucket. This value is ignored if you use the SNSTopicArn/SQSTopicArn parameter.                                                                 | CloudTrail '.json.gz', VpcFlow '.log.gz' |                    |
| NewlinePattern | Enter a regular expression to detect a new log line for multiline logs, e.g., \n(?=\d{2}-\d{2}\s\d{2}:\d{2}:\d{2}.\d{3}).                                                                         |                                          |                    |
| SNSTopicArn    | The ARN for the SNS topic that contains the SNS subscription responsible for retrieving logs from Amazon S3.                                                                                      |                                          |                    |
| SQSTopicArn    | The ARN for the SQS queue that contains the SQS subscription responsible for retrieving logs from Amazon S3.                                                                                      |                                          |                    |
| CSVDelimiter   | Specify a single character to be used as a delimiter when ingesting a CSV file with a header line. This value is applicable when the S3Csv integration type is selected, for example, “,” or ” “. | ,                                        |                    |

### CloudWatch Configuration

Coralogix can be configured to receive data directly from your CloudWatch log group. CloudWatch logs are streamed directly to Coralogix via Lambda. This option does not use S3. You must provide the log group name as a parameter during setup.

| Parameter              | Description                                                                                                                       | Default Value | Required           |
|------------------------|-----------------------------------------------------------------------------------------------------------------------------------|---------------|--------------------|
| CloudWatchLogGroupName | Provide a comma-separated list of CloudWatch log group names to monitor, for example, (`log-group1`, `log-group2`, `log-group3`). |               | :heavy_check_mark: |

### SNS Configuration

To receive SNS messages directly to Coralogix, use the `SNSIntegrationTopicARN` parameter. This differs from the above use of `SNSTopicArn`, which notifies based on S3 events.

| Parameter              | Description                                                                              | Default Value | Required           |
|------------------------|------------------------------------------------------------------------------------------|---------------|--------------------|
| SNSIntegrationTopicARN | Provide the ARN of the SNS topic to which you want to subscribe for retrieving messages. |               | :heavy_check_mark: |

### SQS Configuration

To receive SQS messages directly to Coralogix, use the `SQSIntegrationTopicARN` parameter. This differs from the above use of `SQSTopicArn`, which notifies based on S3 events.

| Parameter              | Description                                                                              | Default Value | Required           |
|------------------------|------------------------------------------------------------------------------------------|---------------|--------------------|
| SQSIntegrationTopicARN | Provide the ARN of the SQS queue to which you want to subscribe for retrieving messages. |               | :heavy_check_mark: |

### Kinesis Configuration

We can receive direct [Kinesis](https://aws.amazon.com/kinesis/) stream data from your AWS account to Coralogix. Your Kinesis stream ARN is a required parameter in this case.

| Parameter        | Description                                                                                   | Default Value | Required           |
|------------------|-----------------------------------------------------------------------------------------------|---------------|--------------------|
| KinesisStreamARN | Provide the ARN of the Kinesis Stream to which you want to subscribe for retrieving messages. |               | :heavy_check_mark: |

### MSK & Kafka Configuration

*Kafka*:

| Parameter           | Description                                                                   | Default Value | Required           |
|---------------------|-------------------------------------------------------------------------------|---------------|--------------------|
| KafkaBrokers        | Comma Delimited List of Kafka broker to connect to                            |               | :heavy_check_mark: |
| KafkaTopic          | The Kafka topic to subscribe to                                               |               | :heavy_check_mark: |
| KafkaBatchSize      | The Kafka batch size to use when reading from Kafka                           | 100           |                    |
| KafkaSecurityGroups | Comma Delimited List of Kafka security groups to use when connecting to Kafka |               | :heavy_check_mark: |
| KafkaSubnets        | Comma Delimited List of Kafka subnets to use when connecting to Kafka         |               | :heavy_check_mark: |

*MSK*:

When using the AWS MSK Integration, your Lambda must be in a VPC with access to the MSK cluster. You can do this by setting the relevant parameters for [vpc configuration](#vpc-configuration-optional).

| Parameter  | Description                                      | Default Value | Required           |
|------------|--------------------------------------------------|---------------|--------------------|
| MSKBrokers | Comma Delimited List of MSK broker to connect to |               | :heavy_check_mark: |
| KafkaTopic | The Kafka topic to subscribe to                  |               | :heavy_check_mark: |

### Generic Configuration (Optional)

These are optional parameters if you wish to receive notification emails, exclude certain logs or send messages to Coralogix at a particular rate.

| Parameter         | Description                                                                                                                                                                                             | Default Value | Required           |
|-------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------|--------------------|
| NotificationEmail | A failure notification will be sent to this email address.                                                                                                                                              |               |                    |
| BlockingPattern   | Enter a regular expression to identify lines excluded from being sent to Coralogix. For example, use `MainActivity.java:\d{3}` to match log lines with `MainActivity` followed by exactly three digits. |               |                    |
| SamplingRate      | Send messages at a specific rate, such as 1 out of every N logs. For example, if your value is 10, a message will be sent for every 10th log.                                                           | 1             | :heavy_check_mark: |
| AddMetadata       | Add metadata to the log message. Expects comma separated values. Options for S3 are `bucket_name`,`key_name`. For CloudWatch use `stream_name`.                                                         |               |                    |

### Lambda Configuration (Optional)

These are the default presets for Lambda. Read [Troubleshooting](#troubleshooting) for more information on changing these defaults.

| Parameter          | Description                                                                                                   | Default Value | Required           |
|--------------------|---------------------------------------------------------------------------------------------------------------|---------------|--------------------|
| FunctionMemorySize | Specify the memory size for the Lambda function in megabytes.                                                 | 1024          | :heavy_check_mark: |
| FunctionTimeout    | Set a timeout for the Lambda function in seconds.                                                             | 300           | :heavy_check_mark: |
| LogLevel           | Specify the log level for the Lambda function, choosing from the following options: INFO, WARN, ERROR, DEBUG. | WARN          | :heavy_check_mark: |
| LambdaLogRetention | Set the CloudWatch log retention period (in days) for logs generated by the Lambda function.                  | 5             | :heavy_check_mark: |

### VPC Configuration (Optional)

Use the following options if you need to configure a private link with Coralogix.

| Parameter             | Description                                                                    | Default Value | Required           |
|-----------------------|--------------------------------------------------------------------------------|---------------|--------------------|
| LambdaSubnetID        | Specify the ID of the subnet where the integration should be deployed.         |               | :heavy_check_mark: |
| LambdaSecurityGroupID | Specify the ID of the Security Group where the integration should be deployed. |               | :heavy_check_mark: |
| UsePrivateLink        | Set this to true if you will be using AWS PrivateLink.                         | false         | :heavy_check_mark: |

### Advanced Configuration

**AWS PrivateLink**

If you want to bypass using the public internet, you can use AWS PrivateLink to facilitate secure connections between your VPCs and AWS Services. This option is available under the [VPC Configuration](#vpc-configuration-optional) tab. To turn it on, either check off the Use Private Link box in the Coralogix UI or set the parameter to `true`. For additional instructions on AWS PrivateLink, please [follow our dedicated tutorial](https://coralogix.com/docs/coralogix-amazon-web-services-aws-privatelink-endpoints/).

**Dynamic Values**

If you wish to use dynamic values for the Application and Subsystem Name parameters, consider the following:

**JSON support:** To reference dynamic values from the log, use `$.my_log.field`. For the CloudTrail source, use `$.eventSource`.

**S3 folder:** Use the following tag: `{{s3_key.value}}` where the value is the folder level. For example, if the file path that triggers the event is `AWSLogs/112322232/ELB1/elb.log` or `AWSLogs/112322232/ELB2/elb.log` and you want ELB1 and ELB2 to be the subsystem, your `subsystemName` should be `{{s3_key.3}}`

## Troubleshooting

**Timeout errors**

If you see “Task timed out after”, you need to increase the Lambda Timeout value. You can do this from the AWS Lambda function settings under **Configuration** > **General Configuration**.

**Not enough memory**

If you see “Task out of memory”, you should increase the Lambda maximum Memory value. In the AWS Lambda function settings, go to Configuration > General Configuration.

**Verbose logs**

To add more verbosity to your function logs, set RUST_LOG to DEBUG.

> **Warning:** Remember to change it back to WARN after troubleshooting.

**Changing defaults**

Set the MAX_ELAPSED_TIME variable for default change (default = 250). Set BATCHES_MAX_SIZE (in MB) sets batch max size before sending to Coralogix. This value is limited by the max payload accepted by the Coralogix endpoint (default = 4). Set BATCHES_MAX_CONCURRENCY sets the maximum amount of concurrent batches that can be sent.

## Support

**Need help?**

Our world-class customer success team is available 24/7 to walk you through your setup and answer any questions that may come up.

Contact us **via our in-app chat** or by emailing [support@coralogix.com](mailto:support@coralogix.com).
