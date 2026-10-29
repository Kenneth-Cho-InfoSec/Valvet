#!/usr/bin/env python3
"""Generate deterministic Parquet fixtures for the narrow P5A reader.

The writer here is intentionally small: one row group, one DATA_PAGE v1 per
column, PLAIN values, optional definition levels, and optional literal-only
Snappy compression. It uses only the Python standard library.
"""

from __future__ import annotations

import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "valvet-tests" / "data"


def varint(v: int) -> bytes:
    out = bytearray()
    while True:
        b = v & 0x7F
        v >>= 7
        if v:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def zigzag(v: int) -> int:
    return (v << 1) ^ (v >> 63)


class Thrift:
    def __init__(self) -> None:
        self.b = bytearray()
        self.last = 0
        self.stack: list[int] = []

    def u8(self, v: int) -> None:
        self.b.append(v & 0xFF)

    def field(self, fid: int, typ: int) -> None:
        delta = fid - self.last
        if 1 <= delta <= 15:
            self.u8((delta << 4) | typ)
        else:
            self.u8(typ)
            self.b += varint(zigzag(fid))
        self.last = fid

    def i32(self, fid: int, v: int) -> None:
        self.field(fid, 5)
        self.b += varint(zigzag(v))

    def i32_value(self, v: int) -> None:
        self.b += varint(zigzag(v))

    def i64(self, fid: int, v: int) -> None:
        self.field(fid, 6)
        self.b += varint(zigzag(v))

    def binary(self, fid: int, v: bytes) -> None:
        self.field(fid, 8)
        self.b += varint(len(v))
        self.b += v

    def binary_value(self, v: bytes) -> None:
        self.b += varint(len(v))
        self.b += v

    def list_header(self, fid: int, count: int, kind: int) -> None:
        self.field(fid, 9)
        if count < 15:
            self.u8((count << 4) | kind)
        else:
            self.u8(0xF0 | kind)
            self.b += varint(count)

    def struct_field(self, fid: int) -> None:
        self.field(fid, 12)
        self.stack.append(self.last)
        self.last = 0

    def struct_element(self) -> None:
        self.stack.append(self.last)
        self.last = 0

    def struct_end(self) -> None:
        self.u8(0)
        self.last = self.stack.pop()


def snappy_literal(data: bytes) -> bytes:
    out = bytearray(varint(len(data)))
    if not data:
        return bytes(out)
    x = len(data) - 1
    if x < 60:
        out.append(x << 2)
    else:
        nbytes = (x.bit_length() + 7) // 8
        out.append((59 + nbytes) << 2)
        out += x.to_bytes(nbytes, "little")
    out += data
    return bytes(out)


def def_levels(values: list[object]) -> bytes:
    n = len(values)
    groups = (n + 7) // 8
    enc = bytearray(varint(groups * 2 + 1))
    for g in range(groups):
        b = 0
        for j in range(8):
            row = g * 8 + j
            if row < n and values[row] is not None:
                b |= 1 << j
        enc.append(b)
    return struct.pack("<I", len(enc)) + bytes(enc)


def page_body(col: dict, n: int) -> bytes:
    kind = col["kind"]
    values = col["values"]
    optional = any(v is None for v in values)
    out = bytearray()
    if optional:
        out += def_levels(values)
    if kind == 2:
        for v in values:
            if v is not None:
                out += struct.pack("<q", v)
    elif kind == 5:
        for v in values:
            if v is not None:
                out += struct.pack("<d", v)
    elif kind == 0:
        bits = bytearray()
        acc = 0
        k = 0
        for v in values:
            if v is not None:
                if v:
                    acc |= 1 << (k % 8)
                k += 1
                if k % 8 == 0:
                    bits.append(acc)
                    acc = 0
        if k % 8:
            bits.append(acc)
        out += bits
    elif kind == 6:
        for v in values:
            if v is not None:
                raw = v.encode()
                out += struct.pack("<I", len(raw)) + raw
    else:
        raise ValueError(kind)
    return bytes(out)


def page(col: dict, n: int, codec: int) -> bytes:
    body = page_body(col, n)
    stored = snappy_literal(body) if codec == 1 else body
    w = Thrift()
    w.i32(1, 0)
    w.i32(2, len(body))
    w.i32(3, len(stored))
    w.struct_field(5)
    w.i32(1, n)
    w.i32(2, 0)
    w.i32(3, 3)
    w.i32(4, 3)
    w.struct_end()
    w.u8(0)
    return bytes(w.b) + stored


