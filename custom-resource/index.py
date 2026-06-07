import json
import boto3
import os
import re
import time
import copy
from urllib import request, error
import functools
from types import SimpleNamespace
import traceback

# Parameter keys whose values are sensitive and must never reach CloudWatch Logs.
# When StoreAPIKeyInSecretsManager=false the template passes the raw Coralogix
# API key through ResourceProperties.Parameters, so it must be redacted before
# the event is logged.
_SENSITIVE_PARAM_KEYS = {"apikey", "coralogixapikey"}


def redact_event(event):
    """
    Return a deep copy of the CloudFormation custom-resource event with
    sensitive parameter values redacted, so the raw API key is never logged.
    """
    try:
        redacted = copy.deepcopy(event)
    except Exception:
        return {"_redaction_error": "unable to copy event for safe logging"}
    # Redact sensitive parameters in both the current and previous property
    # blocks. CloudFormation Update requests also carry OldResourceProperties,
    # which (when StoreAPIKeyInSecretsManager=false) holds the previous raw API
    # key and would otherwise be logged in cleartext.
    for block in ("ResourceProperties", "OldResourceProperties"):
        params = redacted.get(block, {}).get("Parameters", {})
        if isinstance(params, dict):
            for key in params:
                if key.lower() in _SENSITIVE_PARAM_KEYS:
                    params[key] = "***REDACTED***"
    return redacted


def sanitize_statement_id_prefix(identifier):
    """
    Sanitize an identifier for use in Lambda permission statement IDs.
    Ensures only AWS-compatible characters remain using regex.
    Limits output to 65 characters to provide safety margin for StatementId.
    """
    updated_prefix = identifier
    if len(identifier) >= 65:
        updated_prefix = identifier[:60] + identifier[-5:]
    # Use regex to ensure only AWS-compatible characters
    return re.sub(r"[^a-zA-Z0-9\-_]", "_", updated_prefix)


def handle_exceptions(func):
    """
    A decorator that wraps the passed in function and prints exceptions should one occur.
    """

    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        try:
            return func(*args, **kwargs)
        except Exception as e:
            # print(f"Exception in {func.__name__}: {e}")
            stack_trace = traceback.format_exc()
            return f"Exception in {func.__name__}: {e}\n{stack_trace}"

    return wrapper


@handle_exceptions
def configure_dlq(event):
    """
    configure_dlq function used to configure Dead Letter Queue
    """

    if event["RequestType"] == "Delete":
        return

    aws_lambda = boto3.client("lambda")
    lambda_arn = event["ResourceProperties"]["LambdaArn"]
    dlq_arn = event["ResourceProperties"]["DLQ"]["DLQArn"]

    aws_lambda.update_function_configuration(
        FunctionName=lambda_arn,
        DeadLetterConfig={"TargetArn": dlq_arn},
    )

    if event["RequestType"] == "Update":
        print("updating dlq event source mapping...")
        # remove mapping and recreate it on update
        mappings = aws_lambda.list_event_source_mappings(
            FunctionName=lambda_arn,
        )["EventSourceMappings"]

        for mapping in mappings:
            if mapping["EventSourceArn"] == dlq_arn:
                print("deleting previous dlq event source mapping...")
                aws_lambda.delete_event_source_mapping(UUID=mapping["UUID"])

        time.sleep(15)  # give the change a little time to propagate

    # Create event source mapping
    aws_lambda.create_event_source_mapping(
        EventSourceArn=dlq_arn,
        FunctionName=lambda_arn,
        Enabled=True,
        BatchSize=1,
    )


