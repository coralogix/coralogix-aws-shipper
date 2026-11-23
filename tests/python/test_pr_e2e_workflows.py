from pathlib import Path

import yaml


ROOT = Path(__file__).resolve().parents[2]


def workflow(name):
    return yaml.safe_load((ROOT / ".github/workflows" / name).read_text())


def triggers(data):
    return data.get("on", data.get(True))


def test_every_target_pr_head_has_a_build_outcome():
    data = workflow("build-check.yaml")
    event = triggers(data)["pull_request"]
    assert event["branches"] == ["master"]
    assert "paths" not in event and "paths-ignore" not in event
    assert {"opened", "synchronize", "reopened", "unlabeled"} <= set(event["types"])
    assert data["concurrency"]["cancel-in-progress"] is True
    assert "github.event.pull_request.number" in data["concurrency"]["group"]
    assert "github.event.pull_request.head.sha" in yaml.safe_dump(data["jobs"])


def test_lifecycle_notifications_have_no_base_filter_or_privileged_credentials():
    data = workflow("e2e-pr-events.yaml")
    event = triggers(data)["pull_request"]
    assert {"opened", "synchronize", "reopened", "closed", "edited"} <= set(event["types"])
    assert not {"branches", "branches-ignore", "paths", "paths-ignore"} & event.keys()
    assert data["permissions"] == {"contents": "read"}
    jobs = yaml.safe_dump(data["jobs"])
    for forbidden in ["secrets.", "checkout@", "id-token", "CORALOGIX", "AWS_", "create-github-app-token"]:
        assert forbidden not in jobs
