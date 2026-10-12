import math


def epan(bw, x):
    a = bw * math.sqrt(6.0)
    ax = abs(x)
    return 1.0 - ax / a if ax < a else 0.0


def kde(data, bw, x):
    return sum(epan(bw, x - xi) for xi in data) / (len(data) * bw)


data = [3.6, 1.8, 3.333, 2.283, 4.533, 2.883, 4.7, 3.6, 1.95, 4.35,
        1.833, 3.917, 4.2, 1.75, 4.7, 2.167, 1.75, 4.8, 1.6, 4.25]
for p in [1.0, 2.0, 3.0, 4.0, 5.0]:
    print(repr(kde(data, 0.5, p)))
