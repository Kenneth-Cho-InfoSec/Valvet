#!/usr/bin/env python3
"""Reference for papers/kmeans.valvet. IDENTICAL init/order/ops:
init = first k points; assign by squared distance with strict < (ties ->
lowest index); accumulate sums in point order; mean = sum/count; empty
cluster keeps old centroid; stop when no assignment changes; cap 100.
Only + - * / and comparisons, same order as the Valvet port.
"""
pts = [
    (0.8, 1.2), (8.1, 1.9), (4.1, 8.2),
    (1.1, 0.9), (7.8, 2.2), (3.8, 7.9),
    (0.9, 1.0), (8.3, 2.0), (4.2, 7.7),
    (1.2, 1.3), (7.9, 1.7), (3.9, 8.3),
    (0.7, 0.8), (8.0, 2.4), (4.0, 7.8),
    (1.0, 1.1), (8.2, 1.6), (4.3, 8.1),
    (1.3, 0.7), (7.7, 2.1), (3.7, 8.0),
    (0.9, 1.4), (8.4, 2.3), (4.1, 7.9),
]
k = 3
n = len(pts)
cents = [pts[i] for i in range(k)]
assign = [-1] * n
changed = True
iterc = 0
while changed and iterc < 100:
    changed = False
    for p in range(n):
        best = (pts[p][0] - cents[0][0]) ** 2 + (pts[p][1] - cents[0][1]) ** 2
        mini = 0
        for i in range(1, k):
            d = (pts[p][0] - cents[i][0]) ** 2 + (pts[p][1] - cents[i][1]) ** 2
            if d < best:
                best = d
                mini = i
        if mini != assign[p]:
            assign[p] = mini
            changed = True
    sx = [0.0] * k
    sy = [0.0] * k
    cnt = [0] * k
    for q in range(n):
        a = assign[q]
        sx[a] = sx[a] + pts[q][0]
        sy[a] = sy[a] + pts[q][1]
        cnt[a] = cnt[a] + 1
    for r in range(k):
        if cnt[r] > 0:
            cents[r] = (sx[r] / cnt[r], sy[r] / cnt[r])
    iterc += 1
print("centroids:")
for cx, cy in cents:
    print(repr(cx))
    print(repr(cy))
print("assignments:")
for a in assign:
    print(a)
print("iters:")
print(iterc)
