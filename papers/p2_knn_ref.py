train = [
    (1.0, 1.0, 0), (1.5, 2.0, 0), (2.0, 1.0, 0), (0.5, 1.5, 0),
    (2.5, 2.5, 0), (1.0, 2.5, 0), (3.0, 1.5, 0), (2.0, 0.5, 0),
    (8.0, 8.0, 1), (7.5, 9.0, 1), (9.0, 8.0, 1), (8.5, 7.0, 1),
    (7.0, 8.5, 1), (9.5, 9.0, 1), (8.0, 9.5, 1), (7.5, 7.5, 1),
]
queries = [(1.2, 1.3), (8.2, 8.1), (4.9, 5.1), (5.2, 4.8)]

def sqd(a, b):
    return (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2

def knn(q, k):
    best = []
    for (x, y, c) in train:
        d = sqd((x, y), q)
        if len(best) < k:
            best.append((d, c))
            best.sort(key=lambda t: t[0])
        elif d < best[k-1][0]:
            best[k-1] = (d, c)
            best.sort(key=lambda t: t[0])
    c0 = sum(1 for _, c in best if c == 0)
    c1 = len(best) - c0
    return 1 if c1 > c0 else 0

for q in queries:
    for k in (1, 3, 5):
        print(knn(q, k))
