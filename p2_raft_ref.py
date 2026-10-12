class Node:
    def __init__(self, i):
        self.id = i
        self.state = 0
        self.term = 0
        self.voted = -1
        self.log = []
        self.commit = -1
        self.alive = 1


def llen(n):
    return len(n.log)


def logterm(n, i):
    return -1 if i < 0 else n.log[i][0]


def req_vote(v, cterm, cid, clast, clastterm):
    if not v.alive:
        return 0
    if cterm < v.term:
        return 0
    if cterm > v.term:
        v.term = cterm
        v.state = 0
        v.voted = -1
    vlast = llen(v) - 1
    vlastterm = logterm(v, vlast)
    uptodate = 1 if (clastterm > vlastterm or
                     (clastterm == vlastterm and clast >= vlast)) else 0
    if (v.voted in (-1, cid)) and uptodate:
        v.voted = cid
        return 1
    return 0


def app_entries(v, lterm, lid, prev, prevterm, ents, lcommit):
    if not v.alive:
        return 0
    if lterm < v.term:
        return 0
    v.term = lterm
    v.state = 0
    if prev >= 0:
        if llen(v) <= prev or logterm(v, prev) != prevterm:
            return 0
    v.log = v.log[:prev + 1] + list(ents)
    if lcommit > v.commit:
        v.commit = lcommit
    return 1


n0, n1, n2 = Node(0), Node(1), Node(2)
out = [n0.term]
n0.state, n0.term, n0.voted = 1, 1, 0
g1 = req_vote(n1, 1, 0, -1, -1)
g2 = req_vote(n2, 1, 0, -1, -1)
out.append(g1 + g2)
if g1 + g2 >= 2:
    n0.state = 2
out.append(n0.state)
n0.log = [(1, 11)]
a1 = app_entries(n1, 1, 0, -1, -1, list(n0.log), -1)
a2 = app_entries(n2, 1, 0, -1, -1, list(n0.log), -1)
n0.log = [(1, 11), (1, 22), (1, 33)]
b1 = app_entries(n1, 1, 0, 0, 1, [(1, 22), (1, 33)], -1)
b2 = app_entries(n2, 1, 0, 0, 1, [(1, 22), (1, 33)], -1)
out.append(a1 + b1 + a2 + b2)
n0.commit = 2
c1 = app_entries(n1, 1, 0, 2, 1, [], 2)
c2 = app_entries(n2, 1, 0, 2, 1, [], 2)
out.append(n1.commit + n2.commit)
n0.alive = 0
n1.state, n1.term, n1.voted = 1, 2, 1
s2 = req_vote(n2, 2, 1, 2, 1)
s0 = req_vote(n0, 2, 1, 2, 1)
out.append(s2)
out.append(s0)
if s2 >= 1:
    n1.state = 2
out.append(n1.state)
same = 1 if (len(n1.log) == len(n2.log) == 3 and
             n1.log[0][1] == n2.log[0][1] and
             n1.log[2][1] == n2.log[2][1]) else 0
out.append(same)
print("\n".join(map(str, out)))
