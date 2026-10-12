#!/usr/bin/env python3
"""Reference for papers/p2_hierclust.valvet. IDENTICAL ops and order:
squared Euclidean distances; cluster distance = mean over member
cross-pairs accumulated outer-then-inner in member order with ONE final
division; closest-pair scan ascending with strict < (ties -> first);
new ids n..2n-2; merge prints new/left/right/height/size.
Only + - * / and comparisons, same order as the Valvet port.
"""
xs = [1.0, 1.5, 0.8, 5.0, 5.4, 4.9, 9.0, 9.3]
ys = [1.0, 1.2, 1.7, 5.0, 4.8, 5.5, 1.0, 1.4]
n = len(xs)

def sq2(a, b):
    dx = xs[a] - xs[b]
    dy = ys[a] - ys[b]
    return dx * dx + dy * dy

def cdist(ma, mb):
    s = 0.0
    for i in range(len(ma)):
        for j in range(len(mb)):
            s = s + sq2(ma[i], mb[j])
    return s / (len(ma) * len(mb))

members = [[i] for i in range(n)]
sizes = [1] * n
active = [1] * n
nxt = n
nactive = n
print("merges:")
while nactive > 1:
    ba, bb = 0, 0
    best = 0.0
    first = True
    for a in range(nxt):
        if active[a] == 1:
            for b in range(a + 1, nxt):
                if active[b] == 1:
                    d = cdist(members[a], members[b])
                    if first or d < best:
                        best = d
                        ba, bb = a, b
                        first = False
    m = list(members[ba]) + list(members[bb])
    sz = sizes[ba] + sizes[bb]
    members.append(m)
    sizes.append(sz)
    active.append(1)
    active[ba] = 0
    active[bb] = 0
    print(nxt)
    print(ba)
    print(bb)
    print(repr(best))
    print(sz)
    nactive -= 1
    nxt += 1
