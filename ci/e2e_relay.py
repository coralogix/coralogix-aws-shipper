#!/usr/bin/env python3
"""Trusted default-branch relay. PR archives are data and are never executed."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time
from zipfile import ZipFile


REPOSITORY = "coralogix/coralogix-aws-shipper"
BUILD = ".github/workflows/build-check.yaml"
LIFECYCLE = ".github/workflows/e2e-pr-events.yaml"
ARCHITECTURES = ("arm64", "x86-64")
MAX_ARCHIVE = 110 * 1024 * 1024


class DifferentAttempt(ValueError):
    """A retained artifact belongs to an earlier producer attempt."""


def positive(value):
    if type(value) is not int or value < 1:
        raise ValueError("invalid GitHub run/artifact identity")
    return str(value)


def validate_run(run):
    if (
        run.get("repository", {}).get("full_name") != REPOSITORY
        or run.get("event") != "pull_request"
        or run.get("path") not in {BUILD, LIFECYCLE}
        or not isinstance(run.get("head_sha"), str)
        or re.fullmatch(r"[0-9a-f]{40}", run["head_sha"]) is None
        or run.get("status") not in {"queued", "waiting", "pending", "in_progress", "completed", "requested"}
    ):
        raise ValueError("source is not an allowed PR workflow run")
    positive(run.get("id"))
    positive(run.get("run_attempt"))


def allowed_pr(number, *, enabled, allowed):
    if type(number) is not int or number < 1:
        return False
    return enabled or number in allowed


def matches_pr(run, pr):
    """An association can be empty, but source branch and current head must match."""
    return (
        isinstance(run.get("head_branch"), str)
        and run["head_branch"] == pr.get("head", {}).get("ref")
        and run["head_sha"] == pr.get("head", {}).get("sha")
        and (not run.get("pull_requests") or any(p.get("number") == pr["number"] for p in run["pull_requests"]))
    )


def rollout():
    enabled = os.environ.get("SHIPPER_E2E_PR_ENABLED", "false")
    if enabled not in {"true", "false"}:
        raise ValueError("SHIPPER_E2E_PR_ENABLED must be true or false")
    allowed = json.loads(os.environ.get("SHIPPER_E2E_PR_ALLOWED_PRS") or "[]")
    if not isinstance(allowed, list) or any(type(n) is not int or n < 1 for n in allowed):
        raise ValueError("SHIPPER_E2E_PR_ALLOWED_PRS must be a JSON array of PR numbers")
    return enabled == "true", allowed


def api(path):
    if not path.startswith(f"repos/{REPOSITORY}/"):
        raise ValueError("GitHub API path is outside the source repository")
    for attempt in range(3):
        # Only fixed source-repository paths, passed as arguments without a shell.
        result = subprocess.run(gh_command("api", path), capture_output=True, timeout=45)  # noqa: S603
        if result.returncode == 0:
            return json.loads(result.stdout)
        if attempt < 2:
            time.sleep(2**attempt)
    raise RuntimeError("GitHub API request failed; refusing to infer missing data")


def gh_command(*args):
    executable = shutil.which("gh")
    if executable is None:
        raise RuntimeError("GitHub CLI is required")
    return [executable, *args]


def pages(path, field=None):
    result = []
    for page in range(1, 101):
        response = api(f"{path}{'&' if '?' in path else '?'}per_page=100&page={page}")
        rows = response[field] if field else response
        if not isinstance(rows, list):
            raise ValueError("invalid GitHub list response")
        result.extend(rows)
        if len(rows) < 100:
            return result
    raise RuntimeError("GitHub pagination limit exceeded")


def validate_jobs(jobs):
    for arch in ("arm64", "amd64"):
        matching = [j for j in jobs if j.get("name") == f"fips build ({arch}, provided.al2023)"]
        if len(matching) != 1 or matching[0].get("status") != "completed" or matching[0].get("conclusion") != "success":
            raise ValueError("both architecture jobs must succeed in this exact build attempt")


def validate_transport(path, destination, arch, run):
    name = f"coralogix-aws-shipper-{arch}.zip"
    meta_name = name.replace(".zip", ".metadata.json")
    if path.stat().st_size > MAX_ARCHIVE:
        raise ValueError("GitHub artifact exceeds archive size limit")
    with ZipFile(path) as transport:
        members = transport.infolist()
        if len(members) != 2 or {m.filename for m in members} != {name, meta_name}:
            raise ValueError("transport must contain exactly the expected ZIP and metadata")
        for member in members:
            if ((member.external_attr >> 16) & 0o170000) == 0o120000:
                raise ValueError("artifact transport cannot contain symlinks")
            maximum = 100 * 1024 * 1024 if member.filename == name else 65536
            if not 0 < member.file_size <= maximum:
                raise ValueError("artifact transport member exceeds size limit")
        body, metadata_bytes = transport.read(name), transport.read(meta_name)
    metadata = json.loads(metadata_bytes)
    if isinstance(metadata, dict) and (metadata.get("github_run_id"), metadata.get("github_run_attempt")) != (
        positive(run["id"]),
        positive(run["run_attempt"]),
    ):
        raise DifferentAttempt("retained artifact belongs to another attempt")
    expected = dict(
        artifact_name=name,
        artifact_size=len(body),
        architecture=arch,
        source_sha=run["head_sha"],
        workflow_path=BUILD,
        github_run_id=positive(run["id"]),
        github_run_attempt=positive(run["run_attempt"]),
        artifact_digest="sha256:" + hashlib.sha256(body).hexdigest(),
        zip_root="bootstrap",
    )
    if not isinstance(metadata, dict) or any(metadata.get(k) != v for k, v in expected.items()):
        raise ValueError("bundle does not match the exact expected producer")
    native = "arm64" if arch == "arm64" else "amd64"
    if metadata.get("build_arch") not in {arch, native} or type(metadata.get("artifact_size")) is not int:
        raise ValueError("bundle architecture/size is invalid")
    destination.mkdir(parents=True, exist_ok=True)
    (destination / name).write_bytes(body)
    (destination / meta_name).write_bytes(metadata_bytes)
    with ZipFile(destination / name) as archive:
        entries = archive.infolist()
        if len(entries) != 1 or entries[0].filename != "bootstrap" or not 0 < entries[0].file_size <= 250 * 1024 * 1024:
            raise ValueError("deployment ZIP must contain only the root bootstrap")


def download_artifact(artifact, download):
    artifact_id = positive(artifact["id"])
    digest = artifact.get("digest", "")
    if (
        artifact.get("expired") is not False
        or not isinstance(digest, str)
        or re.fullmatch(r"sha256:[0-9a-f]{64}", digest) is None
        or type(artifact.get("size_in_bytes")) is not int
        or not 0 < artifact["size_in_bytes"] <= MAX_ARCHIVE
    ):
        raise ValueError("expired or invalid GitHub artifact")
    with download.open("wb") as stream:
        result = subprocess.run(  # noqa: S603 - fixed repository and validated numeric artifact ID
            gh_command("api", f"repos/{REPOSITORY}/actions/artifacts/{artifact_id}/zip"),
            stdout=stream,
            stderr=subprocess.PIPE,
            timeout=120,
        )
    if result.returncode:
        raise RuntimeError("GitHub artifact download failed")
    if download.stat().st_size > MAX_ARCHIVE or "sha256:" + hashlib.sha256(download.read_bytes()).hexdigest() != digest:
        raise ValueError("GitHub transport digest mismatch")


def select_bundle(artifacts, directory, arch, run):
    selected = []
    for artifact in artifacts:
        if artifact.get("name") != f"coralogix-aws-shipper-{arch}.zip" or artifact.get("expired") is True:
            continue
        artifact_id = positive(artifact["id"])
        download = directory / f"transport-{artifact_id}.zip"
        download_artifact(artifact, download)
        destination = directory / f"bundle-{artifact_id}"
        try:
            validate_transport(download, destination, arch, run)
        except DifferentAttempt:
            continue
        selected.append((artifact, destination))
    if len(selected) != 1:
        raise ValueError("missing or ambiguous architecture artifact for this attempt")
    artifact, destination = selected[0]
    verified = directory / "verified"
    verified.mkdir(exist_ok=True)
    for file in destination.iterdir():
        shutil.copyfile(file, verified / file.name)
    return dict(id=positive(artifact["id"]), digest=artifact["digest"])


def output(**values):
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a") as stream:
            for key, value in values.items():
                stream.write(f"{key}={str(value).lower() if isinstance(value, bool) else value}\n")


def prepare(directory):
    enabled, allowed = rollout()
    directory.mkdir(parents=True, exist_ok=True)
    if not enabled and not allowed:
        output(notify=False, needs_upload=False)
        return
    run_id, attempt = os.environ["SOURCE_RUN_ID"], os.environ["SOURCE_ATTEMPT"]
    for value in (run_id, attempt):
        if re.fullmatch(r"[1-9][0-9]*", value) is None:
            raise ValueError("invalid source workflow attempt")
    run = api(f"repos/{REPOSITORY}/actions/runs/{run_id}/attempts/{attempt}")
    validate_run(run)
    if (str(run["id"]), str(run["run_attempt"])) != (run_id, attempt):
        raise ValueError("GitHub returned a different source attempt")
    output(notify=True)
    objects = []
    if run["path"] == BUILD:
        # Some real PR runs have pull_requests: []; associations are only locators.
        candidates = pages(f"repos/{REPOSITORY}/commits/{run['head_sha']}/pulls")
        prs = []
        for item in candidates:
            number = item["number"]
            if not allowed_pr(number, enabled=enabled, allowed=allowed):
                continue
            pr = api(f"repos/{REPOSITORY}/pulls/{number}")
            if (
                pr.get("state") == "open"
                and pr.get("base", {}).get("ref") == "master"
                and matches_pr(run, pr)
                and (pr["head"].get("repo") or {}).get("full_name") == REPOSITORY
                and pr.get("user", {}).get("type") == "User"
                and not pr["user"]["login"].endswith("[bot]")
            ):
                prs.append(number)
        github_artifacts = {}
        verified = directory / "verified"
        success = run["status"] == "completed" and run.get("conclusion") == "success"
        if prs and success:
            jobs = pages(f"repos/{REPOSITORY}/actions/runs/{run_id}/attempts/{attempt}/jobs", "jobs")
            validate_jobs(jobs)
            artifacts = pages(f"repos/{REPOSITORY}/actions/runs/{run_id}/artifacts", "artifacts")
            for arch in ARCHITECTURES:
                github_artifacts[arch] = select_bundle(artifacts, directory, arch, run)
        for number in prs:
            prefix = f"rc/pr/{number}/{run['head_sha']}/{run_id}/{attempt}/"
            producer = dict(workflow_path=BUILD, run_id=run_id, run_attempt=attempt)
            event = dict(
                schema_version=1,
                source_repository=REPOSITORY,
                pull_request=number,
                source_sha=run["head_sha"],
                producer=producer,
                status=run["status"],
                conclusion=run.get("conclusion"),
                created_at=run["created_at"],
            )
            event_file = directory / f"event-{number}.json"
            event_file.write_text(json.dumps(event, sort_keys=True) + "\n")
            objects.append(
                dict(
                    key=prefix + f"events/{run['status']}-{run.get('conclusion') or 'none'}.json", path=str(event_file)
                )
            )
            if success:
                for path in sorted(verified.iterdir()):
                    objects.append(dict(key=prefix + path.name, path=str(path)))
                manifest = dict(
                    schema_version=1,
                    kind="pull_request",
                    source_repository=REPOSITORY,
                    pull_request=number,
                    source_sha=run["head_sha"],
                    source_ref=f"refs/pull/{number}/head",
                    producer=producer,
                    architectures=list(ARCHITECTURES),
                    github_artifacts=github_artifacts,
                )
                path = directory / f"manifest-{number}.json"
                path.write_text(json.dumps(manifest, sort_keys=True) + "\n")
                # The final object marks a complete bundle; readers still verify all bytes.
                objects.append(dict(key=prefix + "manifest.json", path=str(path)))
    (directory / "upload-plan.json").write_text(json.dumps(objects))
    output(needs_upload=bool(objects))


def upload(directory):
    import boto3
    from botocore.exceptions import ClientError

    s3 = boto3.client("s3")
    bucket = os.environ["SHIPPER_E2E_GATE_BUCKET"]
    objects = json.loads((directory / "upload-plan.json").read_text())
    for item in objects:
        if (
            re.fullmatch(
                r"rc/pr/[1-9][0-9]*/[0-9a-f]{40}/[1-9][0-9]*/[1-9][0-9]*/(?:events/[a-z_-]+-[a-z_-]+\.json|manifest\.json|coralogix-aws-shipper-(?:arm64|x86-64)\.(?:zip|metadata\.json))",
                item["key"],
            )
            is None
        ):
            raise ValueError("invalid PR staging key")
        path = Path(item["path"]).resolve()
        if not path.is_relative_to(directory.resolve()):
            raise ValueError("upload plan points outside the prepared directory")
        try:
            s3.put_object(
                Bucket=bucket,
                Key=item["key"],
                Body=path.read_bytes(),
                IfNoneMatch="*",
                ContentType="application/zip" if path.suffix == ".zip" else "application/json",
            )
        except ClientError as exc:
            if exc.response["Error"]["Code"] not in {"PreconditionFailed", "412"}:
                raise
            # A duplicate delivery cannot overwrite the bundle. The private resolver
            # pins existing object versions and validates every byte before execution.


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=["prepare", "upload"])
    parser.add_argument("--directory", type=Path, required=True)
    args = parser.parse_args()
    {"prepare": prepare, "upload": upload}[args.command](args.directory)
