"""Differential reference for papers/deque.valvet.

Mirrors the same 64-op script with collections.deque. Output format matches
the Valvet program exactly: one integer per pop/peek result (None -> -1),
then final length, then final contents front-to-back one per line.
"""
from collections import deque

FLAT = [2, 0, 3, 0, 4, 0, 5, 0, 3, 0, 0, 2, 0, 2, 4, 0, 4, 0, 5, 0, 0, 8, 3, 0, 0, 6, 1, 18, 4, 0, 1, 3, 4, 0, 4, 0, 5, 0, 2, 0, 3, 0, 0, 24, 4, 0, 1, 3, 2, 0, 2, 0, 4, 0, 4, 0, 5, 0, 5, 0, 3, 0, 4, 0, 0, 10, 5, 0, 5, 0, 0, 3, 3, 0, 3, 0, 4, 0, 2, 0, 0, 10, 4, 0, 0, 1, 2, 0, 5, 0, 1, 13, 2, 0, 4, 0, 4, 0, 1, 14, 3, 0, 5, 0, 0, 10, 4, 0, 0, 13, 4, 0, 2, 0, 5, 0, 0, 8, 2, 0, 1, 2, 0, 12, 3, 0, 4, 0]

out = []
d = deque()
it = iter(FLAT)
for op, arg in zip(it, it):
    if op == 0:
        d.appendleft(arg)
    elif op == 1:
        d.append(arg)
    elif op == 2:
        out.append(str(d.popleft() if d else -1))
    elif op == 3:
        out.append(str(d.pop() if d else -1))
    elif op == 4:
        out.append(str(d[0] if d else -1))
    elif op == 5:
        out.append(str(d[-1] if d else -1))
    else:
        raise AssertionError(op)
out.append(str(len(d)))
out.extend(str(x) for x in d)
print("\n".join(out))
