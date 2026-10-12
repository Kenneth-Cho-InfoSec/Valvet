def vsqrt(x):
    if x < 0.0:
        return float("nan")
    if x == 0.0:
        return 0.0
    guess = x
    for _ in range(1000):
        nxt = (guess + x / guess) / 2.0
        if nxt == guess:
            return nxt
        if abs(nxt - guess) < 1e-12:
            return nxt
        guess = nxt
    return guess


def vlog(x):
    if x == 0.0:
        return float("-inf")
    k = 0
    y = x
    while y >= 2.0:
        y = y / 2.0
        k += 1
    while y < 1.0:
        y = y * 2.0
        k -= 1
    t = (y - 1.0) / (y + 1.0)
    t2 = t * t
    term = t
    s = t
    n = 3
    for _ in range(100):
        term = term * t2
        s = s + term / float(n)
        n += 2
    return s * 2.0 + float(k) * 0.6931471805599453


LINES = [(0, 1, 2), (3, 4, 5), (6, 7, 8), (0, 3, 6), (1, 4, 7),
         (2, 5, 8), (0, 4, 8), (2, 4, 6)]


def board_win(b):
    for a, bi, c in LINES:
        if b[a] != 0 and b[a] == b[bi] and b[bi] == b[c]:
            return b[a]
    if all(x != 0 for x in b):
        return 0
    return -1


def legal_moves(b):
    return [i for i in range(9) if b[i] == 0]


def other(p):
    return 2 if p == 1 else 1


class ND:
    __slots__ = ("board", "player", "visits", "wins", "parent", "kids", "tries")

    def __init__(self, board, player, parent, tries):
        self.board = board
        self.player = player
        self.visits = 0
        self.wins = 0.0
        self.parent = parent
        self.kids = []
        self.tries = tries


def ucb(w, n, npar):
    return w / float(n) + 1.4142135623730951 * vsqrt(vlog(float(npar)) / float(n))


tree = [ND([0] * 9, 1, -1, list(range(9)))]
rng = 12345


def rng_next():
    global rng
    rng = (rng * 1664525 + 1013904223) % 4294967296
    return rng


for _ in range(2000):
    cur = 0
    while True:
        nd = tree[cur]
        if board_win(nd.board) != -1:
            leaf = cur
            break
        if len(nd.tries) > 0:
            leaf = cur
            break
        best, bestu = nd.kids[0], -1.0
        for k in nd.kids:
            ck = tree[k]
            u = 1000000.0 if ck.visits == 0 else ucb(ck.wins, ck.visits, nd.visits)
            if u > bestu:
                bestu, best = u, k
        cur = best
    nd = tree[leaf]
    if board_win(nd.board) == -1 and len(nd.tries) > 0:
        mv = nd.tries[0]
        rest = nd.tries[1:]
        nb = list(nd.board)
        nb[mv] = nd.player
        nd.tries = rest
        child = len(tree)
        nd.kids.append(child)
        tree.append(ND(nb, other(nd.player), leaf, [i for i in range(9) if nb[i] == 0]))
        start = child
    else:
        start = leaf
    nd = tree[start]
    b, p = list(nd.board), nd.player
    while True:
        w = board_win(b)
        if w != -1:
            winner = w
            break
        ms = legal_moves(b)
        pick = rng_next() % len(ms)
        b[ms[pick]] = p
        p = other(p)
    cur = start
    while cur != -1:
        nd = tree[cur]
        nd.visits += 1
        if winner == 0:
            nd.wins += 0.5
        elif winner == nd.player:
            nd.wins += 1.0
        cur = nd.parent

root = tree[0]
best, bestv = root.kids[0], -1
for k in root.kids:
    if tree[k].visits > bestv:
        bestv, best = tree[k].visits, k
bn = tree[best].board
mv = next(i for i in range(9) if bn[i] != 0)
print(mv)
print(root.visits)
print(repr(root.wins))
