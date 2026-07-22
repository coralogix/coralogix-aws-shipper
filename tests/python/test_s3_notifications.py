import importlib.util
import sys
import unittest
from pathlib import Path
from types import ModuleType, SimpleNamespace
from unittest.mock import MagicMock


sys.modules.setdefault("boto3", ModuleType("boto3"))


def load_custom_resource(name: str, path: Path) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"Unable to load custom resource from {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ConfigureS3IntegrationDeleteTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        repository_root = Path(__file__).resolve().parents[2]
        cls.modules = {
            "standard": load_custom_resource(
                "standard_custom_resource", repository_root / "custom-resource" / "index.py"
            ),
            "govcloud": load_custom_resource(
                "govcloud_custom_resource", repository_root / "custom-resource-govcloud" / "index.py"
            ),
        }

    def integration(self, module: ModuleType, response: dict[str, object]) -> object:
        integration = module.ConfigureS3Integration.__new__(module.ConfigureS3Integration)
        integration.event = {"ResourceProperties": {"LambdaArn": "arn:aws:lambda:region:account:function:shipper"}}
        integration.params = SimpleNamespace(S3BucketName="logs-bucket")
        integration.s3 = MagicMock()
        integration.s3.get_bucket_notification_configuration.return_value = response
        return integration

    def test_delete_preserves_eventbridge_configuration(self) -> None:
        for name, module in self.modules.items():
            with self.subTest(custom_resource=name):
                integration = self.integration(
                    module,
                    {
                        "LambdaFunctionConfigurations": [
                            {"LambdaFunctionArn": "arn:aws:lambda:region:account:function:shipper"}
                        ],
                        "TopicConfigurations": [{"Id": "topic-notification"}],
                        "QueueConfigurations": [{"Id": "queue-notification"}],
                        "EventBridgeConfiguration": {},
                        "ResponseMetadata": {"RequestId": "request-id"},
                    },
                )

                self.assertIsNone(integration.delete())
                integration.s3.put_bucket_notification_configuration.assert_called_once_with(
                    Bucket="logs-bucket",
                    NotificationConfiguration={
                        "LambdaFunctionConfigurations": [],
                        "TopicConfigurations": [{"Id": "topic-notification"}],
                        "QueueConfigurations": [{"Id": "queue-notification"}],
                        "EventBridgeConfiguration": {},
                    },
                )

    def test_delete_does_not_enable_eventbridge_when_absent(self) -> None:
        for name, module in self.modules.items():
            with self.subTest(custom_resource=name):
                integration = self.integration(
                    module,
                    {
                        "LambdaFunctionConfigurations": [
                            {"LambdaFunctionArn": "arn:aws:lambda:region:account:function:shipper"}
                        ],
                        "ResponseMetadata": {"RequestId": "request-id"},
                    },
                )

                self.assertIsNone(integration.delete())
                notification_configuration = integration.s3.put_bucket_notification_configuration.call_args.kwargs[
                    "NotificationConfiguration"
                ]
                self.assertNotIn("EventBridgeConfiguration", notification_configuration)


if __name__ == "__main__":
    unittest.main()
