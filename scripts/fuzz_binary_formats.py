#!/usr/bin/env python3
"""Deterministic mutation smoke test for the narrow Parquet and Arrow readers.

This is not a coverage-guided fuzzer. It mutates every checked-in binary
fixture in a fixed order, runs one prebuilt Valvet probe against each case,
and fails on a signal, a timeout, or a non-zero exit. A clean `Err` and a
successful parse are both acceptable: the goal is to prove malformed bytes
never crash or hang the reader.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "valvet-tests" / "data"
BIN = Path(os.environ.get("VALVET_BIN", ROOT / "target" / "debug" / "valvet"))
CASE = Path("/tmp/opencode/valvet_fuzz_case.bin")
TIMEOUT = 5.0

PROBE = """import std::arrow::read_arrow;
import std::parquet::read_parquet;

fun main() {
    match (read_parquet(\"__CASE__\".clone())) { Ok(t) -> println(\"parquet ok\"), Err(e) -> println(\"parquet err\") };
    match (read_arrow(\"__CASE__\".clone())) { Ok(t) -> println(\"arrow ok\"), Err(e) -> println(\"arrow err\") };
}
"""


def fixtures() -> list[Path]:
    out = sorted(DATA.glob("parquet_*.parquet")) + sorted(DATA.glob("arrow_*.arrow"))
    if not out:
        raise SystemExit("no binary fixtures found under valvet-tests/data")
    return out


def mutations(data: bytes) -> list[tuple[str, bytes]]:
    cases: list[tuple[str, bytes]] = []
    if not data:
        return [("empty", b"")]
    positions = sorted({0, 1, len(data) // 2, max(0, len(data) - 1)})
    for pos in positions:
        for bit in (0x01, 0x80, 0xFF):
            b = bytearray(data)
            b[pos] ^= bit
            cases.append((f"xor-{pos}-{bit:02x}", bytes(b)))
    for cut in (1, 2, len(data) // 4, len(data) // 2, max(0, len(data) - 1)):
        cases.append((f"truncate-{cut}", data[:cut]))
    cases.append(("append-zero", data + b"\x00"))
    cases.append(("append-many", data + b"\xff" * 16))
    return cases


def build_probe(tmp: Path) -> Path:
    src = tmp / "fuzz_probe.valvet"
    src.write_text(PROBE.replace("__CASE__", str(CASE)))
    out = subprocess.run(
        [str(BIN), "build", str(src)],
        cwd=ROOT,
        capture_output=True,
        text=True,
        timeout=120,
    )
    if out.returncode != 0:
        raise SystemExit(f"probe build failed:\n{out.stdout}\n{out.stderr}")
    exe = ROOT / "target" / "valvet" / "fuzz_probe"
    if not exe.exists():
        raise SystemExit(f"probe executable missing: {exe}")
    return exe


def run_case(exe: Path, name: str, data: bytes) -> None:
    CASE.write_bytes(data)
    try:
        out = subprocess.run(
            [str(exe)],
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        raise SystemExit(f"{name}: timed out after {TIMEOUT}s")
    if out.returncode < 0:
        raise SystemExit(f"{name}: died from signal {-out.returncode}")
    if out.returncode != 0:
        raise SystemExit(f"{name}: exit {out.returncode}\n{out.stdout}\n{out.stderr}")
    text = out.stdout.strip().splitlines()
    if len(text) != 2 or not text[0].startswith("parquet ") or not text[1].startswith("arrow "):
        raise SystemExit(f"{name}: unexpected probe output: {out.stdout!r}")


def main() -> int:
    if not BIN.exists():
        raise SystemExit(f"missing compiler binary: {BIN}")
    CASE.parent.mkdir(parents=True, exist_ok=True)
    tmp = Path(tempfile.mkdtemp(prefix="valvet-fuzz-"))
    try:
        exe = build_probe(tmp)
        total = 0
        for path in fixtures():
            data = path.read_bytes()
            for label, mutated in mutations(data):
                run_case(exe, f"{path.name}:{label}", mutated)
                total += 1
        print(f"fuzz smoke ok: {total} cases")
        return 0
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
        CASE.unlink(missing_ok=True)


if __name__ == "__main__":
    sys.exit(main())
