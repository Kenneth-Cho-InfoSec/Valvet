#!/usr/bin/env python3
"""Differential test for papers/p2_quantile.valvet (Hyndman & Fan 1996;
F# source papers/p2_quantile.fs).

Implements the same 3 definitions directly over the same fixed dataset:
  kind 1 = H&F Type 1 = F# OfSorted.empiricalInvCdf: s[ceil(n*q)-1]
  kind 2 = H&F Type 2 = F# OfSorted.empiricalInvCdfAverage:
           (s[ceil(n*q)-1] + s[floor(n*q)]) / 2
  kind 7 = H&F Type 7 = F# OfSorted.mode (R default):
           h = (n-1)*q+1, s[h'-1] + (h-h')*(s[h']-s[h'-1])
with q=0 -> min, q=1 -> max. Compares with rel tolerance 1e-12.
Exit 0 on match, 1 otherwise.
"""
import math
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VALVET = os.path.join(ROOT, "target", "debug", "valvet")
PROG = os.path.join(ROOT, "papers", "p2_quantile.valvet")
TOL = 1e-12

DATA = [12.5, 3.1, 7.8, 19.2, 5.5, 8.4, 1.2, 15.6, 9.9, 4.4, 11.1, 6.7,
        13.3, 2.8, 17.5, 10.0, 14.2, 0.5, 18.8, 16.1, 7.2, 9.1, 3.9, 20.4]
PS = [0.0, 0.25, 0.5, 0.75, 1.0]
KINDS = [1, 2, 7]


def qtype1(s, q):
    n = len(s)
    if q <= 0.0:
        return min(s)
    if q >= 1.0:
        return max(s)
    return s[math.ceil(n * q) - 1]


def qtype2(s, q):
    n = len(s)
    if q <= 0.0:
        return min(s)
    if q >= 1.0:
        return max(s)
    a = s[math.ceil(n * q) - 1]
    b = s[math.floor(n * q)]
    return (a + b) / 2.0


def qtype7(s, q):
    n = len(s)
    if q <= 0.0:
        return min(s)
    if q >= 1.0:
        return max(s)
    h = (n - 1) * q + 1.0
    hp = math.floor(h)
    a = s[hp - 1]
    b = s[hp]
    return a + (h - hp) * (b - a)


def main():
    s = sorted(DATA)
    expected = []
    for k in KINDS:
        f = {1: qtype1, 2: qtype2, 7: qtype7}[k]
        for p in PS:
            expected.append(f(s, p))
    env = dict(os.environ)
    env["LD_LIBRARY_PATH"] = (os.path.expanduser("~/.local/vgcc/usr/lib") +
                              os.pathsep + env.get("LD_LIBRARY_PATH", ""))
    r = subprocess.run([VALVET, "run", PROG], capture_output=True, text=True,
                       env=env)
    if r.returncode != 0:
        print("valvet failed:\n" + r.stderr)
        return 1
    got = []
    for line in r.stdout.splitlines():
        try:
            got.append(float(line.strip()))
        except ValueError:
            pass
    print("expected:", [repr(v) for v in expected])
    print("got:     ", [repr(v) for v in got])
    if len(got) != len(expected):
        print(f"count mismatch: got {len(got)}, want {len(expected)}")
        return 1
    worst = 0.0
    for g, e in zip(got, expected):
        d = abs(g - e) / max(1.0, abs(e))
        worst = max(worst, d)
        if d > TOL:
            print(f"MISMATCH: got {g!r}, want {e!r} (rel {d:.3g})")
            return 1
    print(f"all {len(expected)} quantiles agree (worst rel diff {worst:.3g})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
