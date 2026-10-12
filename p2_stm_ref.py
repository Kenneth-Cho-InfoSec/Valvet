class Store:
    def __init__(self):
        self.vals = [100, 50, 0]
        self.vers = [0, 0, 0]


def run_script(s, ops):
    rvars, rvals, rvers, wvars, wvals = [], [], [], [], []
    seen = []
    i = 0
    ok = True
    def read(v):
        if v in wvars:
            return wvals[wvars.index(v)]
        if v in rvars:
            return rvals[rvars.index(v)]
        rvars.append(v)
        rvals.append(s.vals[v])
        rvers.append(s.vers[v])
        return s.vals[v]
    def write(v, x):
        if v in wvars:
            wvals[wvars.index(v)] = x
        else:
            wvars.append(v)
            wvals.append(x)
    while i < len(ops) and ok:
        op = ops[i]
        if op == 0:
            seen.append(read(ops[i+1]))
            i += 2
        elif op == 1:
            write(ops[i+1], ops[i+2])
            i += 3
        elif op == 2:
            if read(ops[i+1]) == 0:
                ok = False
            i += 2
        else:
            if read(ops[i+1]) != ops[i+2]:
                ok = False
            i += 3
    return (rvars, rvals, rvers, wvars, wvals, seen, ok)


def valid(s, log):
    rvars, rvals, rvers, _, _, _, _ = log
    return all(s.vers[v] == rv for v, rv in zip(rvars, rvers))


def commit(s, log):
    _, _, _, wvars, wvals, _, _ = log
    for v, x in zip(wvars, wvals):
        s.vals[v] = x
        s.vers[v] += 1


def atomic(s, ops):
    log = run_script(s, ops)
    if not log[6] or not valid(s, log):
        return False
    commit(s, log)
    return True


def or_else(s, p, q):
    return atomic(s, p) or atomic(s, q)


def show(s):
    print("\n".join(map(str, s.vals)))


s = Store()
t1 = [0, 0, 1, 0, 110]
print(str(atomic(s, t1)).lower())
show(s)
t2 = [3, 0, 110, 1, 0, 80, 1, 1, 80]
print(str(atomic(s, t2)).lower())
show(s)
t3 = [3, 0, 999, 1, 0, 0]
print(str(atomic(s, t3)).lower())
show(s)
p4 = [3, 2, 1]
q4 = [1, 2, 7]
print(str(or_else(s, p4, q4)).lower())
show(s)
t5 = [0, 0]
log5 = run_script(s, t5)
tw = [0, 0, 1, 0, 81]
atomic(s, tw)
print(str(valid(s, log5)).lower())
print(s.vers[0])