class CFNResponse:
    """
    CFNResponse class, used to handle cloudformation responses
    as lambda can be inconsistent when importing cfnresponse module.
    """

    SUCCESS = "SUCCESS"
    FAILED = "FAILED"

    def __init__(self, event, context):
        self.event = event
        self.context = context
        self.response_url = event["ResponseURL"]

    def send(self, status, response_data=None, physical_resource_id=None, no_echo=False, reason=None):
        if response_data is None:
            response_data = {}
        print(f"sending cloudformation response: [{status}]")
        response_body = {
            "Status": status,
            "Reason": reason or f"See the details in CloudWatch Log Stream: {self.context.log_stream_name}",
            "PhysicalResourceId": physical_resource_id or self.context.log_stream_name,
            "StackId": self.event["StackId"],
            "RequestId": self.event["RequestId"],
            "LogicalResourceId": self.event["LogicalResourceId"],
            "NoEcho": no_echo,
            "Data": response_data,
        }
        json_response_body = json.dumps(response_body).encode("utf-8")
        headers = {"content-type": "", "content-length": str(len(json_response_body))}
        try:
            # CloudFormation custom resources must send status to this AWS-provided presigned URL.
            # codeql[py/full-ssrf]
            req = request.Request(self.response_url, data=json_response_body, headers=headers, method="PUT")
            with request.urlopen(req) as response:
                print("cloudformation response status code:", response.getcode())
        except error.HTTPError as e:
            print("HTTPError:", e.reason)
            print("Status code:", e.code)
        except error.URLError as e:
            print("URLError:", e.reason)


class ConfigureS3Integration:
    """
    ConfigureS3Trigger class used to configure S3 Integration
    """

    def __init__(self, event, context, cfn):
        self.context = context
        self.event = event
        self.cfn = cfn
        self.s3 = boto3.client("s3")
        self.aws_lambda = boto3.client("lambda")
        self.params = SimpleNamespace(**event["ResourceProperties"]["Parameters"])

    @handle_exceptions
    def handle_lambda_permissions(self, bucket_name_list, lambda_function_arn, function_name, request_type):
        for bucket_name in bucket_name_list.split(","):
            sanitized_bucket_name = sanitize_statement_id_prefix(bucket_name)
            statement_id = f"allow-s3-{sanitized_bucket_name}-invoke-{function_name}"
            # Check final statement_id length and truncate if needed
            if len(statement_id) >= 100:
                statement_id = f"allow-s3-{sanitized_bucket_name}-invoke-" + statement_id[-5:]
            try:
                if request_type == "Delete" or request_type == "Update":
                    response = self.aws_lambda.remove_permission(
                        FunctionName=lambda_function_arn, StatementId=statement_id
                    )
                    print("Permission removed from Lambda function:", response)
                if request_type == "Delete":
                    return
            except Exception as e:
                print(f"Couldent remove permission with statement id: {statement_id}, {str(e)}")

            response = self.aws_lambda.add_permission(
                FunctionName=lambda_function_arn,
                StatementId=statement_id,
                Action="lambda:InvokeFunction",
                Principal="s3.amazonaws.com",
                SourceArn=f"arn:aws:s3:::{bucket_name}",
            )
            print("Permission added to Lambda function:", response)

    @handle_exceptions
    def create(self):
        print("Request Type:", self.event["RequestType"])
        for bucket in self.params.S3BucketName.split(","):
            function_name = self.event["ResourceProperties"]["LambdaArn"].split(":")[-1]
            shipper_arn = self.event["ResourceProperties"]["LambdaArn"]
            BucketNotificationConfiguration = self.s3.get_bucket_notification_configuration(Bucket=bucket)
            BucketNotificationConfiguration.pop("ResponseMetadata")
            BucketNotificationConfiguration.setdefault("LambdaFunctionConfigurations", [])

            # Drop any pre-existing configuration for this shipper before
            # re-adding it. On a CFN Create retry the previous (partial) config
            # is still present, and appending unconditionally would register a
            # duplicate/ambiguous notification for the same Lambda.
            BucketNotificationConfiguration["LambdaFunctionConfigurations"] = [
                cfg
                for cfg in BucketNotificationConfiguration["LambdaFunctionConfigurations"]
                if cfg.get("LambdaFunctionArn") != shipper_arn
            ]

            BucketNotificationConfiguration["LambdaFunctionConfigurations"].append(
                {
                    "Id": self.event.get("PhysicalResourceId", self.context.aws_request_id),
                    "LambdaFunctionArn": self.event["ResourceProperties"]["LambdaArn"],
                    "Filter": {
                        "Key": {
                            "FilterRules": [
                                {"Name": "prefix", "Value": self.params.S3KeyPrefix},
                                {"Name": "suffix", "Value": self.params.S3KeySuffix},
                            ]
                        }
                    },
                    "Events": ["s3:ObjectCreated:*"],
                }
            )
            if len(BucketNotificationConfiguration["LambdaFunctionConfigurations"]) == 0:
                BucketNotificationConfiguration.pop("LambdaFunctionConfigurations")
            print(f"nofication configuration: {BucketNotificationConfiguration}")

            err = self.handle_lambda_permissions(
                bucket, self.event["ResourceProperties"]["LambdaArn"], function_name, self.event["RequestType"]
            )

            if err:
                raise Exception(err)

            if self.event["RequestType"] != "Delete":
                print("creating bucket notification configuration...")
                self.s3.put_bucket_notification_configuration(
                    Bucket=bucket, NotificationConfiguration=BucketNotificationConfiguration
                )

        # responseStatus = self.cfn.SUCCESS
        print(self.event["RequestType"], "request completed....")

    @handle_exceptions
    def update(self):
        err = self.delete()
        if err:
            raise Exception(err)

        time.sleep(15)
        return self.create()

    @handle_exceptions
    def delete(self):
        lambda_function_arn = self.event["ResourceProperties"]["LambdaArn"]
        for bucket in self.params.S3BucketName.split(","):
            # Get the current notification configuration for the bucket
            response = self.s3.get_bucket_notification_configuration(Bucket=bucket)

            # Remove Lambda function triggers from the notification configuration
            configs = response.get("LambdaFunctionConfigurations", [])
            if len(configs) == 0:
                print("no previous notification configurations found...")
                return

            updated_configuration = {
                "LambdaFunctionConfigurations": [
                    config for config in configs if config["LambdaFunctionArn"] != lambda_function_arn
                ],
                # Preserve other configurations (if any)
                "TopicConfigurations": response.get("TopicConfigurations", []),
                "QueueConfigurations": response.get("QueueConfigurations", []),
            }

            print("Updated Configuration:", updated_configuration)

            # Update the bucket's notification configuration
            self.s3.put_bucket_notification_configuration(
                Bucket=bucket, NotificationConfiguration=updated_configuration
            )

            print(f"Removed Lambda function {lambda_function_arn} trigger from S3 bucket {bucket}.")

    def handle(self):
        responseStatus = self.cfn.SUCCESS
        on_sns_or_sqs = False

        # Skip S3 trigger configuration if SQS Topic ARN is not a placeholder or empty
        if all(
            [self.params.SNSTopicArn != "arn:aws:sns:us-east-1:123456789012:placeholder", self.params.SNSTopicArn != ""]
        ):
            on_sns_or_sqs = True

        if all(
            [self.params.SQSTopicArn != "arn:aws:sqs:us-east-1:123456789012:placeholder", self.params.SQSTopicArn != ""]
        ):
            on_sns_or_sqs = True

        err = None
        if not on_sns_or_sqs:
            match self.event["RequestType"]:
                case "Create":
                    err = self.create()
                case "Update":
                    err = self.update()
                case "Delete":
                    err = self.delete()
                    if err and "ResourceNotFoundException" in err:
                        err = None

        if err:
            print(f"[ConfigureS3Trigger] failed to process: {err}")
            responseStatus = self.cfn.FAILED

        # send response to cloudformation
        self.cfn.send(
            responseStatus, physical_resource_id=self.event.get("PhysicalResourceId", self.context.aws_request_id)
        )


