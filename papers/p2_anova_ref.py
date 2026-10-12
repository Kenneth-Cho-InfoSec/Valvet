def mean(v):
    s = 0.0
    for x in v:
        s += x
    return s / len(v)

g1 = [6.0, 8.0, 4.0, 5.0, 3.0, 4.0]
g2 = [8.0, 12.0, 9.0, 11.0, 6.0, 8.0]
g3 = [13.0, 9.0, 11.0, 8.0, 7.0, 12.0]
m1, m2, m3 = mean(g1), mean(g2), mean(g3)
n1, n2, n3 = float(len(g1)), float(len(g2)), float(len(g3))
n = n1 + n2 + n3
grand = (n1 * m1 + n2 * m2 + n3 * m3) / n
ssb = n1 * (m1 - grand) ** 2 + n2 * (m2 - grand) ** 2 + n3 * (m3 - grand) ** 2
ssw = sum((x - m1) ** 2 for x in g1) + sum((x - m2) ** 2 for x in g2) + sum((x - m3) ** 2 for x in g3)
f = (ssb / (3.0 - 1.0)) / (ssw / (n - 3.0))
for v in (m1, m2, m3, grand, ssb, ssw, f):
    print(repr(v))
