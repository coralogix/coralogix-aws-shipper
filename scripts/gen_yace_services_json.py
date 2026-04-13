#!/usr/bin/env python3
"""Parse YACE pkg/config/services.go and emit JSON for Rust metrics tag enrichment."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path


def split_service_blocks(text: str) -> list[str]:
    marker = "var SupportedServices = serviceConfigs{"
    i = text.find(marker)
    if i < 0:
        raise SystemExit("marker not found")
    i = text.find("{", i)
    depth = 1
    i += 1
    items: list[str] = []
    svc_open: int | None = None
    while i < len(text):
        c = text[i]
        if c == "{":
            depth += 1
            if depth == 2:
                svc_open = i
        elif c == "}":
            depth -= 1
            if depth == 1 and svc_open is not None:
                items.append(text[svc_open + 1 : i])
                svc_open = None
            elif depth == 0:
                break
        i += 1
    return items


def parse_block(block: str) -> dict:
    m = re.search(r'Namespace:\s*"([^"]+)"', block)
    if not m:
        return {}
    namespace = m.group(1)
    filters = re.findall(r'aws\.String\("([^"]+)"\)', block)
    # Double-quoted MustCompile
    rx_dq = re.findall(
        r'regexp\.MustCompile\("((?:[^"\\]|\\.)*)"\)', block, flags=re.DOTALL
    )
    # Backtick MustCompile
    rx_bt = re.findall(r"regexp\.MustCompile\(`([^`]*)`\)", block, flags=re.DOTALL)

    def unescape_go(s: str) -> str:
        return (
            s.replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace('\\"', '"')
            .replace("\\\\", "\\")
        )

    regexes = [unescape_go(s) for s in rx_dq + rx_bt]
    return {
        "namespace": namespace,
        "resource_filters": filters,
        "dimension_regexes": regexes,
    }


def main() -> None:
    src = Path(__file__).with_name("yace_services.go.source")
    out = Path(__file__).resolve().parent.parent / "src" / "metrics" / "yace_services.json"
    text = src.read_text(encoding="utf-8")
    blocks = split_service_blocks(text)
    services = [b for b in (parse_block(bl) for bl in blocks) if b.get("namespace")]
    payload = {
        "_comment": "Generated from prometheus-community/yet-another-cloudwatch-exporter v0.55.0 pkg/config/services.go — run scripts/gen_yace_services_json.py after updating the source file.",
        "services": services,
    }
    out.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {len(services)} services to {out}", file=sys.stderr)


if __name__ == "__main__":
    main()
