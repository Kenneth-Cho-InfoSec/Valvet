#!/usr/bin/env python3
"""Generate deterministic Arrow IPC file fixtures for the narrow P5A reader.

One record batch, Int64/Float64/Bool/Utf8, no compression or dictionaries.
Uses only the Python standard library.
"""

from __future__ import annotations

import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "valvet-tests" / "data"


class FB:
    def __init__(self) -> None:
        self.b = bytearray()

    def u8(self, v: int) -> None:
        self.b.append(v & 0xFF)

    def u16(self, v: int) -> None:
        self.b += struct.pack("<H", v & 0xFFFF)

    def u32(self, v: int) -> None:
        self.b += struct.pack("<I", v & 0xFFFFFFFF)

    def i32(self, v: int) -> None:
        self.b += struct.pack("<i", v)

    def i64(self, v: int) -> None:
        self.b += struct.pack("<q", v)

    def align(self, n: int) -> None:
        while len(self.b) % n:
            self.b.append(0)

    def patch_u32(self, at: int, v: int) -> None:
        self.b[at : at + 4] = struct.pack("<I", v & 0xFFFFFFFF)

    def set_u16(self, at: int, v: int) -> None:
        self.b[at : at + 2] = struct.pack("<H", v & 0xFFFF)

    def set_i32(self, at: int, v: int) -> None:
        self.b[at : at + 4] = struct.pack("<i", v)

    def set_u8(self, at: int, v: int) -> None:
        self.b[at] = v & 0xFF

    def table(self, maxid: int, body: int, align: int) -> tuple[int, int]:
        self.align(2)
        vt = len(self.b)
        vtsize = 4 + (maxid + 1) * 2
        self.b += b"\x00" * vtsize
        self.align(align)
        pos = len(self.b)
        self.b += b"\x00" * body
        self.set_u16(vt, vtsize)
        self.set_u16(vt + 2, body)
        self.set_i32(pos, pos - vt)
        return vt, pos

    def set_field(self, vt: int, fid: int, off: int) -> None:
        self.set_u16(vt + 4 + fid * 2, off)

    def patch_uoffset(self, at: int, target: int) -> None:
        self.patch_u32(at, target - at)

    def string(self, s: str) -> int:
        self.align(4)
        pos = len(self.b)
        raw = s.encode()
        self.u32(len(raw))
        self.b += raw
        self.u8(0)
        return pos


def type_table(fb: FB, kind: str) -> int:
    if kind == "int64":
        vt, pos = fb.table(1, 12, 4)
        fb.set_field(vt, 0, 4)
        fb.set_field(vt, 1, 8)
        fb.set_i32(pos + 4, 64)
        fb.set_u8(pos + 8, 1)
        return pos
    if kind == "int16":
        vt, pos = fb.table(1, 12, 4)
        fb.set_field(vt, 0, 4)
        fb.set_field(vt, 1, 8)
        fb.set_i32(pos + 4, 16)
        fb.set_u8(pos + 8, 1)
        return pos
    if kind == "float64":
        vt, pos = fb.table(0, 8, 4)
        fb.set_field(vt, 0, 4)
        fb.set_u8(pos + 4, 2)
        return pos
    vt, pos = fb.table(-1, 4, 4)
    return pos


def field_table(fb: FB, name: str, kind: str, nullable: bool, children: list[tuple[str, str, bool]] | None = None, dictionary: bool = False) -> int:
    vt, pos = fb.table(5, 24, 4)
    fb.set_field(vt, 0, 8)
    fb.set_field(vt, 1, 4)
    fb.set_field(vt, 2, 5)
    fb.set_field(vt, 3, 12)
    fb.set_u8(pos + 4, 1 if nullable else 0)
    tt = {"int64": 2, "int16": 2, "float64": 3, "bool": 6, "utf8": 5}[kind]
    fb.set_u8(pos + 5, tt)
    name_pos = fb.string(name)
    fb.patch_uoffset(pos + 8, name_pos)
    tp = type_table(fb, kind)
    fb.patch_uoffset(pos + 12, tp)
    if dictionary:
        dvt, dpos = fb.table(-1, 4, 4)
        fb.set_field(vt, 4, 16)
        fb.patch_uoffset(pos + 16, dpos)
    if children:
        fb.align(4)
        vpos = len(fb.b)
        fb.u32(len(children))
        for _ in children:
            fb.u32(0)
        for i, (cname, ckind, cnull) in enumerate(children):
            cp = field_table(fb, cname, ckind, cnull)
            fb.patch_uoffset(vpos + 4 + i * 4, cp)
        fb.set_field(vt, 5, 20)
        fb.patch_uoffset(pos + 20, vpos)
    return pos


