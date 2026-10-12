ACT = [
    [13, 0, 0],
    [0, 14, 30],
    [0, 22, 22],
    [0, 23, 23],
    [13, 0, 0],
    [0, 21, 21],
]
GOTO = [
    [1, 2],
    [-1, -1],
    [-1, -1],
    [-1, -1],
    [-1, 5],
    [-1, -1],
]
RULES = {1: (0, 3), 2: (0, 1), 3: (1, 1)}  # rule -> (lhs_nt, rhs_len)


def parse(toks, vals):
    states = [0]
    vstack = []
    pos = 0
    while True:
        st = states[-1]
        tok = toks[pos]
        act = ACT[st][tok]
        kind, num = act // 10, act % 10
        if kind == 0:
            return (False, 0.0, pos)
        if kind == 3:
            return (True, vstack[-1], pos)
        if kind == 1:
            states.append(num)
            vstack.append(vals[pos])
            pos += 1
        else:
            lhs, ln = RULES[num]
            if num == 1:
                b = vstack[-1]
                a = vstack[-3]
                vstack = vstack[:-3]
                states = states[:-3]
                vstack.append(a + b)
            else:
                v = vstack[-1]
                vstack = vstack[:-1]
                states = states[:-1]
                vstack.append(v)
            ns = states[-1]
            states.append(GOTO[ns][lhs])


def run(toks, vals):
    ok, v, p = parse(toks, vals)
    print(repr(v) if ok else -1000 - p)


run([0, 1, 0, 1, 0, 2], [3.0, 0.0, 4.0, 0.0, 2.0, 0.0])
run([0, 2], [7.0, 0.0])
run([0, 1, 0, 1, 0, 1, 0, 2], [1.0, 0.0, 2.0, 0.0, 3.0, 0.0, 4.0, 0.0])
run([1, 0, 2], [0.0, 5.0, 0.0])
run([0, 1, 2], [3.0, 0.0, 0.0])
run([2], [0.0])
