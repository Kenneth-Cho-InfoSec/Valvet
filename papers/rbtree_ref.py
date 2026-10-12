"""Reference for papers/rbtree.valvet: same arena Okasaki RB, same LCG order."""
NEG1 = -1


def empty():
    return -1


class Arena:
    def __init__(self):
        self.red = []
        self.key = []
        self.left = []
        self.right = []

    def mk(self, red, key, l, r):
        self.red.append(red)
        self.key.append(key)
        self.left.append(l)
        self.right.append(r)
        return len(self.red) - 1

    def is_red(self, t):
        return t != -1 and self.red[t]


def balance(a, t):
    p = plan_full(a, t)
    if p is None:
        return t
    kx, la, lb, ky, kz, c, d = p
    nl = a.mk(False, kx, la, lb)
    nr = a.mk(False, kz, c, d)
    return a.mk(True, ky, nl, nr)


def plan_full(a, t):
    if t == -1:
        return None
    n_red, z, l, r = a.red[t], a.key[t], a.left[t], a.right[t]
    if n_red:
        return None
    # All four patterns are parallel: classify each side on its own.
    lt = 0
    if a.is_red(l):
        y, ll, c = a.key[l], a.left[l], a.right[l]
        if a.is_red(ll):
            lt = 1  # LL
        elif a.is_red(a.right[l]):
            lr = a.right[l]
            lt = 2  # LR
    rt = 0
    if a.is_red(r):
        rl = a.left[r]
        if a.is_red(rl):
            rt = 3  # RL
        elif a.is_red(a.right[r]):
            rt = 4  # RR
    if lt == 1:
        return (a.key[ll], a.left[ll], a.right[ll], y, z, c, r)
    if lt == 2:
        lr = a.right[l]
        return (y, ll, a.left[lr], a.key[lr], z, a.right[lr], r)
    if rt == 3:
        rl = a.left[r]
        return (z, l, a.left[rl], a.key[rl], a.key[r], a.right[rl], a.right[r])
    if rt == 4:
        rr = a.right[r]
        return (z, l, a.left[r], a.key[r], a.key[rr], a.left[rr], a.right[rr])
    return None


def ins(a, t, key):
    if t == -1:
        return a.mk(True, key, -1, -1)
    k = a.key[t]
    if key < k:
        a.left[t] = ins(a, a.left[t], key)
        return balance(a, t)
    if key > k:
        a.right[t] = ins(a, a.right[t], key)
        return balance(a, t)
    return t


def check(a, t):
    if t == -1:
        return 0
    hl = check(a, a.left[t])
    hr = check(a, a.right[t])
    if hl == -1 or hr == -1 or hl != hr:
        return -1
    if a.red[t]:
        if a.is_red(a.left[t]) or a.is_red(a.right[t]):
            return -1
        return hl
    return hl + 1


def inorder(a, t, out):
    if t == -1:
        return
    inorder(a, a.left[t], out)
    out.append(a.key[t])
    inorder(a, a.right[t], out)


def members(a, t, key):
    while t != -1:
        k = a.key[t]
        if key == k:
            return True
        t = a.left[t] if key < k else a.right[t]
    return False


keys = list(range(201))
state = 12345
j = 200
while j > 0:
    state = (state * 1103515245 + 12345) % 2147483648
    k = state % (j + 1)
    keys[j], keys[k] = keys[k], keys[j]
    j -= 1

a = Arena()
root = -1
for k in keys:
    root = ins(a, root, k)
    a.red[root] = False  # textbook insert blackens after EVERY key
order = []
inorder(a, root, order)
bh = check(a, root)
ok = (not a.is_red(root)) and bh != -1
out = []
out.append("insertions=%d" % len(keys))
out.append("[" + ", ".join(str(x) for x in order) + "]")
out.append("nodes=%d" % len(a.red))
out.append("root_key=%d" % a.key[root])
out.append("black_height=%d" % bh)
out.append("invariants_ok=%s" % str(ok).lower())
out.append("member_0=%s" % str(members(a, root, 0)).lower())
out.append("member_200=%s" % str(members(a, root, 200)).lower())
out.append("member_201=%s" % str(members(a, root, 201)).lower())
out.append("member_42=%s" % str(members(a, root, 42)).lower())
print("\n".join(out))