def schema_table(fb: FB, fields: list[tuple[str, str, bool]], nested: bool = False, dictionary: bool = False) -> int:
    vt, pos = fb.table(1, 8, 4)
    fb.set_field(vt, 1, 4)
    fb.align(4)
    vpos = len(fb.b)
    fb.u32(len(fields))
    for _ in fields:
        fb.u32(0)
    for i, (name, kind, nullable) in enumerate(fields):
        children = [("child", "int64", True)] if nested and i == 0 else None
        fp = field_table(fb, name, kind, nullable, children, dictionary and i == 0)
        fb.patch_uoffset(vpos + 4 + i * 4, fp)
    fb.patch_uoffset(pos + 4, vpos)
    return pos


def record_batch(fb: FB, n: int, nodes: list[tuple[int, int]], buffers: list[tuple[int, int]]) -> int:
    vt, pos = fb.table(2, 24, 8)
    fb.set_field(vt, 0, 8)
    fb.set_field(vt, 1, 16)
    fb.set_field(vt, 2, 20)
    fb.i64(0)
    fb.b[pos + 8 : pos + 16] = struct.pack("<q", n)
    fb.align(8)
    npos = len(fb.b)
    fb.u32(len(nodes))
    for length, nulls in nodes:
        fb.i64(length)
        fb.i64(nulls)
    fb.patch_uoffset(pos + 16, npos)
    fb.align(8)
    bpos = len(fb.b)
    fb.u32(len(buffers))
    for off, length in buffers:
        fb.i64(off)
        fb.i64(length)
    fb.patch_uoffset(pos + 20, bpos)
    return pos


def message(fb: FB, body_len: int, n: int, nodes: list[tuple[int, int]], buffers: list[tuple[int, int]]) -> bytes:
    out = FB()
    out.u32(0)
    vt, pos = out.table(3, 24, 8)
    out.set_field(vt, 1, 4)
    out.set_field(vt, 2, 16)
    out.set_field(vt, 3, 8)
    out.set_u8(pos + 4, 3)
    out.i64(0)
    out.b[pos + 8 : pos + 16] = struct.pack("<q", body_len)
    rb = record_batch(out, n, nodes, buffers)
    out.patch_uoffset(pos + 16, rb)
    out.patch_u32(0, pos)
    return bytes(out.b)


def footer(fb: FB, fields: list[tuple[str, str, bool]], block: tuple[int, int, int], nested: bool = False, dictionary: bool = False, block_count: int = 1) -> bytes:
    out = FB()
    out.u32(0)
    vt, pos = out.table(3, 12, 4)
    out.set_field(vt, 1, 4)
    out.set_field(vt, 3, 8)
    sp = schema_table(out, fields, nested, dictionary)
    out.patch_uoffset(pos + 4, sp)
    out.align(8)
    bpos = len(out.b)
    out.u32(block_count)
    off, meta_len, body_len = block
    for _ in range(block_count):
        out.i64(off)
        out.i32(meta_len)
        out.u32(0)
        out.i64(body_len)
    out.patch_uoffset(pos + 8, bpos)
    out.patch_u32(0, pos)
    return bytes(out.b)


