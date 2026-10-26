"""Summarise release compilation units for ci/govcloud/verify-build-inputs.sh.

Reads `cargo check --release --message-format=json` output. Compilation units are
per-context and per-artifact, so unlike cargo metadata they describe what is actually
compiled into the release build: a feature enabled only by a dev or build dependency
does not appear on the shipped library's unit.

Three things this is careful about:

  - package ids are matched exactly, against ids taken from cargo metadata. Parsing a
    name out of the id fails for path dependencies, which are spelled
    `path+file:///...#0.103.13` with no name.
  - only `lib` artifacts whose filenames sit under the target triple are counted.
    Build scripts and proc macros are compiled for the host and are not shipped.
  - features are never unioned across artifacts sharing a package id. Two artifacts
    for one crate is a finding in itself, so the count is reported and the caller
    rejects anything other than one.
"""

import json
import sys


def main() -> None:
    unitpath, report, target = sys.argv[1], sys.argv[2], sys.argv[3]
    wanted = {
        name: info["id"]
        for name, info in json.loads(report).items()
        if info.get("id")
    }

    finished_ok = False
    artifacts: dict[str, list[list[str]]] = {}

    for line in open(unitpath):
        try:
            message = json.loads(line)
        except ValueError:
            continue

        if message.get("reason") == "build-finished":
            finished_ok = bool(message.get("success"))
            continue
        if message.get("reason") != "compiler-artifact":
            continue
        if "lib" not in (message.get("target", {}).get("kind") or []):
            continue
        # Host artifacts land outside the target triple directory.
        if not any(f"/{target}/" in f for f in message.get("filenames") or []):
            continue

        package_id = message["package_id"]
        if package_id in wanted.values():
            artifacts.setdefault(package_id, []).append(
                sorted(message.get("features", []))
            )

    crates = {}
    for name, package_id in wanted.items():
        found = artifacts.get(package_id, [])
        crates[name] = {
            "artifacts": len(found),
            # Only meaningful when exactly one artifact exists; the caller checks that
            # first, so an ambiguous result never reaches a feature assertion.
            "features": found[0] if len(found) == 1 else [],
        }

    print(json.dumps({"finished_ok": finished_ok, "crates": crates}))


if __name__ == "__main__":
    main()
