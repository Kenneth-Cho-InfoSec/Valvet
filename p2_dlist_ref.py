"""Differential reference for papers/p2_dlist.valvet.

Mirrors the Hughes difference-list laws with Python lists (plain-list
semantics: cons prepends, snoc appends, append concatenates). Output
format matches the Valvet program exactly (Vec<Int64> prints as
[1, 2, 3]).
"""


def fmt(v):
    return "[" + ", ".join(str(x) for x in v) + "]"


out = []
a = []
for x in (1, 2, 3):
    a = a + [x]
b = [6]
b = [5] + b
b = [4] + b
c = [7] + []
c = c + [8]
out.append(fmt(a))
out.append(fmt(b))
out.append(fmt(c))
out.append(f"{len(a)} {len(b)} {len(c)}")
lt = (a + b) + c
rt = a + (b + c)
out.append(fmt(lt))
out.append(str(int(lt == rt)))
out.append(str(int(a + [] == a and [] + a == a)))
out.append(str(int(len([]) == 0)))
out.append(str(int([0] + a == [0] + a)))
out.append(str(int(a + [9] == a + [9])))
d = []
for k in range(1, 11):
    d = d + [k]
out.append(fmt(d))
out.append(str(len(d)))
big = (a + b) + (c + d)
out.append(fmt(big))
out.append(str(len(big)))
print("\n".join(out))