def body_for(fields: list[tuple[str, str, bool]], values: dict[str, list[object]]) -> tuple[bytes, list[tuple[int, int]], list[tuple[int, int]]]:
    body = bytearray()
    nodes: list[tuple[int, int]] = []
    buffers: list[tuple[int, int]] = []
    n = len(next(iter(values.values())))
    for name, kind, _nullable in fields:
        vals = values[name]
        nulls = sum(1 for v in vals if v is None)
        nodes.append((n, nulls))
        if nulls:
            while len(body) % 8:
                body.append(0)
            off = len(body)
            nbytes = (n + 7) // 8
            for g in range(nbytes):
                b = 0
                for j in range(8):
                    row = g * 8 + j
                    if row < n and vals[row] is not None:
                        b |= 1 << j
                body.append(b)
            buffers.append((off, nbytes))
        else:
            buffers.append((0, 0))
        if kind in ("int64", "int16"):
            while len(body) % 8:
                body.append(0)
            off = len(body)
            for v in vals:
                body += struct.pack("<q", 0 if v is None else v)
            buffers.append((off, n * 8))
        elif kind == "float64":
            while len(body) % 8:
                body.append(0)
            off = len(body)
            for v in vals:
                body += struct.pack("<d", 0.0 if v is None else v)
            buffers.append((off, n * 8))
        elif kind == "bool":
            while len(body) % 8:
                body.append(0)
            off = len(body)
            nbytes = (n + 7) // 8
            for g in range(nbytes):
                b = 0
                for j in range(8):
                    row = g * 8 + j
                    if row < n and vals[row]:
                        b |= 1 << j
                body.append(b)
            buffers.append((off, nbytes))
        elif kind == "utf8":
            while len(body) % 8:
                body.append(0)
            off = len(body)
            cur = 0
            for v in vals:
                body += struct.pack("<I", cur)
                if v is not None:
                    cur += len(v.encode())
            body += struct.pack("<I", cur)
            buffers.append((off, (n + 1) * 4))
            while len(body) % 8:
                body.append(0)
            off2 = len(body)
            for v in vals:
                if v is not None:
                    body += v.encode()
            buffers.append((off2, cur))
    while len(body) % 8:
        body.append(0)
    return bytes(body), nodes, buffers


def make(path: Path, fields: list[tuple[str, str, bool]], values: dict[str, list[object]], nested: bool = False, dictionary: bool = False, block_count: int = 1) -> bytes:
    n = len(next(iter(values.values())))
    body, nodes, buffers = body_for(fields, values)
    meta = message(FB(), len(body), n, nodes, buffers)
    out = bytearray(b"ARROW1\x00\x00")
    block_off = len(out)
    out += struct.pack("<I", 0xFFFFFFFF)
    out += struct.pack("<I", len(meta))
    out += meta
    while len(out) % 8:
        out.append(0)
    block_meta_len = len(out) - block_off
    out += body
    ft = footer(FB(), fields, (block_off, block_meta_len, len(body)), nested, dictionary, block_count)
    out += ft
    out += struct.pack("<I", len(ft))
    out += b"ARROW1"
    path.write_bytes(out)
    return bytes(out)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    basic_fields = [("id", "int64", False), ("score", "float64", True), ("flag", "bool", True), ("name", "utf8", True)]
    basic = {"id": [1, 2, 3, 4], "score": [1.5, 2.5, 3.5, 4.5], "flag": [True, False, True, False], "name": ["a", "b", "c", "d"]}
    missing = {"id": [1, 2, 3, 4], "score": [1.5, None, 3.5, 4.5], "flag": [True, False, None, True], "name": ["a", None, "c", "d"]}
    make(OUT / "arrow_basic.arrow", basic_fields, basic)
    make(OUT / "arrow_missing.arrow", basic_fields, missing)
    make(OUT / "arrow_unsupported.arrow", [("id", "int16", False)], {"id": [1, 2, 3, 4]})
    make(OUT / "arrow_nested.arrow", basic_fields, basic, nested=True)
    make(OUT / "arrow_dictionary.arrow", basic_fields, basic, dictionary=True)
    data = (OUT / "arrow_basic.arrow").read_bytes()
    (OUT / "arrow_truncated.arrow").write_bytes(data[: len(data) // 2])
    make(OUT / "arrow_multiple_batches.arrow", basic_fields, basic, block_count=2)


if __name__ == "__main__":
    main()
