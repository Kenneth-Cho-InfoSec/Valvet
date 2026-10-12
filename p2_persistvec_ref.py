#!/usr/bin/env python3
"""Reference for papers/p2_persistvec.valvet. Each version is a separate
Python list (set copies the whole list); same script: v0=[10,20,30,40],
v1=set(v0,1,200), v2=set(v1,3,400), v3=set(v0,0,100); print all versions
after all sets. Compared EXACTLY (byte-for-byte) with the Valvet output.
"""
v0 = [10, 20, 30, 40]
v1 = list(v0); v1[1] = 200
v2 = list(v1); v2[3] = 400
v3 = list(v0); v3[0] = 100
for tag, v in (("v0:", v0), ("v1:", v1), ("v2:", v2), ("v3:", v3)):
    print(tag)
    for x in v:
        print(x)
