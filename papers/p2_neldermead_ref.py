def rosen(x, y):
    a = 1.0 - x
    b = y - x * x
    return a * a + 100.0 * b * b

def order3(f0, f1, f2):
    order = [0, 1, 2]
    fs = [f0, f1, f2]
    for i in range(3):
        for j in range(i + 1, 3):
            if fs[j] < fs[i]:
                a, b = fs[i], fs[j]
                fs[i], fs[j] = b, a
                u, w = order[i], order[j]
                order[i], order[j] = w, u
    return order

p = [(-1.2, 1.0), (-1.2 + 0.5, 1.0), (-1.2, 1.0 + 0.5)]
for _ in range(300):
    f = [rosen(*q) for q in p]
    o = order3(*f)
    bi, mi, wi = o
    pb, pm, pw = p[bi], p[mi], p[wi]
    cx = (pb[0] + pm[0]) / 2.0
    cy = (pb[1] + pm[1]) / 2.0
    rx, ry = cx + (cx - pw[0]), cy + (cy - pw[1])
    fr = rosen(rx, ry)
    fb = rosen(*pb)
    if fr < fb:
        ex, ey = cx + 2.0 * (cx - pw[0]), cy + 2.0 * (cy - pw[1])
        fe = rosen(ex, ey)
        if fe < fr:
            p = [pb, pm, (ex, ey)]
        else:
            p = [pb, pm, (rx, ry)]
    else:
        fm = rosen(*pm)
        if fr < fm:
            p = [pb, pm, (cx + 0.5 * (rx - cx), cy + 0.5 * (ry - cy))]
        else:
            ix, iy = cx + 0.5 * (pw[0] - cx), cy + 0.5 * (pw[1] - cy)
            fi = rosen(ix, iy)
            if fi < rosen(*pw):
                p = [pb, pm, (ix, iy)]
            else:
                p = [pb,
                     (pb[0] + 0.5 * (pm[0] - pb[0]), pb[1] + 0.5 * (pm[1] - pb[1])),
                     (pb[0] + 0.5 * (pw[0] - pb[0]), pb[1] + 0.5 * (pw[1] - pb[1]))]
print(repr(p[0][0]))
print(repr(p[0][1]))
print(repr(rosen(*p[0])))
