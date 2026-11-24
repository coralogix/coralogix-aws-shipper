from pathlib import Path
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[2]


def workflow(name):
    return yaml.safe_load((ROOT / ".github/workflows" / name).read_text())


def triggers(data):
    return data.get("on", data.get(True))


class WorkflowContracts(unittest.TestCase):
    def test_every_target_pr_head_has_a_build_outcome(self):
        data = workflow("build-check.yaml")
        event = triggers(data)["pull_request"]
        assert event["branches"] == ["master"]
        assert "paths" not in event and "paths-ignore" not in event
        assert {"opened", "synchronize", "reopened", "unlabeled", "edited"} <= set(event["types"])
        assert data["concurrency"]["cancel-in-progress"] is True
        assert "github.event.pull_request.number" in data["concurrency"]["group"]
        assert "github.event.pull_request.head.sha" in yaml.safe_dump(data["jobs"])
        upload = next(
            s for s in data["jobs"]["fips-build"]["steps"] if s.get("uses", "").startswith("actions/upload-artifact@")
        )
        assert "github.run_attempt" in upload["with"]["name"]

    def test_lifecycle_notifications_have_no_base_filter_or_privileged_credentials(self):
        data = workflow("e2e-pr-events.yaml")
        event = triggers(data)["pull_request"]
        assert {"opened", "synchronize", "reopened", "closed", "edited", "labeled", "unlabeled"} <= set(event["types"])
        assert not {"branches", "branches-ignore", "paths", "paths-ignore"} & event.keys()
        assert data["permissions"] == {"contents": "read"}
        jobs = yaml.safe_dump(data["jobs"])
        for forbidden in ["secrets.", "checkout@", "id-token", "CORALOGIX", "AWS_", "create-github-app-token"]:
            assert forbidden not in jobs

    def test_trusted_relay_handles_reruns_and_keeps_app_credentials_away_from_archives(self):
        data = workflow("e2e-relay.yaml")
        event = triggers(data)["workflow_run"]
        assert set(event["types"]) == {"requested", "in_progress", "completed"}
        assert set(event["workflows"]) == {"build check", "AWS Shipper E2E PR Events"}
        stage, notify = data["jobs"]["stage"], data["jobs"]["notify"]
        assert "refs/heads/master" in stage["if"]
        assert "SHIPPER_E2E_PR_ENABLED" in stage["if"]
        stage_text = yaml.safe_dump(stage)
        assert "AUTOMATION_PRIVATE_KEY" not in stage_text
        assert "CORALOGIX" not in stage_text
        assert "workflow_run.head_sha" not in stage_text
        notify_text = yaml.safe_dump(notify)
        assert "permission-contents: write" in notify_text
        assert "repository_dispatch" not in stage_text
        assert "configure-aws-credentials" not in notify_text
        assert "checkout@" not in notify_text