class ConfigureKafkaIntegration:
    """
    ConfigureKafkaIntegration
    """

    def __init__(self, event, context, cfn):
        self.cfn = cfn
        self.event = event
        self.context = context
        self.aws_lambda = boto3.client("lambda")
        self.params = SimpleNamespace(**event["ResourceProperties"]["Parameters"])
        self.integration = self.params.IntegrationType

    @handle_exceptions
    def create(self):
        if self.integration == "Kafka":
            print("creating kafka event source mapping...")
            function_name = self.event["ResourceProperties"]["LambdaArn"].split(":")[-1]
            response = self.aws_lambda.create_event_source_mapping(
                FunctionName=function_name,
                BatchSize=int(self.params.KafkaBatchSize),
                StartingPosition="LATEST",
                Topics=[self.params.KafkaTopic],
                SelfManagedEventSource={"Endpoints": {"KAFKA_BOOTSTRAP_SERVERS": self.params.KafkaBrokers}},
                SourceAccessConfigurations=list(
                    [{"Type": "VPC_SUBNET", "URI": "subnet:" + subnetId} for subnetId in self.params.KafkaSubnets]
                )
                + list(
                    [
                        {"Type": "VPC_SECURITY_GROUP", "URI": "security_group:" + securityGroupId}
                        for securityGroupId in self.params.KafkaSecurityGroups
                    ]
                ),
            )
            print("create kafka event source mapping response:", response)

        if self.integration == "MSK":
            print("creating msk event source mapping...")
            lambda_name = self.event["ResourceProperties"]["LambdaArn"].split(":")[-1]
            msk_cluster_arn = self.params.MSKClusterArn
            topics = self.params.KafkaTopic.split(",")
            for topic in topics:
                response = self.aws_lambda.create_event_source_mapping(
                    EventSourceArn=msk_cluster_arn,
                    FunctionName=lambda_name,
                    Topics=[topic],
                    StartingPosition="LATEST",
                    BatchSize=100,
                )

            print("create msk event source mapping response:", response)

    @handle_exceptions
    def update(self):
        err = self.delete()
        if err:
            raise Exception(err)
        time.sleep(15)
        return self.create()

    @handle_exceptions
    def delete(self):
        print("msk/kafka deleting previous mapping(s)")
        function_name = self.event["ResourceProperties"]["LambdaArn"].split(":")[-1]
        mappings = self.aws_lambda.list_event_source_mappings(
            FunctionName=function_name,
        )["EventSourceMappings"]
        for mapping in mappings:
            # disable mapping
            if mapping["State"] == "Enabled":
                self.aws_lambda.update_event_source_mapping(
                    UUID=mapping["UUID"], FunctionName=function_name, Enabled=False
                )
            time.sleep(10)  # wait for mapping to be disabled
            self.aws_lambda.delete_event_source_mapping(UUID=mapping["UUID"])

    def handle(self):
        print(f"configuring {self.integration} Integration")
        responseStatus = self.cfn.SUCCESS
        match self.event["RequestType"]:
            case "Create":
                err = self.create()
            case "Update":
                err = self.update()
            case "Delete":
                err = self.delete()
                if err and "ResourceNotFoundException" in err:
                    err = None

        if err:
            print(f"[ConfigureKafka/MSKIntegration] failed to process: {err}")
            responseStatus = self.cfn.FAILED

        # send response to cloudformation
        self.cfn.send(
            responseStatus, physical_resource_id=self.event.get("PhysicalResourceId", self.context.aws_request_id)
        )


