# Forward AWS Logs via Lambda Shipper

[![license](https://img.shields.io/github/license/coralogix/coralogix-aws-shipper.svg)](https://raw.githubusercontent.com/coralogix/coralogix-aws-shipper/master/LICENSE) ![publish workflow](https://github.com/coralogix/coralogix-aws-shipper/actions/workflows/publish.yaml/badge.svg) ![Dynamic TOML Badge](https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fcoralogix%2Fcoralogix-aws-shipper%2Fmaster%2FCargo.toml&query=%24.package.version&label=version) [![Rust Report Card](https://rust-reportcard.xuri.me/badge/github.com/coralogix/coralogix-aws-shipper)](https://rust-reportcard.xuri.me/report/github.com/coralogix/coralogix-aws-shipper) ![Static Badge](https://img.shields.io/badge/status-GA-brightgreen)

### Contents

1. [Overview](#overview)
2. [Supported Services](#supported-services)
   - [Amazon S3, CloudTrail, VPC Flow Logs and more](#amazon-s3-cloudtrail-vpc-flow-logs-and-more)
   - [Amazon SNS/SQS](#amazon-snssqs)
   - [Amazon CloudWatch](#amazon-cloudwatch)
   - [Amazon Kinesis](#amazon-kinesis)
   - [Amazon MSK & Kafka](#amazon-msk--kafka)
   - [Amazon ECR Image Security Scan](#amazon-ecr-image-security-scan)
3. [Deployment Options](#deployment-options)
   - [Integrate using the Coralogix Platform (Recommended)](#integrate-using-the-coralogix-platform-recommended)
   - [Quick Create a CloudFormation Stack](#quick-create-a-cloudformation-stack)
   - [Deploy the AWS Serverless Application](#deploy-the-aws-serverless-application)
   - [Terraform Module](#terraform-module)
4. [Configuration Parameters](#configuration-parameters)
   - [Universal Configuration](#universal-configuration)
   - [S3/CloudTrail/VpcFlow/S3Csv Configuration](#s3cloudtrailvpcflows3csv-configuration)
   - [CloudWatch Configuration](#cloudwatch-configuration)
   - [SNS Configuration](#sns-configuration)
   - [SQS Configuration](#sqs-configuration)
   - [Kinesis Configuration](#kinesis-configuration)
   - [Kafka Configuration](#kafka-configuration)
   - [MSK Configuration](#msk-configuration)
   - [Generic Configuration (Optional)](#generic-configuration-optional)
   - [Lambda Configuration (Optional)](#lambda-configuration-optional)
   - [VPC Configuration (Optional)](#vpc-configuration-optional)
   - [Metadata](#metadata)
   - [Advanced Configuration](#advanced-configuration)
   - [DLQ](#dlq)
5. [Troubleshooting](#troubleshooting)
6. [Cloudwatch Metrics Stream via Firehose for PrivateLink (beta)](#cloudwatch-metrics-stream-via-firehose-privatelink-beta)
7. [Support](#support)

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

Coralogix can be configured to receive ECR [Image Scanning](https://docs.aws.amazon.com/AmazonECR/latest/userguide/image-scanning.html).

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

| Parameter                    | Description                                                                                                                                                                                                                                                                                                                        | Default Value | Required           |
|------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------|--------------------|
| Application name             | This will also be the name of the CloudFormation stack that creates your integration. It can include letters (A–Z and a–z), numbers (0–9) and dashes (-).                                                                                                                                                                          |               | :heavy_check_mark: |
| IntegrationType              | Choose the AWS service that you wish to integrate with Coralogix. Can be one of: S3, CloudTrail, VpcFlow, CloudWatch, S3Csv, SNS, SQS, CloudFront, Kinesis, Kafka, MSK, EcrScan.                                                                                                                                                   | S3            | :heavy_check_mark: |
| CoralogixRegion              | Your data source should be in the same region as the integration stack. You may choose from one of [the default Coralogix regions](https://coralogix.com/docs/coralogix-domain/): [Custom, EU1, EU2, AP1, AP2, US1, US2]. If this value is set to Custom you must specify the Custom Domain to use via the CustomDomain parameter. | Custom        | :heavy_check_mark: |
| CustomDomain                 | If you choose a custom domain name for your private cluster, Coralogix will send telemetry from the specified address (e.g. custom.coralogix.com).                                                                                                                                                                                 |               |                    |
| ApplicationName              | The name of the application for which the integration is configured. [Advanced Configuration](#advanced-configuration) specifies dynamic value retrieval options.                                                                                                                                                                  |               | :heavy_check_mark: |
| SubsystemName                | Specify the [name of your subsystem](https://coralogix.com/docs/application-and-subsystem-names/). For a dynamic value, refer to the Advanced Configuration section. For CloudWatch, leave this field empty to use the log group name.                                                                                             |               | :heavy_check_mark: |
| ApiKey                       | The Send-Your-Data [API Key](https://coralogix.com/docs/send-your-data-api-key/) validates your authenticity. This value can be a direct Coralogix API Key or an AWS Secret Manager ARN containing the API Key.<br>*Note the parameter expects the API Key in plain text or if stored in secret manager.*                          |               | :heavy_check_mark: |
| StoreAPIKeyInSecretsManager  | Enable this to store your API Key securely. Otherwise, it will remain exposed in plain text as an environment variable in the Lambda function console.                                                                                                                                                                             | True          | :heavy_check_mark: |
| ReservedConcurrentExecutions | The number of concurrent executions that are reserved for the function, leave empty so the lambda will use unreserved account concurrency.                                                                                                                                                                                         | n/a           |                    |
| LambdaAssumeRoleARN          | A role that the lambda will assume, leave empty to use the default permissions.<br> Note that if this Parameter is used, all **S3** and **ECR** API calls from the lambda will be made with the permissions of the Assumed Role.                                                                                                   |               |                    |
| ExecutionRoleARN             | The arn of a user defined role that will be used as the execution role for the lambda function                                                                                                                                                                                                                                     |               |                    |

> **Note:** `EcrScan` doesn't need any extra configuration.

#### Working with Roles

In some cases special or more fine tuned IAM permissions are required. The AWS Shipper supports more granular IAM control using 2 parameters:

- **LambdaAssumeRoleARN**: This parameter allows you to specify a Role ARN, enabling the Lambda function to assume the role. The assumed role will only affect S3 and ECR API calls, as these are the only services invoked by the Lambda function at the code level.

- **ExecutionRoleARN**: This parameter lets you specify the Execution Role for the AWS Shipper Lambda. The provided role must have basic Lambda execution permissions, and any additional permissions required for the Lambda’s operation will be automatically added during deployment.

Basic lambda execution role permission:

```yaml
        Statement:
          - Effect: "Allow"
            Principal:
              Service: "lambda.amazonaws.com"
            Action: "sts:AssumeRole"
```

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

| Parameter                | Description                                                                                                                                                                                                                                                                                                                                           | Default Value | Required           |
|--------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------|--------------------|
| CloudWatchLogGroupName   | Provide a comma-separated list of CloudWatch log group names to monitor, for example, (`log-group1`, `log-group2`, `log-group3`).                                                                                                                                                                                                                     |               | :heavy_check_mark: |
| CloudWatchLogGroupPrefix | Prefix of the CloudWatch log groups that will trigger the lambda, in case that your log groups are `log-group1, log-group2, log-group3` then you can set the value to `log-group`. When using this variable you will not be able to see the log groups as trigger for the lambda. The parameter dose not replace **CloudWatchLogGroupName** parameter |               |                    |

In case your log group name is longer than 70, than in the lambda function you will see the permission for that log group as: `allow-trigger-from-<the log group first 65 characters and the last 5 characters>` this is because of length limit in AWS for permission name.

### SNS Configuration

To receive SNS messages directly to Coralogix, use the `SNSIntegrationTopicARN` parameter. This differs from the above use of `SNSTopicArn`, which notifies based on S3 events.

| Parameter              | Description                                                                              | Default Value | Required           |
|------------------------|------------------------------------------------------------------------------------------|---------------|--------------------|
| SNSIntegrationTopicArn | Provide the ARN of the SNS topic to which you want to subscribe for retrieving messages. |               | :heavy_check_mark: |

### SQS Configuration

To receive SQS messages directly to Coralogix, use the `SQSIntegrationTopicARN` parameter. This differs from the above use of `SQSTopicArn`, which notifies based on S3 events.

| Parameter              | Description                                                                              | Default Value | Required           |
|------------------------|------------------------------------------------------------------------------------------|---------------|--------------------|
| SQSIntegrationTopicArn | Provide the ARN of the SQS queue to which you want to subscribe for retrieving messages. |               | :heavy_check_mark: |

### Kinesis Configuration

We can receive direct [Kinesis](https://aws.amazon.com/kinesis/) stream data from your AWS account to Coralogix. Your Kinesis stream ARN is a required parameter in this case.

| Parameter        | Description                                                                                   | Default Value | Required           |
|------------------|-----------------------------------------------------------------------------------------------|---------------|--------------------|
| KinesisStreamArn | Provide the ARN of the Kinesis Stream to which you want to subscribe for retrieving messages. |               | :heavy_check_mark: |

### Kafka Configuration

| Parameter           | Description                                                                   | Default Value | Required           |
|---------------------|-------------------------------------------------------------------------------|---------------|--------------------|
| KafkaBrokers        | Comma-delimited list of Kafka brokers to establish a connection with.         |               | :heavy_check_mark: |
| KafkaTopic          | Subscribe to this Kafka topic for data consumption.                           |               | :heavy_check_mark: |
| KafkaBatchSize      | Specify the size of data batches to be read from Kafka during each retrieval. | 100           |                    |
| KafkaSecurityGroups | Comma-delimited list of Kafka security groups for secure connection setup.    |               | :heavy_check_mark: |
| KafkaSubnets        | Comma-delimited list of Kafka subnets to use when connecting to Kafka.        |               | :heavy_check_mark: |

### MSK Configuration

Your Lambda function must be in a VPC that has access to the MSK cluster. You can configure your VPC via the provided [VPC configuration parameters](#vpc-configuration-optional).

| Parameter  | Description                                           | Default Value | Required           |
|------------|-------------------------------------------------------|---------------|--------------------|
| MSKBrokers | Comma-delimited list of MSK brokers to connect to.    |               | :heavy_check_mark: |
| KafkaTopic | Comma separated list of Kafka topics to Subscribe to. |               | :heavy_check_mark: |

### Generic Configuration (Optional)

These are optional parameters if you wish to receive notification emails, exclude certain logs or send messages to Coralogix at a particular rate.

| Parameter         | Description                                                                                                                                                                                                | Default Value | Required           |
|-------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------|--------------------|
| NotificationEmail | A failure notification will be sent to this email address.                                                                                                                                                 |               |                    |
| BlockingPattern   | Enter a regular expression to identify lines excluded from being sent to Coralogix. For example, use `MainActivity.java:\d{3}` to match log lines with `MainActivity` followed by exactly three digits.    |               |                    |
| SamplingRate      | Send messages at a specific rate, such as 1 out of every N logs. For example, if your value is 10, a message will be sent for every 10th log.                                                              | 1             | :heavy_check_mark: |
| AddMetadata       | Add aws event metadata to the log message. Expects comma separated values. Options for S3 are `bucket_name`,`key_name`. For CloudWatch use `stream_name`, `loggroup_name` . For Kafka/MSK use `topic_name` |               |                    |
| CustomMetadata    | Add custom metadata to the log message. Expects comma separated values. Options are key1=value1,key2=value2                                                                                                |               |                    |

### Lambda Configuration (Optional)

These are the default presets for Lambda. Read [Troubleshooting](#troubleshooting) for more information on changing these defaults.

| Parameter             | Description                                                                                                   | Default Value   | Required           |
|-----------------------|---------------------------------------------------------------------------------------------------------------|-----------------|--------------------|
| FunctionMemorySize    | Specify the memory size for the Lambda function in megabytes.                                                 | 1024            | :heavy_check_mark: |
| FunctionTimeout       | Set a timeout for the Lambda function in seconds.                                                             | 300             | :heavy_check_mark: |
| LogLevel              | Specify the log level for the Lambda function, choosing from the following options: INFO, WARN, ERROR, DEBUG. | WARN            | :heavy_check_mark: |
| LambdaLogRetention    | Set the CloudWatch log retention period (in days) for logs generated by the Lambda function.                  | 5               | :heavy_check_mark: |
| FunctionRunTime       | Type of runtime for the lambda, allowd values are provided.al2023 or provided.al2.                            | provided.al2023 | :heavy_check_mark: |
| FunctionArchitectures | Architectures for the lambda function, allowed values are arm64 or x86_64.                                    | arm64           | :heavy_check_mark: |

### VPC Configuration (Optional)

Use the following options if you need to configure a private link with Coralogix.

| Parameter             | Description                                                                    | Default Value | Required           |
|-----------------------|--------------------------------------------------------------------------------|---------------|--------------------|
| LambdaSubnetID        | Specify the ID of the subnet where the integration should be deployed.         |               | :heavy_check_mark: |
| LambdaSecurityGroupID | Specify the ID of the Security Group where the integration should be deployed. |               | :heavy_check_mark: |
| UsePrivateLink        | Set this to true if you will be using AWS PrivateLink.                         | false         | :heavy_check_mark: |

### Metadata

The metadata features decribed below are only available in `coralogix-aws-shipper v1.1.0` and later.

The `AddMetadata` parameter allows you to add metadata to the log message. The metadata is added to the log message as a JSON object. The metadata is specific to the integration type. For example, for S3, the metadata is `s3.object.key` and `s3.bucket`. For CloudWatch, the metadata is `cw.log.group` and `cw.log.stream`. See table below for full list of metadata.

| Integration Type | Metadata Key             | Description                           |
|------------------|--------------------------|---------------------------------------|
| S3               | s3.bucket                | The name of the S3 bucket             |
| S3               | s3.object.key            | The key/path of the S3 object         |
| CloudWatch       | cw.log.group             | The name of the CloudWatch log group  |
| CloudWatch       | cw.log.stream            | The name of the CloudWatch log stream |
| Cloudwatch       | cw.owner                 | The owner of the log group            |
| Kafka            | kafka.topic              | The name of the Kafka topic           |
| MSK              | kafka.topic              | The name of the Kafka topic           |
| Kinesis          | kinesis.event.id         | The kinesis event ID                  |
| Kinesis          | kinesis.event.name       | The kinesis event name                |
| kinesis          | kinesis.event.source     | The kinesis event source              |
| kinesis          | kinesis.event.source_arn | The kinesis event source ARN          |
| Sqs              | sqs.event.source         | The sqs event source/queue            |
| Sqs              | sqs.event.id             | The sqs event id                      |
| Ecr              | ecr.scan.id              | The ecr scan id                       |
| Ecr              | ecr.scan.source          | The ecr scan source                   |

Note that metadata is not added by default. You must specify the metadata keys you want in the `AddMetadata` parameter.

For example, if you want to add the bucket name and key name to the log message, you would set the `AddMetadata` parameter to `s3.object.key,s3.bucket`.

Some metadata keys will overlap as some integrations share the same metadata. For example, both Kafka and MSK have the same metadata key `kafka.topic` or both Kinesis and Cloudwatch metadata will be added in cases where a Cloudwatch log stream is being ingested from a Kinesis stream.

##### Dynamic Subsystem or Application Name

As of `v1.1.0`,you can use dynamic values for the Application and Subsystem Name parameters based on the internal metadata defined above.

To do accomplish this, you can use the following syntax:

```
{{ metadata.key | r'regex' }}
```

For example, if you want to use the bucket name as the subsystem name, you would set the `SubsystemName` parameter to:

```
{{ s3.bucket }}
```

If you want to use the log group name as the application name, you would set the `ApplicationName` parameter to:

```
{{ cw.log.group }}
```

If you only want to use part of the metadata value, you can use a regular expression to extract the desired part. For example, If we have an `s3.object.key` value of `AWSLogs/112322232/ELB1/elb.log` and we want to extract the last part of the key as the Subsystem name, we would set the `SubsystemName` parameter to:

```
{{ s3.object.key | r'AWSLogs\/.+\/(.*)$' }}
```

This would result in a SubsystemName value of `elb.log` as this is the part of the regex that is captured by the group `(.*)`.

**Important**:

- The regex must be a valid regex pattern.
- The regex must define a capture group for part of the string you want to use as the value
- The metadata key must exist in the list defined above and be a part of the integration type that is deployed.

Dynamic values are only supported for the `ApplicationName` and `SubsystemName` parameters, the `CustomMetadata` parameter is not supported.

### Advanced Configuration

**AWS PrivateLink**

If you want to bypass using the public internet, you can use AWS PrivateLink to facilitate secure connections between your VPCs and AWS Services. This option is available under the [VPC Configuration](#vpc-configuration-optional) tab. To turn it on, either check off the Use Private Link box in the Coralogix UI or set the parameter to `true`. For additional instructions on AWS PrivateLink, please [follow our dedicated tutorial](https://coralogix.com/docs/coralogix-amazon-web-services-aws-privatelink-endpoints/).

**Dynamic Values**

> Note the following method for using dynamic values will change to the method defined above in `coralogix-aws-shipper v1.1.0` and later. This approach will no longer be supported.

If you wish to use dynamic values for the Application and Subsystem Name parameters, consider the following:

**JSON support:** To reference dynamic values from the log, use `$.my_log.field`. For the CloudTrail source, use `$.eventSource`.

**S3 folder:** Use the following tag: `{{s3_key.value}}` where the value is the folder level. For example, if the file path that triggers the event is `AWSLogs/112322232/ELB1/elb.log` or `AWSLogs/112322232/ELB2/elb.log` and you want ELB1 and ELB2 to be the subsystem, your `subsystemName` should be `{{s3_key.3}}`

**S3Csv Custom Headers:** Add Environment Variable "CUSTOM_CSV_HEADER" with the key names. This must be with the same delimiter as the CSV archive, for example if the csv file delimiter is ";", then your environment varialble should be like this: CUSTOM_CSV_HEADER = name;country;age

### DLQ

A Dead Letter Queue (DLQ) is a queue where messages are sent if they cannot be processed by the Lambda function. This is useful for debugging and monitoring.

The DLQ workflow for the Coralogix AWS Shipper is as follows:

![DLQ Workflow](./static/dlq-workflow.png)

To enable the DLQ, you must provide the required parameters outlined below.

| Parameter     | Description                                                                   | Default Value | Required           |
|---------------|-------------------------------------------------------------------------------|---------------|--------------------|
| EnableDLQ     | Enable the Dead Letter Queue for the Lambda function.                         | false         | :heavy_check_mark: |
| DLQS3Bucket   | An S3 bucket used to store all failure events that have exhausted retries.    |               | :heavy_check_mark: |
| DLQRetryLimit | The number of times a failed event should be retried before being saved in S3 | 3             | :heavy_check_mark: |
| DLQRetryDelay | The delay in seconds between retries of failed events                         | 900           | :heavy_check_mark: |

## Troubleshooting

**Parameter max value** If you tried to deploy the integration and got this error `length is greater than 4094`, then you can upload the value of the parameter to an S3 bucket as txt and pass the file URL as the parameter value ( this option is available for `KafkaTopic` and `CloudWatchLogGroupName` parameters).

**Timeout errors**

If you see “Task timed out after”, you need to increase the Lambda Timeout value. You can do this from the AWS Lambda function settings under **Configuration** > **General Configuration**.

**Not enough memory**

If you see “Task out of memory”, you should increase the Lambda maximum Memory value. In the AWS Lambda function settings, go to Configuration > General Configuration.

**Verbose logs**

To add more verbosity to your function logs, set RUST_LOG to DEBUG.

**Trigger Failed on Deployment** If Deployment is failing while asigning the trigger, please check that S3 Bucket notifications has no notifications enabled. If Using Cloudwatch max number of notificactions per LogGroup is 2.

> **Warning:** Remember to change it back to WARN after troubleshooting.

**Changing defaults**

Set the MAX_ELAPSED_TIME variable for default change (default = 250). Set BATCHES_MAX_SIZE (in MB) sets batch max size before sending to Coralogix. This value is limited by the max payload accepted by the Coralogix endpoint (default = 4). Set BATCHES_MAX_CONCURRENCY sets the maximum amount of concurrent batches that can be sent.

# Cloudwatch Metrics Stream via Firehose PrivateLink (beta)

As of version `v1.3.0`, the Coralogix AWS Shipper supports streaming **Cloudwatch Metrics to Coralogix via Firehose over a PrivateLink**.

This workflow is designed for scenarios where you need to stream metrics from a CloudWatch Metrics stream to Coralogix via a PrivateLink endpoint.

#### Why Use This Workflow?

AWS Firehose does not support PrivateLink endpoints as a destination because Firehose cannot be connected to a VPC, which is required to reach a PrivateLink endpoint. To overcome this limitation, the Coralogix AWS Shipper acts as a transform function. It is attached to a Firehose instance that receives metrics from the CloudWatch Metrics stream and forwards them to Coralogix over a PrivateLink.

#### When to Use This Workflow

This workflow is specifically for bypassing the limitation of using Firehose with the Coralogix PrivateLink endpoint. If there is no requirement for PrivateLink, we recommend using the default Firehose Integration for CloudWatch Stream Metrics found [here](https://coralogix.com/docs/integrations/aws/amazon-data-firehose/aws-cloudwatch-metric-streams-with-amazon-data-firehose/).

#### How does it work?

![Cloudwatch stream via PrivateLink Workflow](./static/cloudwatch-metrics-pl-workflow.png)

To enable the Cloudwatch Metrics Stream via Firehose (PrivateLink) you must provide the required parameters outlined below.

| Parameter                   | Description                                                                                                                                                                                                                                                                                                                        | Default Value | Required           |
|-----------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|---------------|--------------------|
| TelemetryMode               | Specify the telemetry collection modes, supported values (`metrics`, `logs`). Note that this value must be set to `metrics` for the Cloudwatch metric stream workflow                                                                                                                                                              | logs          | :heavy_check_mark: |
| ApiKey                      | The Send-Your-Data [API Key](https://coralogix.com/docs/send-your-data-api-key/) validates your authenticity. This value can be a direct Coralogix API Key or an AWS Secret Manager ARN containing the API Key.<br>*Note the parameter expects the API Key in plain text or if stored in secret manager.*                          |               | :heavy_check_mark: |
| ApplicationName             | The name of the application for which the integration is configured. [Advanced Configuration](#advanced-configuration) specifies dynamic value retrieval options.                                                                                                                                                                  |               | :heavy_check_mark: |
| SubsystemName               | Specify the [name of your subsystem](https://coralogix.com/docs/application-and-subsystem-names/). For a dynamic value, refer to the Advanced Configuration section. For CloudWatch, leave this field empty to use the log group name.                                                                                             |               | :heavy_check_mark: |
| CoralogixRegion             | Your data source should be in the same region as the integration stack. You may choose from one of [the default Coralogix regions](https://coralogix.com/docs/coralogix-domain/): [Custom, EU1, EU2, AP1, AP2, US1, US2]. If this value is set to Custom you must specify the Custom Domain to use via the CustomDomain parameter. | Custom        | :heavy_check_mark: |
| S3BucketName                | The S3Bucket that will be used to store records that have failed processing                                                                                                                                                                                                                                                        |               | :heavy_check_mark: |
| LambdaSubnetID              | Specify the ID of the subnet where the integration should be deployed.                                                                                                                                                                                                                                                             |               | :heavy_check_mark: |
| LambdaSecurityGroupID       | Specify the ID of the Security Group where the integration should be deployed.                                                                                                                                                                                                                                                     |               | :heavy_check_mark: |
| StoreAPIKeyInSecretsManager | Enable this to store your API Key securely. Otherwise, it will remain exposed in plain text as an environment variable in the Lambda function console.                                                                                                                                                                             | True          |                    |

## Support

**Need help?**

Our world-class customer success team is available 24/7 to walk you through your setup and answer any questions that may come up.

Contact us **via our in-app chat** or by emailing [support@coralogix.com](mailto:support@coralogix.com).
