#!/usr/bin/env python3
"""Exhaustive-style binary64 sqrt sweep for the Valvet std.math.sqrt wrapper.

The sweep samples positive finite bit patterns, starting with zero and the
subnormal range, and compares every printed result bit-for-bit with the
host's correctly rounded `math.sqrt` (which is IEEE 754 sqrt on CPython).
"""

from __future__ import annotations

import argparse
import json
import math
import os
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

MIN_NORMAL_BITS = 0x0010000000000000
SUBNORMAL_COUNT = MIN_NORMAL_BITS - 1
MAX_FINITE_BITS = 0x7FEFFFFFFFFFFFFF


def bits_to_float(bits: int) -> float:
    return struct.unpack("<d", struct.pack("<Q", bits))[0]


def float_to_bits(value: float) -> int:
    return struct.unpack("<Q", struct.pack("<d", value))[0]


def sample_bits(count: int) -> list[int]:
    """Return exactly `count` deterministic values, always including edges."""
    if count < 12:
        raise SystemExit("--count must be at least 12")
    edges = {
        0, 1, 2, 3,
        MIN_NORMAL_BITS - 3, MIN_NORMAL_BITS - 2, MIN_NORMAL_BITS - 1,
        MIN_NORMAL_BITS, MIN_NORMAL_BITS + 1, MIN_NORMAL_BITS + 2,
        0x3FF0000000000000, MAX_FINITE_BITS,
    }
    low = {b for b in edges if b < MIN_NORMAL_BITS}
    high = {b for b in edges if b >= MIN_NORMAL_BITS}
    sub_target = min(
        max(count * 3 // 4, len(low)), SUBNORMAL_COUNT, count - len(high)
    )
    i = 0
    while len(low) < sub_target:
        low.add(1 + (i * (SUBNORMAL_COUNT - 1)) // max(1, sub_target - 1))
        i += 1
    j = 0
    rest_span = max(1, count - len(low) - 1)
    while len(low) + len(high) < count:
        high.add(
            MIN_NORMAL_BITS
            + 1
            + (j * (MAX_FINITE_BITS - MIN_NORMAL_BITS - 1)) // rest_span
        )
        j += 1
    b = 1
    while len(low) + len(high) < count:
        high.add(b)
        b += 1
    return sorted(low | high)


def make_source(values: list[float], path: Path) -> None:
    lines = ["import std::math::sqrt;", "", "fun main() {"]
    for value in values:
        lines.append(f"    println(sqrt({value!r}));")
    lines.append("}")
    path.write_text("\n".join(lines) + "\n")


def run_batch(valvet: str, values: list[float], batch_dir: Path, index: int) -> list[float]:
    source = batch_dir / f"batch_{index}.valvet"
    make_source(values, source)
    proc = subprocess.run(
        [valvet, "run", str(source)],
        cwd=batch_dir,
        text=True,
        capture_output=True,
        timeout=300,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"valvet run failed for batch {index}:\n{proc.stdout}\n{proc.stderr}"
        )
    lines = proc.stdout.splitlines()
    if len(lines) != len(values):
        raise RuntimeError(
            f"batch {index}: expected {len(values)} output lines, got {len(lines)}"
        )
    return [float(line) for line in lines]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--count", type=int, default=1_000_000)
    ap.add_argument("--batch", type=int, default=10_000)
    ap.add_argument("--valvet", default=os.environ.get("VALVET", "target/debug/valvet"))
    ap.add_argument("--json", type=Path, default=None)
    ap.add_argument("--fail-fast", action="store_true")
    args = ap.parse_args()

    valvet = str(Path(args.valvet).resolve())
    if not Path(valvet).exists():
        raise SystemExit(f"valvet binary not found: {valvet}")
    bits = sample_bits(args.count)
    mismatches: list[dict[str, object]] = []
    checked = 0
    with tempfile.TemporaryDirectory(prefix="valvet-sqrt-") as td:
        batch_dir = Path(td)
        for start in range(0, len(bits), args.batch):
            chunk = bits[start:start + args.batch]
            values = [bits_to_float(b) for b in chunk]
            results = run_batch(valvet, values, batch_dir, start // args.batch)
            for b, value, got in zip(chunk, values, results):
                checked += 1
                expected = math.sqrt(value)
                if float_to_bits(got) != float_to_bits(expected):
                    item = {
                        "input_bits": f"0x{b:016x}",
                        "input": value.hex(),
                        "expected_bits": f"0x{float_to_bits(expected):016x}",
                        "actual_bits": f"0x{float_to_bits(got):016x}",
                    }
                    mismatches.append(item)
                    if args.fail_fast:
                        break
            if args.fail_fast and mismatches:
                break
    summary = {
        "schema": "valvet-sqrt-sweep-v1",
        "requested": args.count,
        "checked": checked,
        "mismatches": len(mismatches),
        "status": "ok" if not mismatches else "mismatch",
        "first": mismatches[:10],
    }
    print(json.dumps(summary, indent=2))
    if args.json:
        args.json.write_text(json.dumps(summary, indent=2) + "\n")
    return 0 if not mismatches else 1


if __name__ == "__main__":
    raise SystemExit(main())