class ConfigureCloudwatchIntegration:
    """
    ConfigureCloudwatchIntegration handles the configuration of Cloudwatch Integration
    """

    def __init__(self, event, context, cfn):
        self.event = event
        self.context = context
        self.cfn = cfn
        self.aws_lambda = boto3.client("lambda")
        self.cloudwatch_logs = boto3.client("logs")
        self.aws_lambda = boto3.client("lambda")
        self.params = SimpleNamespace(**event["ResourceProperties"]["Parameters"])

    @handle_exceptions
    def create(self):
        lambda_arn = self.event["ResourceProperties"]["LambdaArn"]
        custom_lambda_arn = os.environ["AWS_LAMBDA_FUNCTION_NAME"]
        region = self.context.invoked_function_arn.split(":")[3]
        account_id = self.context.invoked_function_arn.split(":")[4]
        logGroupNames = self.params.CloudWatchLogGroupName.split(",")
        LambdaPremissionPrefix = self.params.CloudWatchLogGroupPrefix.split(",")
        environment_variables = {"log_groups": self.params.CloudWatchLogGroupName}
        self.update_custom_lambda_environment_variables(custom_lambda_arn, environment_variables)
        if LambdaPremissionPrefix and LambdaPremissionPrefix != [""]:
            for prefix in LambdaPremissionPrefix:
                replaced_prefix = sanitize_statement_id_prefix(prefix)
                try:
                    self.aws_lambda.add_permission(
                        FunctionName=lambda_arn,
                        StatementId=f"allow-trigger-from-{replaced_prefix}-log-groups",
                        Action="lambda:InvokeFunction",
                        Principal="logs.amazonaws.com",
                        SourceArn=f"arn:aws:logs:{region}:{account_id}:log-group:{prefix}*:*",
                    )
                except Exception as e:
                    print("assuming permission already exists: ", str(e))

        for log_group in logGroupNames:
            if (
                LambdaPremissionPrefix
                and LambdaPremissionPrefix != [""]
                and any(log_group.startswith(prefix) for prefix in LambdaPremissionPrefix)
            ):
                continue
            response = self.cloudwatch_logs.describe_subscription_filters(
                logGroupName=log_group, filterNamePrefix=f"coralogix-aws-shipper-cloudwatch-trigger-{lambda_arn[-4:]}"
            )
            if (
                not response.get("subscriptionFilters")
                or response.get("subscriptionFilters")[0].get("destinationArn") != lambda_arn
            ):
                replaced_prefix = sanitize_statement_id_prefix(log_group)
                try:
                    self.aws_lambda.add_permission(
                        FunctionName=lambda_arn,
                        StatementId=f"allow-trigger-from-{replaced_prefix}",
                        Action="lambda:InvokeFunction",
                        Principal="logs.amazonaws.com",
                        SourceArn=f"arn:aws:logs:{region}:{account_id}:log-group:{log_group}:*",
                    )
                except Exception as e:
                    print("assuming permission already exists: ", str(e))

        time.sleep(15)

        for log_group in logGroupNames:
            self.cloudwatch_logs.put_subscription_filter(
                destinationArn=self.event["ResourceProperties"]["LambdaArn"],
                filterName=f"coralogix-aws-shipper-cloudwatch-trigger-{lambda_arn[-4:]}",
                filterPattern="",
                logGroupName=log_group,
            )

    def update_custom_lambda_environment_variables(self, function_name, new_environment_variables):
        self.aws_lambda.update_function_configuration(
            FunctionName=function_name, Environment={"Variables": new_environment_variables}
        )

    def remove_subscription_filter(self, log_group, lambda_arn):
        response = self.cloudwatch_logs.describe_subscription_filters(logGroupName=log_group)
        lambda_arn = self.event["ResourceProperties"]["LambdaArn"]
        LambdaPremissionPrefix = self.params.CloudWatchLogGroupPrefix.split(",")
        for filter in response["subscriptionFilters"]:
            if filter["filterName"] == f"coralogix-aws-shipper-cloudwatch-trigger-{lambda_arn[-4:]}":
                self.cloudwatch_logs.delete_subscription_filter(
                    filterName=f"coralogix-aws-shipper-cloudwatch-trigger-{lambda_arn[-4:]}", logGroupName=log_group
                )
            # CloudWatchLogGroupPrefix.split(',') yields [''] (not []) when the
            # prefix is empty, so guard against that explicitly; otherwise the
            # per-log-group invoke permission added in create() is never removed.
            if not LambdaPremissionPrefix or LambdaPremissionPrefix == [""]:
                replaced_prefix = sanitize_statement_id_prefix(log_group)
                response = self.aws_lambda.remove_permission(
                    FunctionName=lambda_arn, StatementId=f"allow-trigger-from-{replaced_prefix}"
                )

    @handle_exceptions
    def update(self):
        custom_lambda_name = os.environ["AWS_LAMBDA_FUNCTION_NAME"]
        new_log_group_names = self.params.CloudWatchLogGroupName.split(",")
        new_environment_variables = {"log_groups": self.params.CloudWatchLogGroupName}

        old_log_group_names = os.environ.get("log_groups", "").split(",")
        for old_log_group in old_log_group_names:
            if old_log_group not in new_log_group_names:
                self.remove_subscription_filter(old_log_group, custom_lambda_name)

        self.update_custom_lambda_environment_variables(custom_lambda_name, new_environment_variables)
        self.create()

        err = self.delete()
        if err:
            raise Exception(err)
        time.sleep(15)
        return self.create()

    @handle_exceptions
    def delete(self):
        lambda_arn = self.event["ResourceProperties"]["LambdaArn"]
        logGroupNames = self.params.CloudWatchLogGroupName.split(",")
        for log_group in logGroupNames:
            self.remove_subscription_filter(log_group, lambda_arn)

    def handle(self):
        responseStatus = self.cfn.SUCCESS
        match self.event["RequestType"]:
            case "Create":
                err = self.create()
            case "Update":
                err = self.update()
            case "Delete":
                err = self.delete()
                if err and "ResourceNotFoundException" in err:
                    err = None

        if err:
            print(f"[ConfigureCloudwatchTrigger] failed to process: {err}")
            responseStatus = self.cfn.FAILED

        # send response to cloudformation
        self.cfn.send(
            responseStatus, physical_resource_id=self.event.get("PhysicalResourceId", self.context.aws_request_id)
        )


