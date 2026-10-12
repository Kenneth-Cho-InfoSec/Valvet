"""Differential reference for papers/align.valvet.

Independent Needleman-Wunsch (linear gap) implementation. Must use the same
recurrence, scoring (match +2, mismatch -1, gap -2), traceback tie-break
(diagonal, then up, then left), and output format: per case, score, row-1
length + elements, row-2 length + elements (gap = -1).
"""

MATCH = 2
MISMATCH = -1
GAP = -2


def nw(a, b):
    m, n = len(a), len(b)
    dp = [[0] * (n + 1) for _ in range(m + 1)]
    for j in range(1, n + 1):
        dp[0][j] = GAP * j
    for i in range(1, m + 1):
        dp[i][0] = GAP * i
    for i in range(1, m + 1):
        for j in range(1, n + 1):
            s = MATCH if a[i - 1] == b[j - 1] else MISMATCH
            best = dp[i - 1][j - 1] + s
            up = dp[i - 1][j] + GAP
            if up > best:
                best = up
            lf = dp[i][j - 1] + GAP
            if lf > best:
                best = lf
            dp[i][j] = best
    al1, al2 = [], []
    i, j = m, n
    while i > 0 or j > 0:
        if i > 0 and j > 0:
            cur = dp[i][j]
            s = MATCH if a[i - 1] == b[j - 1] else MISMATCH
            if cur == dp[i - 1][j - 1] + s:
                al1.append(a[i - 1])
                al2.append(b[j - 1])
                i -= 1
                j -= 1
            elif cur == dp[i - 1][j] + GAP:
                al1.append(a[i - 1])
                al2.append(-1)
                i -= 1
            else:
                al1.append(-1)
                al2.append(b[j - 1])
                j -= 1
        elif i > 0:
            al1.append(a[i - 1])
            al2.append(-1)
            i -= 1
        else:
            al1.append(-1)
            al2.append(b[j - 1])
            j -= 1
    al1.reverse()
    al2.reverse()
    return dp[m][n], al1, al2


CASES = [
    ([2, 0, 3, 3, 0, 1, 0], [2, 1, 0, 3, 2, 1, 3]),
    ([0, 1, 2, 3], [0, 1, 2, 3]),
    ([0], [1]),
    ([], [0, 1]),
]

out = []
for a, b in CASES:
    score, r1, r2 = nw(a, b)
    out.append(str(score))
    out.append(str(len(r1)))
    out.extend(str(x) for x in r1)
    out.append(str(len(r2)))
    out.extend(str(x) for x in r2)
print("\n".join(out))
