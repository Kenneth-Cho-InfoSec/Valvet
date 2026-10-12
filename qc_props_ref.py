"""Differential reference for papers/qc_props.valvet.

Reimplements the same Section 2 properties (Claessen & Hughes, ICFP 2000)
over Python random data: the four true properties must hold on all cases,
and the mistaken revApp variant must fail (as the Valvet runner reports).
"""
import random

rng = random.Random(2000)


def gen_int(lo=-100, hi=100):
    return rng.randint(lo, hi)


def gen_list(maxlen=10):
    return [gen_int() for _ in range(rng.randint(0, maxlen))]


def rev(xs):
    return xs[::-1]


def prop_rev_unit(x):
    return rev([x]) == [x]


def prop_rev_app(xs, ys):
    return rev(xs + ys) == rev(ys) + rev(xs)


def prop_rev_rev(xs):
    return rev(rev(xs)) == xs


def prop_max_le(x, y):
    return (max(x, y) == y) if x <= y else True


def prop_rev_app_wrong(xs, ys):
    return rev(xs + ys) == rev(xs) + rev(ys)


N = 1000
for name, mk in [
    ("prop_RevUnit", lambda: prop_rev_unit(gen_int())),
    ("prop_RevApp", lambda: prop_rev_app(gen_list(), gen_list())),
    ("prop_RevRev", lambda: prop_rev_rev(gen_list())),
    ("prop_MaxLe", lambda: prop_max_le(gen_int(), gen_int())),
]:
    bad = sum(1 for _ in range(N) if not mk())
    assert bad == 0, f"{name}: {bad}/{N} failures"
    print(f"{name}: OK, passed {N} tests")

fails = sum(1 for _ in range(N)
            if not prop_rev_app_wrong(gen_list(), gen_list()))
assert fails > 0, "broken revApp variant unexpectedly held"
print(f"prop_RevAppWrong: FAILED as expected ({fails}/{N} counterexamples)")
print("Python agrees with Valvet: 4 hold, 1 fails")