class ConfigureMetricsIntegration:
    """
    ConfigureMetricsIntegration handles the configuration of Metrics Integration
        - Create CloudWatch Metric Stream
    """

    def __init__(self, event, context, cfn):
        self.event = event
        self.context = context
        self.cfn = cfn
        self.params = SimpleNamespace(**self.event["ResourceProperties"]["Parameters"])
        self.cloudwatch_metrics = boto3.client("cloudwatch")

    @handle_exceptions
    def create(self):
        print("creating cloudwatch metric stream...")
        stream_params = {
            "Name": self.params.CWMetricStreamName,
            "FirehoseArn": self.params.CWStreamFirehoseDestinationARN,
            "RoleArn": self.params.CWStreamFirehoseAccessRoleARN,
            "OutputFormat": "opentelemetry1.0",
        }

        # Add IncludeFilters and ExcludeFilters only if the parameters are not empty
        if self.params.MetricsFilter != "":
            stream_params["IncludeFilters"] = json.loads(self.params.MetricsFilter)
        if self.params.ExcludeMetricsFilters != "":
            stream_params["ExcludeFilters"] = json.loads(self.params.ExcludeMetricsFilters)

        response = self.cloudwatch_metrics.put_metric_stream(**stream_params)
        print("create cloudwatch metric stream response:", response)

    @handle_exceptions
    def update(self):
        err = self.delete()
        if err:
            raise Exception(err)
        time.sleep(30)  # give time for resource to be deleted
        return self.create()

    @handle_exceptions
    def delete(self):
        print(f"deleting cloudwatch metric stream... {self.params.CWMetricStreamName}")
        response = self.cloudwatch_metrics.delete_metric_stream(
            Name=self.params.CWMetricStreamName,
        )
        print("delete cloudwatch metric stream response:", response)

    def handle(self):
        responseStatus = self.cfn.SUCCESS
        match self.event["RequestType"]:
            case "Create":
                err = self.create()
            case "Update":
                err = self.update()
            case "Delete":
                err = self.delete()
                if err and "ResourceNotFoundException" in err:
                    err = None

        if err:
            print(f"[ConfigureMetricsIntegration] failed to process: {err}")
            responseStatus = self.cfn.FAILED

        # send response to cloudformation
        self.cfn.send(
            responseStatus, physical_resource_id=self.event.get("PhysicalResourceId", self.context.aws_request_id)
        )


