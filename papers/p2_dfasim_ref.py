T = [
    [1, 0, 3],
    [1, 2, 3],
    [1, 0, 3],
    [3, 3, 3],
]


def run(inp):
    st = 0
    for cls in inp:
        st = 3 if cls < 0 or cls > 2 else T[st][cls]
    return st


for inp in ([0, 1], [0, 0, 1], [1, 0, 1], [0, 0], [1, 1], [0, 2, 1], [1], []):
    st = run(inp)
    print(st if st == 2 else -100 - st)
