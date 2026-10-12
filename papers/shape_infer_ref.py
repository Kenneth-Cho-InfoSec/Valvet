#!/usr/bin/env python3
"""B1 differential test: independent Python model of the merge rules in
papers/shape_infer.valvet (Types-from-Data, PLDI 2016, Sec. 3).

Runs the Valvet program, compares its stdout EXACTLY against this model's
output. All outputs are plain strings (no floats), so comparison is exact.
Exit 0 on match, 1 with a diff otherwise.
"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VALVET = os.path.join(ROOT, "target", "debug", "valvet")
PROG = os.path.join(ROOT, "papers", "shape_infer.valvet")

INT, FLOAT, STR, BOOL, NULL, REC, ARR, ANY = range(8)


def prim(t):
    return (t, None)


def record(fields):
    # fields: list of (name, shape, optional)
    return (REC, list(fields))


def arr(elem):
    return (ARR, elem)


def any_of(opts):
    return (ANY, list(opts))


def eq(a, b):
    if a[0] != b[0]:
        return False
    t = a[0]
    if t in (INT, FLOAT, STR, BOOL, NULL):
        return True
    if t == ARR:
        return eq(a[1], b[1])
    if t == ANY:
        return len(a[1]) == len(b[1]) and all(eq(x, y) for x, y in zip(a[1], b[1]))
    # REC: order-sensitive field compare (mirrors Valvet eq_records)
    fa, fb = a[1], b[1]
    return (
        len(fa) == len(fb)
        and all(
            na == nb and oa == ob and eq(sa, sb)
            for (na, sa, oa), (nb, sb, ob) in zip(fa, fb)
        )
    )


def merge(a, b):
    ta, tb = a[0], b[0]
    if ta == tb:
        if ta == REC:
            return merge_records(a[1], b[1])
        if ta == ARR:
            return arr(merge(a[1], b[1]))
        if ta == ANY:
            return merge_anys(a[1], b[1])
        return prim(ta)
    if {ta, tb} == {INT, FLOAT}:
        return prim(FLOAT)
    if ta == NULL:
        return b
    if tb == NULL:
        return a
    if ta == ANY:
        return any_add(a[1], b)
    if tb == ANY:
        return any_add_right(b[1], a)
    return any_of([a, b])


def merge_records(fa, fb):
    out = []
    for na, sa, oa in fa:
        hit = next(((nb, sb, ob) for nb, sb, ob in fb if nb == na), None)
        if hit is None:
            out.append((na, sa, True))
        else:
            _, sb, ob = hit
            out.append((na, merge(sa, sb), oa or ob))
    names_a = {n for n, _, _ in fa}
    for nb, sb, _ in fb:
        if nb not in names_a:
            out.append((nb, sb, True))
    return record(out)


def merge_anys(oa, ob):
    out = list(oa)
    for y in ob:
        if not any(eq(x, y) for x in out):
            out.append(y)
    return any_of(out)


def any_add(oa, b):
    if any(eq(x, b) for x in oa):
        return any_of(list(oa))
    return any_of(list(oa) + [b])


def any_add_right(ob, a):
    if any(eq(x, a) for x in ob):
        return any_of(list(ob))
    return any_of([a] + list(ob))


NAMES = {INT: "int", FLOAT: "float", STR: "string", BOOL: "bool", NULL: "null"}


def desc(s):
    t = s[0]
    if t in NAMES:
        return NAMES[t]
    if t == ARR:
        return "array(%s)" % desc(s[1])
    if t == ANY:
        return "any(%s)" % "|".join(desc(o) for o in s[1])
    parts = []
    for n, sub, o in s[1]:
        parts.append("%s%s=%s" % (n, "?" if o else "", desc(sub)))
    return "record(%s)" % ",".join(parts)


def main():
    string, integer, float_, null = prim(STR), prim(INT), prim(FLOAT), prim(NULL)
    doc1 = record([("name", string, False), ("age", integer, False)])
    doc2 = record([("name", string, False)])
    doc3 = record([("name", string, False), ("age", float_, False)])
    e1 = record([("id", integer, False)])
    e2 = record([("id", string, False)])
    q1 = record([("nick", null, False)])
    q2 = record([("nick", string, False)])

    expected = [
        desc(merge(doc1, doc2)),
        desc(merge(merge(doc1, doc2), doc3)),
        desc(merge(e1, e2)),
        desc(merge(arr(integer), arr(float_))),
        desc(merge(q1, q2)),
        desc(merge(integer, float_)),
        desc(merge(null, string)),
    ]

    env = dict(
        os.environ,
        LD_LIBRARY_PATH=os.path.expanduser("~/.local/vgcc/usr/lib")
        + ":"
        + os.environ.get("LD_LIBRARY_PATH", ""),
    )
    p = subprocess.run([VALVET, "run", PROG], capture_output=True, text=True, env=env)
    if p.returncode != 0:
        print("valvet run failed:\n%s" % p.stderr)
        return 1
    actual = p.stdout.splitlines()
    if actual == expected:
        print("B1 OK: %d lines match exactly" % len(expected))
        for line in actual:
            print("  " + line)
        return 0
    print("B1 MISMATCH")
    print("expected: %r" % expected)
    print("actual:   %r" % actual)
    return 1


if __name__ == "__main__":
    sys.exit(main())
