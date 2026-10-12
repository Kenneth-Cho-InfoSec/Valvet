#!/usr/bin/env python3
"""Exhaustive minimax oracle for papers/alphabeta.valvet.
Same rules: move order 0..8, strict > keeps first best, terminal at entry
(winner w -> +1 if w == side else -1; full board -> 0), 1 node per call.
Implements both full search and alpha-beta; checks they agree and that
pruning visits strictly fewer nodes on the empty board.
"""
import sys

LINES = [(0, 1, 2), (3, 4, 5), (6, 7, 8),
         (0, 3, 6), (1, 4, 7), (2, 5, 8),
         (0, 4, 8), (2, 4, 6)]

def winner(b):
    for a, c, d in LINES:
        if b[a] != 0 and b[a] == b[c] and b[a] == b[d]:
            return b[a]
    return 0

def full(b):
    return all(x != 0 for x in b)

def neg_full(b, side):
    nodes = 1
    w = winner(b)
    if w != 0:
        return (1 if w == side else -1, -1, nodes)
    if full(b):
        return (0, -1, nodes)
    best, bestmv = -2, -1
    for m in range(9):
        if b[m] == 0:
            nb = list(b)
            nb[m] = side
            cv, _, cn = neg_full(nb, 3 - side)
            nodes += cn
            v = -cv
            if v > best:
                best, bestmv = v, m
    return (best, bestmv, nodes)

def neg_ab(b, side, alpha, beta):
    nodes = 1
    w = winner(b)
    if w != 0:
        return (1 if w == side else -1, -1, nodes)
    if full(b):
        return (0, -1, nodes)
    best, bestmv = -2, -1
    a = alpha
    for m in range(9):
        if b[m] == 0:
            nb = list(b)
            nb[m] = side
            cv, _, cn = neg_ab(nb, 3 - side, -beta, -a)
            nodes += cn
            v = -cv
            if v > best:
                best, bestmv = v, m
            if best > a:
                a = best
            if a >= beta:
                break
    return (best, bestmv, nodes)

POSITIONS = [
    # (board, side, tag)
    ([0, 0, 0, 0, 0, 0, 0, 0, 0], 1, "empty-X-draw"),
    ([1, 1, 0, 2, 2, 0, 0, 0, 0], 1, "X-wins-in-one"),
    ([2, 2, 0, 1, 0, 0, 0, 0, 0], 1, "X-must-block"),
    ([2, 2, 0, 1, 1, 0, 0, 0, 0], 2, "O-wins-in-one"),
    ([1, 2, 1, 1, 2, 2, 2, 1, 0], 1, "last-move-draw"),
    ([1, 0, 2, 0, 1, 0, 0, 0, 0], 1, "midgame"),
]

def main():
    out = []
    for b, side, tag in POSITIONS:
        fv, fm, fn_ = neg_full(b, side)
        av, am, an = neg_ab(b, side, -2, 2)
        assert (fv, fm) == (av, am), (tag, (fv, fm), (av, am))
        agree = (fv == av and fm == am)
        out.append(tag)
        out.append(str(av))
        out.append(str(am))
        out.append(str(an))
        out.append(str(fv))
        out.append(str(fm))
        out.append(str(fn_))
        out.append("true" if agree else "false")
    eb = [0] * 9
    _, _, eab = neg_ab(eb, 1, -2, 2)
    _, _, efull = neg_full(eb, 1)
    out.append("empty")
    out.append(str(eab))
    out.append(str(efull))
    out.append("true" if eab < efull else "false")
    assert eab < efull
    print("\n".join(out))

if __name__ == "__main__":
    main()
