import hashlib
import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from zipfile import ZipFile

PATH = Path(__file__).resolve().parents[2] / "ci/e2e_relay.py"
spec = importlib.util.spec_from_file_location("e2e_relay", PATH)
relay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(relay)

SHA = "a" * 40
REPO = "coralogix/coralogix-aws-shipper"


def run(**changes):
    return (
        dict(
            id=42,
            run_attempt=2,
            event="pull_request",
            path=".github/workflows/build-check.yaml",
            head_sha=SHA,
            repository={"full_name": REPO},
            status="completed",
            conclusion="success",
        )
        | changes
    )


def transport(arch="arm64", **changes):
    inner = io.BytesIO()
    with ZipFile(inner, "w") as z:
        z.writestr("bootstrap", b"binary")
    body = inner.getvalue()
    name = f"coralogix-aws-shipper-{arch}.zip"
    metadata = (
        dict(
            artifact_name=name,
            artifact_digest="sha256:" + hashlib.sha256(body).hexdigest(),
            artifact_size=len(body),
            architecture=arch,
            build_arch=arch,
            source_sha=SHA,
            workflow_path=".github/workflows/build-check.yaml",
            github_run_id="42",
            github_run_attempt="2",
            zip_root="bootstrap",
        )
        | changes
    )
    outer = io.BytesIO()
    with ZipFile(outer, "w") as z:
        z.writestr(name, body)
        z.writestr(name.replace(".zip", ".metadata.json"), json.dumps(metadata))
    return outer.getvalue()


class RelayContracts(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)

    def test_source_run_cannot_be_substituted(self):
        for changes in [
            {"event": "push"},
            {"repository": {"full_name": "other/repo"}},
            {"path": ".github/workflows/publish.yaml"},
            {"id": True},
            {"run_attempt": 0},
            {"head_sha": "../x"},
        ]:
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                relay.validate_run(run(**changes))

    def test_original_deployment_zip_is_preserved(self):
        archive = transport()
        path = self.directory / "download.zip"
        path.write_bytes(archive)
        relay.validate_transport(path, self.directory / "verified", "arm64", run())
        with ZipFile(io.BytesIO(archive)) as z:
            self.assertEqual(
                (self.directory / "verified/coralogix-aws-shipper-arm64.zip").read_bytes(),
                z.read("coralogix-aws-shipper-arm64.zip"),
            )

    def test_bundle_must_match_exact_producer_attempt(self):
        for changes in [
            {"github_run_id": "41"},
            {"github_run_attempt": "1"},
            {"source_sha": "b" * 40},
            {"architecture": "x86-64"},
            {"artifact_digest": "sha256:" + "0" * 64},
            {"artifact_size": 1},
        ]:
            path = self.directory / "download.zip"
            path.write_bytes(transport(**changes))
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                relay.validate_transport(path, self.directory / "verified", "arm64", run())

    def test_transport_cannot_write_code_or_escape(self):
        path = self.directory / "download.zip"
        with ZipFile(path, "w") as z:
            z.writestr("../e2e_relay.py", "not allowed")
        with self.assertRaises(ValueError):
            relay.validate_transport(path, self.directory / "verified", "arm64", run())
        self.assertFalse((self.directory / "e2e_relay.py").exists())

    def test_skipped_or_incomplete_matrix_is_not_success(self):
        jobs = [
            dict(name=f"fips build ({arch}, provided.al2023)", status="completed", conclusion="success")
            for arch in ["arm64", "amd64"]
        ]
        relay.validate_jobs(jobs)
        for bad in [jobs[:1], jobs + [jobs[0]], [jobs[0], jobs[1] | {"conclusion": "skipped"}]]:
            with self.subTest(jobs=bad), self.assertRaises(ValueError):
                relay.validate_jobs(bad)

    def test_rollout_defaults_to_disabled_and_canary_ids_are_exact(self):
        self.assertFalse(relay.allowed_pr(123, enabled=False, allowed=[]))
        self.assertTrue(relay.allowed_pr(123, enabled=False, allowed=[123]))
        self.assertFalse(relay.allowed_pr(124, enabled=False, allowed=[123]))
        self.assertTrue(relay.allowed_pr(124, enabled=True, allowed=[]))

    def test_rerun_selects_only_the_requested_attempt_among_retained_artifacts(self):
        artifacts = [
            dict(id=n, name="coralogix-aws-shipper-arm64.zip", expired=False, digest=f"digest-{n}") for n in [1, 2]
        ]

        def download(artifact, path):
            path.write_bytes(transport(github_run_attempt=str(artifact["id"])))

        with patch.object(relay, "download_artifact", side_effect=download):
            selected = relay.select_bundle(artifacts, self.directory, "arm64", run())
            self.assertEqual(selected["id"], "2")
            with self.assertRaisesRegex(ValueError, "this attempt"):
                relay.select_bundle(artifacts[:1], self.directory, "arm64", run())

    def test_two_artifacts_claiming_one_attempt_are_rejected(self):
        artifacts = [dict(id=n, name="coralogix-aws-shipper-arm64.zip", expired=False) for n in [1, 2]]
        with (
            patch.object(relay, "download_artifact", side_effect=lambda _a, p: p.write_bytes(transport())),
            self.assertRaisesRegex(ValueError, "ambiguous"),
        ):
            relay.select_bundle(artifacts, self.directory, "arm64", run())

    def test_wrong_head_with_current_attempt_is_not_treated_as_a_retained_artifact(self):
        artifact = dict(id=2, name="coralogix-aws-shipper-arm64.zip", expired=False)
        with (
            patch.object(
                relay, "download_artifact", side_effect=lambda _a, p: p.write_bytes(transport(source_sha="b" * 40))
            ),
            self.assertRaisesRegex(ValueError, "exact expected producer"),
        ):
            relay.select_bundle([artifact], self.directory, "arm64", run())

    def test_same_sha_associations_still_require_the_exact_pr_branch(self):
        source = run(head_branch="feature", pull_requests=[])
        pr = dict(number=123, head=dict(ref="feature", sha=SHA))
        self.assertTrue(relay.matches_pr(source, pr))
        self.assertFalse(relay.matches_pr(source, pr | {"head": dict(ref="other", sha=SHA)}))
        self.assertFalse(relay.matches_pr(source | {"pull_requests": [{"number": 124}]}, pr))

    def test_attempt_named_artifacts_avoid_upload_collisions_and_skip_other_attempts(self):
        artifacts = [
            dict(id=n, name=f"coralogix-aws-shipper-arm64-attempt-{n}.zip", expired=False, digest=f"digest-{n}")
            for n in [1, 2]
        ]
        with patch.object(relay, "download_artifact", side_effect=lambda _a, p: p.write_bytes(transport())) as download:
            selected = relay.select_bundle(artifacts, self.directory, "arm64", run())
            self.assertEqual(selected["id"], "2")
            self.assertEqual(download.call_count, 1)
