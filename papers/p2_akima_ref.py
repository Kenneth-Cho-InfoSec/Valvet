def fabs(x):
    return -x if x < 0.0 else x

def three_point(xs, ys, it, i0, i1, i2):
    x0, x1, x2 = ys[i0], ys[i1], ys[i2]
    t = xs[it] - xs[i0]
    t1 = xs[i1] - xs[i0]
    t2 = xs[i2] - xs[i0]
    a = (x2 - x0 - (t2 / t1 * (x1 - x0))) / (t2 * t2 - t1 * t2)
    b = (x1 - x0 - a * t1 * t1) / t1
    return (2.0 * a * t) + b

def akima(xs, ys):
    n = len(xs)
    diff = [(ys[i+1] - ys[i]) / (xs[i+1] - xs[i]) for i in range(n - 1)]
    we = [0.0] + [fabs(diff[i] - diff[i-1]) for i in range(1, n - 2)] + [0.0]
    dd = [0.0] * n
    for i in range(2, n - 2):
        w0, w1 = we[i-1], we[i+1]
        if w0 == 0.0 and w1 == 0.0:
            dd[i] = (((xs[i+1] - xs[i]) * diff[i-1]) + ((xs[i] - xs[i-1]) * diff[i])) / (xs[i+1] - xs[i-1])
        else:
            dd[i] = ((w1 * diff[i-1]) + (w0 * diff[i])) / (w1 + w0)
    dd[0] = three_point(xs, ys, 0, 0, 1, 2)
    dd[1] = three_point(xs, ys, 1, 0, 1, 2)
    dd[n-2] = three_point(xs, ys, n-2, n-3, n-2, n-1)
    dd[n-1] = three_point(xs, ys, n-1, n-3, n-2, n-1)
    c0, c1, c2, c3 = [], [], [], []
    for i in range(n - 1):
        w = xs[i+1] - xs[i]
        ww = w * w
        c0.append(ys[i])
        c1.append(dd[i])
        c2.append(((3.0 * (ys[i+1] - ys[i])) / w - 2.0 * dd[i] - dd[i+1]) / w)
        c3.append(((2.0 * (ys[i] - ys[i+1])) / w + dd[i] + dd[i+1]) / ww)
    return (xs, c0, c1, c2, c3)

def akima_eval(s, x):
    xs, c0, c1, c2, c3 = s
    k = 0
    while k + 1 < len(xs) - 1 and xs[k+1] <= x:
        k += 1
    dx = x - xs[k]
    return c0[k] + dx * (c1[k] + dx * (c2[k] + dx * c3[k]))

xs = [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
ys = [1.0, 5.0, 4.0, 13.0, 17.0, 9.0, 12.0]
s = akima(xs, ys)
for p in [0.0, 0.5, 1.0, 2.5, 3.4, 4.0, 5.75, 6.0]:
    print(repr(akima_eval(s, p)))