def lambda_handler(event, context):
    """
    AWS Lambda handler function
    """
    print("Received event:", redact_event(event))
    cfn = CFNResponse(event, context)

    # handle metrics integration
    if event["ResourceProperties"]["Parameters"]["TelemetryMode"] == "metrics":
        print("telemetry mode is set to metrics")
        try:
            ConfigureMetricsIntegration(event, context, cfn).handle()
        except Exception as e:
            stack_trace = traceback.format_exc()
            print(f"failed to process: {e}\n{stack_trace}")
            cfn.send(cfn.FAILED, response_data={}, physical_resource_id=None, no_echo=False, reason=None)
        return

    integration_type = event["ResourceProperties"]["Parameters"]["IntegrationType"]
    dlq_enabled = event["ResourceProperties"]["DLQ"].get("EnableDLQ", False)
    if dlq_enabled == "false":
        dlq_enabled = False

    try:
        print("checking dlq_enabled:", dlq_enabled)
        if dlq_enabled:
            err = configure_dlq(event)
            if err:
                raise Exception(err)

        match integration_type:
            case "S3" | "S3Csv" | "VpcFlow" | "CloudTrail" | "CloudFront":
                ConfigureS3Integration(event, context, cfn).handle()
            case "Kafka":
                ConfigureKafkaIntegration(event, context, cfn).handle()
            case "CloudWatch":
                ConfigureCloudwatchIntegration(event, context, cfn).handle()
            case "Kinesis" | "MSK" | "Sqs" | "Sns" | "EcrScan":
                cfn.send(cfn.SUCCESS, response_data={}, physical_resource_id=None, no_echo=False, reason=None)
                return
            case _:
                raise ValueError(f"invalid or unsupported integration type: {integration_type}")

    except Exception as e:
        stack_trace = traceback.format_exc()
        print(f"Failed to process: {e}\n{stack_trace}")
        cfn.send(cfn.FAILED, response_data={}, physical_resource_id=None, no_echo=False, reason=None)

    return
