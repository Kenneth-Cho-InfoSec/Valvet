#!/usr/bin/env python3
"""P2 differential test: Adam on 2D Rosenbrock vs papers/p2_adam.valvet.

Implements IDENTICAL operation order to the Valvet program, including an
exact port of the Valvet stdlib sqrt (stdlib/math.valvet Newton loop:
start guess=x, next=(guess+x/guess)/2, stop when next==guess or
|next-guess|<1e-12, max 1000 iters). Python math.sqrt is correctly rounded
and does NOT match the stdlib approximation, hence the shared-loop scheme.
Bias powers are running products on both sides, matching the Valvet
b1p/b2p update bit-for-bit. Compares with relative tolerance 1e-12.
Exit 0 on match, 1 otherwise.
"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VALVET = os.path.join(ROOT, "target", "debug", "valvet")
PROG = os.path.join(ROOT, "papers", "p2_adam.valvet")
TOL = 1e-12


def vsqrt(x):
    if x < 0.0:
        return float("nan")
    if x == 0.0:
        return 0.0
    guess = x
    i = 0
    while i < 1000:
        nxt = (guess + x / guess) / 2.0
        if nxt == guess:
            return nxt
        diff = nxt - guess
        adiff = 0.0 - diff if diff < 0.0 else diff
        if adiff < 0.000000000001:
            return nxt
        guess = nxt
        i += 1
    return guess


def rosen_f(x, y):
    t1 = 1.0 - x
    a = t1 * t1
    t2 = y - x * x
    return a + 100.0 * t2 * t2


def rel(a, b):
    if a == b:
        return 0.0
    d = abs(a - b)
    m = max(abs(a), abs(b))
    return d / m if m else d


def main():
    x = 0.0 - 1.2
    y = 1.0
    lr = 0.001
    beta1 = 0.9
    beta2 = 0.999
    eps = 0.00000001
    omb1 = 1.0 - beta1
    omb2 = 1.0 - beta2
    m0 = 0.0
    m1 = 0.0
    v0 = 0.0
    v1 = 0.0
    b1p = 1.0
    b2p = 1.0
    step = 0
    while step < 1500:
        step += 1
        t1 = 1.0 - x
        xsq = x * x
        t2 = y - xsq
        g0 = 0.0 - 2.0 * t1 - 400.0 * x * t2
        g1 = 200.0 * t2
        m0 = m0 * beta1 + g0 * omb1
        m1 = m1 * beta1 + g1 * omb1
        v0 = v0 * beta2 + g0 * g0 * omb2
        v1 = v1 * beta2 + g1 * g1 * omb2
        b1p = b1p * beta1
        b2p = b2p * beta2
        bc1 = 1.0 - b1p
        bc2 = 1.0 - b2p
        d0 = vsqrt(v0) / vsqrt(bc2) + eps
        d1 = vsqrt(v1) / vsqrt(bc2) + eps
        p0 = m0 / d0
        p1 = m1 / d1
        ss = lr / bc1
        x = x - ss * p0
        y = y - ss * p1
    expected = [x, y, rosen_f(x, y)]

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
    names = ["x_final", "y_final", "f_final"]
    ok = True
    if len(actual) != len(expected):
        print("line count mismatch: valvet=%d python=%d" % (len(actual), len(expected)))
        return 1
    for n, e, a in zip(names, expected, actual):
        r = rel(e, a)
        flag = "OK " if r <= TOL else "FAIL"
        if r > TOL:
            ok = False
        print("%s %-8s valvet=%r python=%r rel=%g" % (flag, n, a, e, r))
    print("p2_adam %s" % ("OK" if ok else "MISMATCH"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
