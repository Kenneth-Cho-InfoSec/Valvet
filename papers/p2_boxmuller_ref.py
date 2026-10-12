#!/usr/bin/env python3
"""Differential test for papers/p2_boxmuller.valvet.

PAPER SWAP: papers/p2_normal.fs Normal.SampleUnchecked implements the
Marsaglia-Bray polar method (1964), not Box & Muller (1958); the Valvet
port mirrors the F# exactly, filenames kept as p2_boxmuller.*.

Hardcodes the IDENTICAL LCG (a=1664525, c=1013904223, m=2**32,
seed=12345) so the uniform stream is bit-identical, then applies the
IDENTICAL op order per accepted pair: v1=2*u1-1, v2=2*u2-1,
r=v1*v1+v2*v2, accept while r<1 and r>0,
fac=sqrt((0-2*log(r))/r), keep v1*fac (mu=0, sigma=1, N=2000).
Mean/variance(two-pass, population)/min/max/first-5 in the same order.
Compares with rel tolerance 1e-9 (Valvet sqrt/log are iterative
approximations in stdlib/math.valvet, not libm). Exit 0/1.
"""
import math
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VALVET = os.path.join(ROOT, "target", "debug", "valvet")
PROG = os.path.join(ROOT, "papers", "p2_boxmuller.valvet")
TOL = 1e-9
N = 2000


def main():
    seed = 12345
    out = []
    while len(out) < N:
        seed = (1664525 * seed + 1013904223) % 4294967296
        u1 = seed / 4294967296.0
        seed = (1664525 * seed + 1013904223) % 4294967296
        u2 = seed / 4294967296.0
        v1 = 2.0 * u1 - 1.0
        v2 = 2.0 * u2 - 1.0
        r = v1 * v1 + v2 * v2
        if r < 1.0 and r > 0.0:
            fac = math.sqrt((0.0 - 2.0 * math.log(r)) / r)
            out.append(v1 * fac)
    mean = sum(out) / N
    var = sum((x - mean) ** 2 for x in out) / N
    expected = [mean, var, min(out), max(out)] + out[:5]
    env = dict(os.environ)
    env["LD_LIBRARY_PATH"] = (os.path.expanduser("~/.local/vgcc/usr/lib") +
                              os.pathsep + env.get("LD_LIBRARY_PATH", ""))
    r_ = subprocess.run([VALVET, "run", PROG], capture_output=True,
                        text=True, env=env)
    if r_.returncode != 0:
        print("valvet failed:\n" + r_.stderr)
        return 1
    got = []
    for line in r_.stdout.splitlines():
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
    names = ["mean", "variance", "min", "max"] + [f"z[{i}]" for i in range(5)]
    for name, g, e in zip(names, got, expected):
        d = abs(g - e) / max(1.0, abs(e))
        worst = max(worst, d)
        if d > TOL:
            print(f"MISMATCH {name}: got {g!r}, want {e!r} (rel {d:.3g})")
            return 1
    print(f"all {len(expected)} values agree (worst rel diff {worst:.3g})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
