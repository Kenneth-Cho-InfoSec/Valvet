#!/usr/bin/env python3
"""P2 differential test: CH4 isotopic pattern vs papers/p2_isotopic.valvet.

Implements IDENTICAL operation order to the Valvet program (multinomial
enumeration over C-13 count c in 0..1 and H-2 count h in 0..4, factorial
literals, repeated-multiplication powers, +-*/ only). Compares all 10
(mass, probability) bins with relative tolerance 1e-9.
Exit 0 on match, 1 otherwise.
"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VALVET = os.path.join(ROOT, "target", "debug", "valvet")
PROG = os.path.join(ROOT, "papers", "p2_isotopic.valvet")
TOL = 1e-9


def fact4(n):
    if n == 0:
        return 1.0
    else:
        if n == 1:
            return 1.0
        else:
            if n == 2:
                return 2.0
            else:
                if n == 3:
                    return 6.0
                else:
                    return 24.0


def pow_n(base, n):
    r = 1.0
    k = 0
    while k < n:
        r = r * base
        k += 1
    return r


def rel(a, b):
    if a == b:
        return 0.0
    d = abs(a - b)
    m = max(abs(a), abs(b))
    return d / m if m else d


def main():
    mc12 = 12.0
    mc13 = 13.0033548378
    mh1 = 1.00782503223
    mh2 = 2.01410177812
    pc12 = 0.9893
    pc13 = 0.0107
    ph1 = 0.999885
    ph2 = 0.000115
    expected = []
    for c in range(0, 1 + 1):
        for h in range(0, 4 + 1):
            cf = float(c)
            hf = float(h)
            mass = (1.0 - cf) * mc12 + cf * mc13 + (4.0 - hf) * mh1 + hf * mh2
            pc = pc12 if c == 0 else pc13
            prob = 24.0 / (fact4(h) * fact4(4 - h)) * pc * pow_n(ph1, 4 - h) * pow_n(ph2, h)
            expected += [mass, prob]

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
    actual = [float(v) for v in p.stdout.splitlines()]
    if len(actual) != len(expected):
        print("line count mismatch: valvet=%d python=%d" % (len(actual), len(expected)))
        return 1
    ok = True
    for i, (e, a) in enumerate(zip(expected, actual)):
        kind = "mass" if i % 2 == 0 else "prob"
        n = "c=%d,h=%d.%s" % (i // 10, (i // 2) % 5, kind)
        r = rel(e, a)
        flag = "OK " if r <= TOL else "FAIL"
        if r > TOL:
            ok = False
        print("%s %-12s valvet=%r python=%r rel=%g" % (flag, n, a, e, r))
    print("p2_isotopic %s" % ("OK" if ok else "MISMATCH"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
