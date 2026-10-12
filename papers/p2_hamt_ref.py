import re

src = open("papers/p2_hamt.valvet").read()
ops = [int(x) for x in re.findall(r"\((-?\d+) as Int64\)", src.split("val ops:")[1].split("];")[0])]
trips = [(ops[i], ops[i+1], ops[i+2]) for i in range(0, len(ops), 3)]

d = {}
out = []
for op, key, vv in trips:
    if op == 0:
        d[key] = vv
        out.append(str(len(d)))
    elif op == 1:
        out.append(str(d.get(key, -1)))
    else:
        before = len(d)
        d.pop(key, None)
        out.append("1" if len(d) != before else "0")
out.append(str(len(d)))
for key in [5, 17, 42, 0, 29, 200, 201, 210]:
    out.append(str(d.get(key, -1)))

MASK = (1 << 64) - 1
def u64(x):
    return x & MASK
def h(k):
    return u64(k * 6364136223846793005 + 1442695040888963407)
keys = [5, -3, 17, 42, 0, 99, 7, 1001, -1001, 13, 29, 55, 71, 3, 8, 100, -100,
        1000000, -1000000, 123456789, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209]
n = 0
for i in range(len(keys)):
    for j in range(i + 1, len(keys)):
        if keys[i] != keys[j] and h(keys[i]) == h(keys[j]):
            n += 1
out.append(str(n))
print("\n".join(out))