def schema_metadata(cols: list[dict], nested: bool) -> bytes:
    w = Thrift()
    w.i32(1, 1)
    count = len(cols) + (2 if nested else 1)
    w.list_header(2, count, 12)
    w.struct_element()
    w.binary(4, b"schema")
    w.i32(5, 2 if nested else len(cols))
    w.struct_end()
    for col in cols:
        w.struct_element()
        w.i32(1, col["kind"])
        w.i32(3, 1 if any(v is None for v in col["values"]) else 0)
        w.binary(4, col["name"].encode())
        if col["kind"] == 6:
            w.i32(6, 0)
        w.struct_end()
    if nested:
        w.struct_element()
        w.binary(4, b"nested")
        w.i32(5, 1)
        w.struct_end()
        w.struct_element()
        w.i32(1, 2)
        w.i32(3, 1)
        w.binary(4, b"child")
        w.struct_end()
    return bytes(w.b)


def metadata(cols: list[dict], n: int, chunks: list[tuple[int, int]], codec: int, nested: bool, bad_offset: bool) -> bytes:
    w = Thrift()
    w.b += schema_metadata(cols, nested)
    w.last = 2
    w.i64(3, n)
    w.list_header(4, 1, 12)
    w.struct_element()
    w.list_header(1, len(cols), 12)
    total = 0
    for col, (off, size) in zip(cols, chunks):
        w.struct_element()
        w.i64(2, off)
        w.struct_field(3)
        w.i32(1, col["kind"])
        w.list_header(2, 1, 5)
        w.i32_value(0)
        w.list_header(3, 1, 8)
        w.binary_value(col["name"].encode())
        w.i32(4, codec)
        w.i64(5, n)
        w.i64(6, size)
        w.i64(7, size)
        w.i64(9, 999999 if bad_offset else off)
        w.struct_end()
        w.struct_end()
        total += size
    w.i64(2, total)
    w.i64(3, n)
    w.struct_end()
    w.binary(6, b"valvet")
    w.u8(0)
    return bytes(w.b)


def make(path: Path, cols: list[dict], n: int, codec: int = 0, nested: bool = False, bad_offset: bool = False) -> bytes:
    out = bytearray(b"PAR1")
    chunks: list[tuple[int, int]] = []
    for col in cols:
        off = len(out)
        pg = page(col, n, codec)
        out += pg
        chunks.append((off, len(pg)))
    md = metadata(cols, n, chunks, codec, nested, bad_offset)
    out += md
    out += struct.pack("<I", len(md))
    out += b"PAR1"
    path.write_bytes(out)
    return bytes(out)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    basic = [
        {"name": "id", "kind": 2, "values": [1, 2, 3, 4]},
        {"name": "score", "kind": 5, "values": [1.5, 2.5, 3.5, 4.5]},
        {"name": "flag", "kind": 0, "values": [True, False, True, False]},
        {"name": "name", "kind": 6, "values": ["a", "b", "c", "d"]},
    ]
    missing = [
        {"name": "id", "kind": 2, "values": [1, None, 3, 4]},
        {"name": "score", "kind": 5, "values": [1.5, None, 3.5, 4.5]},
        {"name": "flag", "kind": 0, "values": [True, False, None, True]},
        {"name": "name", "kind": 6, "values": ["a", None, "c", "d"]},
    ]
    make(OUT / "parquet_basic.parquet", basic, 4)
    make(OUT / "parquet_missing.parquet", missing, 4)
    make(OUT / "parquet_snappy.parquet", basic, 4, codec=1)
    make(OUT / "parquet_nested.parquet", basic[:1], 4, nested=True)
    make(OUT / "parquet_bad_offset.parquet", basic, 4, bad_offset=True)

    data = (OUT / "parquet_basic.parquet").read_bytes()
    dict_page = bytearray(data)
    # First page starts at offset 4. Its first PageHeader field is type 0;
    # change the zigzag value to 2 while keeping the header length.
    assert dict_page[4:6] == b"\x15\x00"
    dict_page[5] = 0x04
    (OUT / "parquet_dictionary.parquet").write_bytes(dict_page)
    (OUT / "parquet_truncated.parquet").write_bytes(data[: len(data) // 2])


if __name__ == "__main__":
    main()
