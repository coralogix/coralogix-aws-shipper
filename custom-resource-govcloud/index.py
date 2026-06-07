"""
Custom-resource orchestrator for coralogix-aws-shipper on AWS GovCloud.

Handles CloudFormation custom resource events to configure S3 bucket notifications,
CloudWatch Logs subscription filters, Kafka/MSK event source mappings, CloudWatch
Metric Streams, and Dead Letter Queues for the shipper Lambda function.

All ARN construction uses the AWS partition token received from CloudFormation
(AWS::Partition evaluates to "aws-us-gov" in GovCloud regions).
"""

import json
import boto3
import os
import re
import time
import copy
import traceback
import functools
from urllib import request, error
from types import SimpleNamespace


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Lambda StatementId has a hard limit of 100 characters. Callers wrap the
# sanitized value with fixed prefixes/suffixes (e.g. "allow-s3-...-invoke-...",
# "allow-trigger-from-...-log-groups"), so the sanitized portion is capped well
# below 100 to leave room for those additions.
_MAX_SANITIZED_PREFIX_LEN = 45


def sanitize_statement_id_prefix(identifier: str) -> str:
    """
    Sanitize an identifier for use in Lambda permission statement IDs.

    Ensures only AWS-compatible characters remain and caps the length at
    _MAX_SANITIZED_PREFIX_LEN so that callers have headroom to append their own
    fixed prefixes/suffixes without exceeding the 100-character StatementId limit.
    """
    updated_prefix = identifier
    if len(identifier) > _MAX_SANITIZED_PREFIX_LEN:
        head = _MAX_SANITIZED_PREFIX_LEN - 5
        updated_prefix = identifier[:head] + identifier[-5:]
    return re.sub(r"[^a-zA-Z0-9\-_]", "_", updated_prefix)


# Parameter keys whose values are sensitive and must never reach CloudWatch Logs.
# When StoreAPIKeyInSecretsManager=false the template passes the raw Coralogix
# API key through ResourceProperties.Parameters, so it must be redacted before
# the event is logged.
_SENSITIVE_PARAM_KEYS = {"apikey", "coralogixapikey"}
_CLOUDFORMATION_RESPONSE_URL_PATTERN = (
    r"https://cloudformation-custom-resource-response-[a-z0-9-]+"
    r"\.s3(?:[.-][a-z0-9-]+)?\.amazonaws\.com(?:\.cn)?/.*"
)


def validate_cloudformation_response_url(response_url):
    if not re.fullmatch(_CLOUDFORMATION_RESPONSE_URL_PATTERN, response_url):
        raise ValueError("Invalid CloudFormation response URL")
    return response_url


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


def handle_exceptions(func):
    """Decorator that catches exceptions and returns them as error strings."""

    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        try:
            return func(*args, **kwargs)
        except Exception as e:
            stack_trace = traceback.format_exc()
            return f"Exception in {func.__name__}: {e}\n{stack_trace}"

    return wrapper


# ---------------------------------------------------------------------------
# CloudFormation response helper
# ---------------------------------------------------------------------------


class CFNResponse:
    SUCCESS = "SUCCESS"
    FAILED = "FAILED"

    def __init__(self, event, context):
        self.event = event
        self.context = context
        self.response_url = validate_cloudformation_response_url(event["ResponseURL"])

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
        headers = {
            "content-type": "",
            "content-length": str(len(json_response_body)),
        }
        try:
            req = request.Request(
                self.response_url,
                data=json_response_body,
                headers=headers,
                method="PUT",
            )
            with request.urlopen(req) as response:
                print("cloudformation response status code:", response.getcode())
        except error.HTTPError as e:
            print("HTTPError:", e.reason, "Status code:", e.code)
        except error.URLError as e:
            print("URLError:", e.reason)


# ---------------------------------------------------------------------------
# DLQ configuration
# ---------------------------------------------------------------------------


@handle_exceptions
def configure_dlq(event):
    """Configure Dead Letter Queue for the shipper Lambda."""
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
        mappings = aws_lambda.list_event_source_mappings(
            FunctionName=lambda_arn,
        )["EventSourceMappings"]
        for mapping in mappings:
            if mapping["EventSourceArn"] == dlq_arn:
                print("deleting previous dlq event source mapping...")
                aws_lambda.delete_event_source_mapping(UUID=mapping["UUID"])
        time.sleep(15)

    aws_lambda.create_event_source_mapping(
        EventSourceArn=dlq_arn,
        FunctionName=lambda_arn,
        Enabled=True,
        BatchSize=1,
    )


