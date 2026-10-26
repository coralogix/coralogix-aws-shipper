"""Summarise the runtime dependency graph for ci/govcloud/verify-build-inputs.sh.

Reads `cargo metadata --locked --filter-platform <target>` output and walks only
normal edges from the workspace root, so build and dev dependencies cannot satisfy
an assertion about what ships.

Reports each crate's exact package id. Callers match on that rather than parsing a
name out of the id string: path dependencies are spelled `path+file:///...#0.103.13`
with no name at all, which silently reads as a different crate.

Feature sets are deliberately NOT reported here. cargo metadata unifies features
across normal, build and dev contexts, so they do not describe what is compiled into
the artifact -- see _unit_report.py.
"""

import json
import sys

WANTED = ("rustls-webpki", "aws-lc-rs", "aws-lc-fips-sys", "rustls")


def main() -> None:
    here, metapath = sys.argv[1], sys.argv[2]
    data = json.load(open(metapath))

    packages = {p["id"]: p for p in data["packages"]}
    nodes = {n["id"]: n for n in data["resolve"]["nodes"]}
    root = data["resolve"]["root"]

    reachable, stack = set(), [root]
    while stack:
        node_id = stack.pop()
        if node_id in reachable or node_id not in nodes:
            continue
        reachable.add(node_id)
        for dep in nodes[node_id]["deps"]:
            # kind None is a normal dependency; "build" and "dev" are not shipped.
            if any(k.get("kind") is None for k in dep.get("dep_kinds", [])):
                stack.append(dep["pkg"])

    out = {}
    for name in WANTED:
        ids = [i for i in reachable if packages[i]["name"] == name]
        single = packages[ids[0]] if len(ids) == 1 else None
        out[name] = {
            "count": len(ids),
            "id": ids[0] if len(ids) == 1 else None,
            "version": single["version"] if single else None,
            "source": (single.get("source") or "local") if single else None,
            "manifest": single["manifest_path"] if single else None,
        }
    print(json.dumps(out))


if __name__ == "__main__":
    main()
