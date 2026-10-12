#!/usr/bin/env python3
"""B2 differential test: Rosenbrock gradient descent vs
papers/diffsharp_rosen.valvet (DiffSharp grad API via forward-mode duals).

Implements IDENTICAL operation order to both the Valvet Dual operators
(stdlib/autodiff.valvet) and the GD loop. Compares with relative tolerance
1e-12 (floats print with %.17g in Valvet vs shortest-repr in Python, so
exact string match is impossible; bit-identical values are expected).
Also checks the initial dual gradient against the analytic gradient.
Exit 0 on match, 1 otherwise.
"""
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VALVET = os.path.join(ROOT, "target", "debug", "valvet")
PROG = os.path.join(ROOT, "papers", "diffsharp_rosen.valvet")
TOL = 1e-12


def dconst(x):
    return (x, 0.0)


def dadd(a, b):
    return (a[0] + b[0], a[1] + b[1])


def dsub(a, b):
    return (a[0] - b[0], a[1] - b[1])


def dmul(a, b):
    return (a[0] * b[0], a[1] * b[0] + a[0] * b[1])


def seed(w, k):
    return [(w[i], 1.0 if i == k else 0.0) for i in range(2)]


def rosen(p):
    one = dconst(1.0)
    c100 = dconst(100.0)
    x = p[0]
    y = p[1]
    t1 = dsub(one, x)
    a = dmul(t1, t1)
    xsq = dmul(x, x)
    t2 = dsub(y, xsq)
    b = dmul(dmul(c100, t2), t2)
    return dadd(a, b)


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
    w = [-1.2, 1.0]
    g0 = rosen(seed(w, 0))[1]
    g1 = rosen(seed(w, 1))[1]
    # analytic gradient: df/dx = -2(1-x) - 400x(y-x^2), df/dy = 200(y-x^2)
    x0, y0 = w
    ag0 = -2.0 * (1.0 - x0) - 400.0 * x0 * (y0 - x0 * x0)
    ag1 = 200.0 * (y0 - x0 * x0)
    expected = [g0, g1]
    w = [w[0], w[1]]
    for _ in range(2000):
        gg0 = rosen(seed(w, 0))[1]
        gg1 = rosen(seed(w, 1))[1]
        w[0] = w[0] - 0.001 * gg0
        w[1] = w[1] - 0.001 * gg1
    expected += [w[0], w[1], rosen_f(w[0], w[1])]

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
    actual = [float(x) for x in p.stdout.splitlines()]
    names = ["g0_init", "g1_init", "x_final", "y_final", "f_final"]
    ok = True
    for n, e, a in zip(names, expected, actual):
        r = rel(e, a)
        flag = "OK " if r <= TOL else "FAIL"
        if r > TOL:
            ok = False
        print("%s %-8s valvet=%r python=%r rel=%g" % (flag, n, a, e, r))
    for n, d, an in [("ag0", g0, ag0), ("ag1", g1, ag1)]:
        r = rel(d, an)
        flag = "OK " if r <= TOL else "FAIL"
        if r > TOL:
            ok = False
        print("%s dual-vs-analytic-%s rel=%g" % (flag, n, r))
    print("B2 %s" % ("OK" if ok else "MISMATCH"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
