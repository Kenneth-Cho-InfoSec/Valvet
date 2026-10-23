#!/usr/bin/env python3
"""Compare Valvet port oracle output using the three-line label protocol.

Each record is:

    label
    =
    value

Numeric values are compared by binary64 bit pattern.  ``--one-ulp`` permits
one representable step between same-sign values.  Port artifacts are external
to this repository; ``--oracles-root`` supplies the manifest and files.
"""

from __future__ import annotations

import argparse
import json
import math
import struct
import sys
from pathlib import Path

SCHEMA = "valvet-oracle-compare-v1"


def parse_records(path: Path) -> tuple[dict[str, str], int]:
    if not path.exists():
        raise FileNotFoundError(path)
    lines = path.read_text().splitlines()
    records: dict[str, str] = {}
    malformed = 0
    i = 0
    while i < len(lines):
        while i < len(lines) and not lines[i].strip():
            i += 1
        if i >= len(lines):
            break
        if i + 2 >= len(lines) or lines[i + 1].strip() != "=":
            malformed += 1
            i += 3 if i + 2 < len(lines) else len(lines) - i
            continue
        label = lines[i].strip()
        value = lines[i + 2].strip()
        if not label or label == "=":
            malformed += 1
        elif label in records:
            malformed += 1
        else:
            records[label] = value
        i += 3
    return records, malformed


def numeric_bits(text: str) -> int | None:
    try:
        return struct.unpack("<Q", struct.pack("<d", float(text)))[0]
    except (ValueError, OverflowError):
        return None


def one_ulp(a: str, b: str) -> bool:
    ba, bb = numeric_bits(a), numeric_bits(b)
    if ba is None or bb is None or ba == bb:
        return False
    sign = 1 << 63
    if (ba & sign) != (bb & sign):
        return False
    return abs(ba - bb) == 1


def compare(expected: dict[str, str], actual: dict[str, str], allow_one_ulp: bool) -> dict[str, object]:
    exact = 0
    one = 0
    missing = 0
    extra = 0
    diffs: list[dict[str, str]] = []
    for label, want in expected.items():
        if label not in actual:
            missing += 1
            diffs.append({"label": label, "kind": "missing", "expected": want})
            continue
        got = actual[label]
        if got == want:
            exact += 1
        elif allow_one_ulp and one_ulp(want, got):
            one += 1
        else:
            diffs.append(
                {"label": label, "kind": "different", "expected": want, "actual": got}
            )
    for label in actual:
        if label not in expected:
            extra += 1
            diffs.append({"label": label, "kind": "extra", "actual": actual[label]})
    status = "ok" if missing == 0 and extra == 0 and not diffs else "fail"
    return {
        "schema": SCHEMA,
        "labels": len(expected),
        "exact": exact,
        "one_ulp": one,
        "missing": missing,
        "extra": extra,
        "differences": diffs,
        "status": status,
    }


def load_manifest(root: Path) -> dict[str, dict[str, str]]:
    manifest_path = root / "manifest.json"
    data = json.loads(manifest_path.read_text())
    ports = data.get("ports")
    if not isinstance(ports, dict):
        raise ValueError("manifest.json needs a `ports` object")
    return ports


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--actual", type=Path)
    ap.add_argument("--expected", type=Path)
    ap.add_argument("--port", default="")
    ap.add_argument("--json", type=Path, default=None)
    ap.add_argument("--one-ulp", action="store_true")
    ap.add_argument("--oracles-root", type=Path, default=None)
    ap.add_argument("--all", action="store_true")
    args = ap.parse_args(argv)

    results: dict[str, object] = {}
    failed = False
    if args.all:
        if args.oracles_root is None:
            print("error: --all needs --oracles-root", file=sys.stderr)
            return 2
        try:
            ports = load_manifest(args.oracles_root)
        except (OSError, ValueError, json.JSONDecodeError) as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2
        for port, paths in ports.items():
            expected_path = args.oracles_root / paths["expected"]
            actual_path = args.oracles_root / paths["actual"]
            expected, em = parse_records(expected_path)
            actual, am = parse_records(actual_path)
            result = compare(expected, actual, args.one_ulp)
            result["port"] = port
            result["malformed"] = em + am
            if result["malformed"]:
                result["status"] = "fail"
            results[port] = result
            failed |= result["status"] != "ok"
        payload: dict[str, object] = {"schema": SCHEMA, "ports": results}
    else:
        if args.expected is None or args.actual is None:
            print("error: need --expected and --actual (or --all)", file=sys.stderr)
            return 2
        try:
            expected, em = parse_records(args.expected)
            actual, am = parse_records(args.actual)
        except OSError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2
        result = compare(expected, actual, args.one_ulp)
        result["port"] = args.port
        result["malformed"] = em + am
        if result["malformed"]:
            result["status"] = "fail"
        failed = result["status"] != "ok"
        payload = result

    text = json.dumps(payload, indent=2, sort_keys=True) + "\n"
    print(text, end="")
    if args.json:
        args.json.write_text(text)
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
