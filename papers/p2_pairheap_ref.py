import heapq
import re

src = open("papers/p2_pairheap.valvet").read()
ops = [int(x) for x in re.findall(r"\((-?\d+) as Int64\)", src.split("val ops:")[1].split("];")[0])]
h = []
out = []
for op in ops:
    if op == -1:
        out.append(str(heapq.heappop(h) if h else -1))
    elif op == -2:
        out.append(str(h[0] if h else -1))
    else:
        heapq.heappush(h, op)
print("\n".join(out))