# ---------------------------------------------------------------------------
# S3 integration (bind-existing)
# ---------------------------------------------------------------------------


class ConfigureS3Integration:
    """
    Configure S3 bucket notifications for the shipper Lambda.
    Handles existing S3 buckets (bind-existing pattern).
    """

    # GovCloud placeholder sentinels (match template-govcloud.yaml defaults)
    _SNS_PLACEHOLDER = "arn:aws-us-gov:sns:us-gov-west-1:123456789012:placeholder"
    _SQS_PLACEHOLDER = "arn:aws-us-gov:sqs:us-gov-west-1:123456789012:placeholder"

    def __init__(self, event, context, cfn, partition: str):
        self.context = context
        self.event = event
        self.cfn = cfn
        self.partition = partition
        self.s3 = boto3.client("s3")
        self.aws_lambda = boto3.client("lambda")
        self.params = SimpleNamespace(**event["ResourceProperties"]["Parameters"])

    @handle_exceptions
    def _handle_lambda_permissions(self, bucket_name_list, lambda_function_arn, function_name, request_type):
        for bucket_name in bucket_name_list.split(","):
            sanitized = sanitize_statement_id_prefix(bucket_name)
            statement_id = f"allow-s3-{sanitized}-invoke-{function_name}"
            if len(statement_id) >= 100:
                statement_id = f"allow-s3-{sanitized}-invoke-" + statement_id[-5:]
            try:
                if request_type in ("Delete", "Update"):
                    resp = self.aws_lambda.remove_permission(
                        FunctionName=lambda_function_arn,
                        StatementId=statement_id,
                    )
                    print("Permission removed from Lambda function:", resp)
                if request_type == "Delete":
                    return
            except Exception as e:
                print(f"Could not remove permission with statement id: {statement_id}, {e}")

            source_arn = f"arn:{self.partition}:s3:::{bucket_name}"
            resp = self.aws_lambda.add_permission(
                FunctionName=lambda_function_arn,
                StatementId=statement_id,
                Action="lambda:InvokeFunction",
                Principal="s3.amazonaws.com",
                SourceArn=source_arn,
            )
            print("Permission added to Lambda function:", resp)

    @handle_exceptions
    def create(self):
        print("Request Type:", self.event["RequestType"])
        for bucket in self.params.S3BucketName.split(","):
            function_name = self.event["ResourceProperties"]["LambdaArn"].split(":")[-1]
            shipper_arn = self.event["ResourceProperties"]["LambdaArn"]
            bucket_notification = self.s3.get_bucket_notification_configuration(Bucket=bucket)
            bucket_notification.pop("ResponseMetadata")
            bucket_notification.setdefault("LambdaFunctionConfigurations", [])

            # Drop any pre-existing configuration for this shipper before
            # re-adding it. On a CFN Create retry the previous (partial) config
            # is still present, and appending unconditionally would register a
            # duplicate/ambiguous notification for the same Lambda.
            bucket_notification["LambdaFunctionConfigurations"] = [
                cfg
                for cfg in bucket_notification["LambdaFunctionConfigurations"]
                if cfg.get("LambdaFunctionArn") != shipper_arn
            ]

            bucket_notification["LambdaFunctionConfigurations"].append(
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

            print(f"notification configuration: {bucket_notification}")

            err = self._handle_lambda_permissions(
                bucket,
                self.event["ResourceProperties"]["LambdaArn"],
                function_name,
                self.event["RequestType"],
            )
            if err:
                raise Exception(err)

            if self.event["RequestType"] != "Delete":
                print("creating bucket notification configuration...")
                self.s3.put_bucket_notification_configuration(
                    Bucket=bucket,
                    NotificationConfiguration=bucket_notification,
                )

        print(self.event["RequestType"], "request completed....")

    @handle_exceptions
    def update(self):
        # On Update, clean up notifications from the PREVIOUS bucket list
        # (OldResourceProperties), otherwise buckets removed from S3BucketName
        # keep their stale Lambda notification and continue invoking the shipper.
        old_params = self.event.get("OldResourceProperties", {}).get("Parameters", {})
        old_bucket_names = old_params.get("S3BucketName", self.params.S3BucketName)
        err = self.delete(bucket_names=old_bucket_names)
        if err:
            raise Exception(err)
        time.sleep(15)
        return self.create()

    @handle_exceptions
    def delete(self, bucket_names=None):
        lambda_function_arn = self.event["ResourceProperties"]["LambdaArn"]
        if bucket_names is None:
            bucket_names = self.params.S3BucketName
        for bucket in bucket_names.split(","):
            response = self.s3.get_bucket_notification_configuration(Bucket=bucket)
            configs = response.get("LambdaFunctionConfigurations", [])
            if not configs:
                print(f"no previous notification configurations found for bucket {bucket}...")
                continue

            updated_configuration = {
                "LambdaFunctionConfigurations": [
                    cfg for cfg in configs if cfg["LambdaFunctionArn"] != lambda_function_arn
                ],
                "TopicConfigurations": response.get("TopicConfigurations", []),
                "QueueConfigurations": response.get("QueueConfigurations", []),
            }
            print("Updated Configuration:", updated_configuration)
            self.s3.put_bucket_notification_configuration(
                Bucket=bucket,
                NotificationConfiguration=updated_configuration,
            )
            print(f"Removed Lambda function {lambda_function_arn} trigger from S3 bucket {bucket}.")

    def handle(self):
        response_status = self.cfn.SUCCESS
        on_sns_or_sqs = False

        if all(
            [
                self.params.SNSTopicArn != self._SNS_PLACEHOLDER,
                self.params.SNSTopicArn != "",
            ]
        ):
            on_sns_or_sqs = True

        if all(
            [
                self.params.SQSTopicArn != self._SQS_PLACEHOLDER,
                self.params.SQSTopicArn != "",
            ]
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
            response_status = self.cfn.FAILED

        self.cfn.send(
            response_status,
            physical_resource_id=self.event.get("PhysicalResourceId", self.context.aws_request_id),
        )


# ---------------------------------------------------------------------------
# Kafka / MSK integration
# ---------------------------------------------------------------------------


class ConfigureKafkaIntegration:
    """Configure Kafka / MSK event source mappings."""

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
                SelfManagedEventSource={
                    "Endpoints": {
                        "KAFKA_BOOTSTRAP_SERVERS": self.params.KafkaBrokers,
                    }
                },
                SourceAccessConfigurations=(
                    [{"Type": "VPC_SUBNET", "URI": f"subnet:{sid}"} for sid in self.params.KafkaSubnets]
                    + [
                        {"Type": "VPC_SECURITY_GROUP", "URI": f"security_group:{sg}"}
                        for sg in self.params.KafkaSecurityGroups
                    ]
                ),
            )
            print("create kafka event source mapping response:", response)

        if self.integration == "MSK":
            print("creating msk event source mapping...")
            lambda_name = self.event["ResourceProperties"]["LambdaArn"].split(":")[-1]
            msk_cluster_arn = self.params.MSKClusterArn
            topics = self.params.KafkaTopic.split(",")
            response = None
            for topic in topics:
                response = self.aws_lambda.create_event_source_mapping(
                    EventSourceArn=msk_cluster_arn,
                    FunctionName=lambda_name,
                    Topics=[topic],
                    StartingPosition="LATEST",
                    BatchSize=100,
                )
            if response is not None:
                print("create msk event source mapping response:", response)

    @handle_exceptions
    def update(self):
        err = self.delete()
        if err:
            raise Exception(err)
        time.sleep(15)
        return self.create()

    def _is_managed_mapping(self, mapping):
        """
        Return True only for the Kafka/MSK source mapping owned by this
        integration. Other mappings on the same Lambda (e.g. the DLQ SQS
        mapping created by configure_dlq) must be left untouched.
        """
        # Self-managed Kafka mappings have no EventSourceArn; they are
        # identified by the SelfManagedEventSource block.
        if self.integration == "Kafka":
            return "SelfManagedEventSource" in mapping
        # MSK mappings reference the cluster ARN as their EventSourceArn.
        if self.integration == "MSK":
            msk_cluster_arn = getattr(self.params, "MSKClusterArn", None)
            return mapping.get("EventSourceArn") == msk_cluster_arn
        return False

    @handle_exceptions
    def delete(self):
        print("msk/kafka deleting previous mapping(s)")
        function_name = self.event["ResourceProperties"]["LambdaArn"].split(":")[-1]
        mappings = self.aws_lambda.list_event_source_mappings(
            FunctionName=function_name,
        )["EventSourceMappings"]
        for mapping in mappings:
            if not self._is_managed_mapping(mapping):
                print(f"skipping unrelated event source mapping: {mapping.get('UUID')}")
                continue
            if mapping["State"] == "Enabled":
                self.aws_lambda.update_event_source_mapping(
                    UUID=mapping["UUID"],
                    FunctionName=function_name,
                    Enabled=False,
                )
            time.sleep(10)
            self.aws_lambda.delete_event_source_mapping(UUID=mapping["UUID"])

    def handle(self):
        print(f"configuring {self.integration} Integration")
        response_status = self.cfn.SUCCESS
        err = None
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
            response_status = self.cfn.FAILED

        self.cfn.send(
            response_status,
            physical_resource_id=self.event.get("PhysicalResourceId", self.context.aws_request_id),
        )


# ---------------------------------------------------------------------------
# CloudWatch Logs integration (bind-existing)
# ---------------------------------------------------------------------------


class ConfigureCloudwatchIntegration:
    """Configure CloudWatch Logs subscription filters for the shipper Lambda."""

    def __init__(self, event, context, cfn, partition: str):
        self.event = event
        self.context = context
        self.cfn = cfn
        self.partition = partition
        self.aws_lambda = boto3.client("lambda")
        self.cloudwatch_logs = boto3.client("logs")
        self.params = SimpleNamespace(**event["ResourceProperties"]["Parameters"])

    @handle_exceptions
    def create(self):
        lambda_arn = self.event["ResourceProperties"]["LambdaArn"]
        custom_lambda_arn = os.environ["AWS_LAMBDA_FUNCTION_NAME"]
        region = self.context.invoked_function_arn.split(":")[3]
        account_id = self.context.invoked_function_arn.split(":")[4]
        log_group_names = self.params.CloudWatchLogGroupName.split(",")
        lambda_permission_prefixes = self.params.CloudWatchLogGroupPrefix.split(",")
        logs_principal = f"logs.{region}.amazonaws.com"
        environment_variables = {"log_groups": self.params.CloudWatchLogGroupName}
        self._update_custom_lambda_env(custom_lambda_arn, environment_variables)

        if lambda_permission_prefixes and lambda_permission_prefixes != [""]:
            for prefix in lambda_permission_prefixes:
                replaced_prefix = sanitize_statement_id_prefix(prefix)
                source_arn = f"arn:{self.partition}:logs:{region}:{account_id}:log-group:{prefix}*:*"
                try:
                    self.aws_lambda.add_permission(
                        FunctionName=lambda_arn,
                        StatementId=f"allow-trigger-from-{replaced_prefix}-log-groups",
                        Action="lambda:InvokeFunction",
                        Principal=logs_principal,
                        SourceArn=source_arn,
                    )
                except Exception as e:
                    print("assuming permission already exists: ", str(e))

        for log_group in log_group_names:
            if (
                lambda_permission_prefixes
                and lambda_permission_prefixes != [""]
                and any(log_group.startswith(p) for p in lambda_permission_prefixes)
            ):
                continue
            response = self.cloudwatch_logs.describe_subscription_filters(
                logGroupName=log_group,
                filterNamePrefix=f"coralogix-aws-shipper-cloudwatch-trigger-{lambda_arn[-4:]}",
            )
            if (
                not response.get("subscriptionFilters")
                or response["subscriptionFilters"][0].get("destinationArn") != lambda_arn
            ):
                replaced_prefix = sanitize_statement_id_prefix(log_group)
                source_arn = f"arn:{self.partition}:logs:{region}:{account_id}:log-group:{log_group}:*"
                try:
                    self.aws_lambda.add_permission(
                        FunctionName=lambda_arn,
                        StatementId=f"allow-trigger-from-{replaced_prefix}",
                        Action="lambda:InvokeFunction",
                        Principal=logs_principal,
                        SourceArn=source_arn,
                    )
                except Exception as e:
                    print("assuming permission already exists: ", str(e))

        time.sleep(15)

        for log_group in log_group_names:
            self.cloudwatch_logs.put_subscription_filter(
                destinationArn=lambda_arn,
                filterName=f"coralogix-aws-shipper-cloudwatch-trigger-{lambda_arn[-4:]}",
                filterPattern="",
                logGroupName=log_group,
            )

    def _update_custom_lambda_env(self, function_name, new_env_vars):
        self.aws_lambda.update_function_configuration(
            FunctionName=function_name,
            Environment={"Variables": new_env_vars},
        )

    def _remove_subscription_filter(self, log_group):
        response = self.cloudwatch_logs.describe_subscription_filters(logGroupName=log_group)
        lambda_arn = self.event["ResourceProperties"]["LambdaArn"]
        lambda_permission_prefixes = self.params.CloudWatchLogGroupPrefix.split(",")
        for flt in response["subscriptionFilters"]:
            if flt["filterName"] == f"coralogix-aws-shipper-cloudwatch-trigger-{lambda_arn[-4:]}":
                self.cloudwatch_logs.delete_subscription_filter(
                    filterName=f"coralogix-aws-shipper-cloudwatch-trigger-{lambda_arn[-4:]}",
                    logGroupName=log_group,
                )
            # CloudWatchLogGroupPrefix.split(',') yields [''] (not []) when the
            # prefix is empty, so guard against that explicitly; otherwise the
            # per-log-group invoke permission added in create() is never removed.
            if not lambda_permission_prefixes or lambda_permission_prefixes == [""]:
                replaced_prefix = sanitize_statement_id_prefix(log_group)
                self.aws_lambda.remove_permission(
                    FunctionName=lambda_arn,
                    StatementId=f"allow-trigger-from-{replaced_prefix}",
                )

    @handle_exceptions
    def update(self):
        custom_lambda_name = os.environ["AWS_LAMBDA_FUNCTION_NAME"]
        new_log_group_names = self.params.CloudWatchLogGroupName.split(",")
        new_env_vars = {"log_groups": self.params.CloudWatchLogGroupName}

        old_log_group_names = os.environ.get("log_groups", "").split(",")
        for old_log_group in old_log_group_names:
            if old_log_group not in new_log_group_names:
                self._remove_subscription_filter(old_log_group)

        self._update_custom_lambda_env(custom_lambda_name, new_env_vars)

        err = self.delete()
        if err:
            raise Exception(err)
        time.sleep(15)
        return self.create()

    @handle_exceptions
    def delete(self):
        log_group_names = self.params.CloudWatchLogGroupName.split(",")
        for log_group in log_group_names:
            self._remove_subscription_filter(log_group)

    def handle(self):
        response_status = self.cfn.SUCCESS
        err = None
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
            response_status = self.cfn.FAILED

        self.cfn.send(
            response_status,
            physical_resource_id=self.event.get("PhysicalResourceId", self.context.aws_request_id),
        )


# ---------------------------------------------------------------------------
# Metrics integration
# ---------------------------------------------------------------------------


class ConfigureMetricsIntegration:
    """Configure CloudWatch Metric Streams for the shipper Lambda."""

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
        time.sleep(30)
        return self.create()

    @handle_exceptions
    def delete(self):
        print(f"deleting cloudwatch metric stream... {self.params.CWMetricStreamName}")
        response = self.cloudwatch_metrics.delete_metric_stream(
            Name=self.params.CWMetricStreamName,
        )
        print("delete cloudwatch metric stream response:", response)

    def handle(self):
        response_status = self.cfn.SUCCESS
        err = None
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
            response_status = self.cfn.FAILED

        self.cfn.send(
            response_status,
            physical_resource_id=self.event.get("PhysicalResourceId", self.context.aws_request_id),
        )


# ---------------------------------------------------------------------------
# Lambda handler entry point
# ---------------------------------------------------------------------------


def lambda_handler(event, context):
    """AWS Lambda handler for the custom-resource orchestrator."""
    print("Received event:", redact_event(event))
    cfn = CFNResponse(event, context)

    partition = event["ResourceProperties"]["Parameters"].get("AWSPartition", "aws-us-gov")
    print(f"Operating in partition: {partition}")

    # Metrics mode
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
                ConfigureS3Integration(event, context, cfn, partition).handle()
            case "Kafka":
                ConfigureKafkaIntegration(event, context, cfn).handle()
            case "CloudWatch":
                ConfigureCloudwatchIntegration(event, context, cfn, partition).handle()
            case "Kinesis" | "MSK" | "Sqs" | "Sns" | "EcrScan":
                cfn.send(cfn.SUCCESS, response_data={}, physical_resource_id=None, no_echo=False, reason=None)
                return
            case _:
                raise ValueError(f"invalid or unsupported integration type: {integration_type}")

    except Exception as e:
        stack_trace = traceback.format_exc()
        print(f"Failed to process: {e}\n{stack_trace}")
        cfn.send(cfn.FAILED, response_data={}, physical_resource_id=None, no_echo=False, reason=None)
