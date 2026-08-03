import sys
import unittest
from pathlib import Path
from types import ModuleType


CASES = (
    (
        {
            "TelemetryMode": "logs",
            "UsePrivateLink": "true",
            "LogExportProtocol": "otlp_grpc",
            "OTLPEndpoint": "",
        },
        False,
    ),
    (
        {
            "TelemetryMode": "logs",
            "UsePrivateLink": "true",
            "LogExportProtocol": "otlp_grpc",
            "OTLPEndpoint": "https://collector.internal:4317",
        },
        True,
    ),
    (
        {
            "TelemetryMode": "logs",
            "UsePrivateLink": "true",
            "LogExportProtocol": "coralogix_rest",
            "OTLPEndpoint": "",
        },
        True,
    ),
    (
        {
            "TelemetryMode": "logs",
            "UsePrivateLink": "false",
            "LogExportProtocol": "otlp_grpc",
            "OTLPEndpoint": "",
        },
        True,
    ),
    (
        {
            "TelemetryMode": "metrics",
            "UsePrivateLink": "true",
            "LogExportProtocol": "otlp_grpc",
            "OTLPEndpoint": "",
        },
        True,
    ),
)


def evaluate(expression: object, parameters: dict[str, str]) -> object:
    if not isinstance(expression, dict):
        return expression

    if "Ref" in expression:
        return parameters[expression["Ref"]]
    if "Fn::Equals" in expression:
        left, right = expression["Fn::Equals"]
        return evaluate(left, parameters) == evaluate(right, parameters)
    if "Fn::Not" in expression:
        return not evaluate(expression["Fn::Not"][0], parameters)
    if "Fn::And" in expression:
        return all(evaluate(item, parameters) for item in expression["Fn::And"])
    if "Fn::Or" in expression:
        return any(evaluate(item, parameters) for item in expression["Fn::Or"])

    raise ValueError(f"Unsupported CloudFormation expression: {expression}")


def load_template(path: Path) -> dict[str, object]:
    boto3_module = sys.modules.get("boto3")
    if isinstance(boto3_module, ModuleType) and not hasattr(boto3_module, "Session"):
        del sys.modules["boto3"]

    from cfnlint.decode.cfn_yaml import load

    return load(path)


def rule_succeeds(rule: dict[str, object], parameters: dict[str, str]) -> bool:
    if not evaluate(rule["RuleCondition"], parameters):
        return True
    return all(evaluate(assertion["Assert"], parameters) for assertion in rule["Assertions"])


class OtlpPrivateLinkRuleTest(unittest.TestCase):
    def test_rejects_direct_otlp_with_privatelink(self) -> None:
        repository_root = Path(__file__).resolve().parents[2]

        for template_name in ("template.yaml", "template-govcloud.yaml"):
            with self.subTest(template=template_name):
                document = load_template(repository_root / template_name)
                rule = document["Rules"]["ValidateOtlpPrivateLink"]

                for parameters, expected_success in CASES:
                    with self.subTest(parameters=parameters):
                        self.assertEqual(rule_succeeds(rule, parameters), expected_success)


if __name__ == "__main__":
    unittest.main()
