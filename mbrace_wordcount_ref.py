"""Reference for papers/mbrace_wordcount.valvet (collections.Counter)."""
from collections import Counter

text = "the cat sat on the mat and the dog sat on the rug the cat chased the dog and the dog chased the cat back home the mat was flat and the rug was soft the cat naps on the rug while the dog naps on the mat"
words = text.split(" ")
n = len(words)
p = 4
chunk = (n + p - 1) // p

combined = Counter()
order = {}
totals = []
for pid in range(p):
    s = pid * chunk
    e = min(s + chunk, n)
    local = Counter(words[s:e])
    totals.append(float(sum(local.values())))
    for w, c in local.items():  # ordered combine, partition-index order
        if w not in order:
            order[w] = len(order)
        combined[w] = combined.get(w, 0) + c

ss = 0.0
for t in totals:
    ss = ss + t
ps = ss  # par_sum is bit-identical (fixed blocks + ordered combining)

out = []
out.append("words=%d" % n)
out.append("partitions=%d" % p)
out.append("part0=%.17g part1=%.17g part2=%.17g part3=%.17g" % tuple(totals))
out.append("distinct=%d" % len(combined))
out.append("seq_sum=%.17g" % ss)
out.append("par_sum=%.17g" % ps)
out.append("sums_equal=%s" % str(ps == ss).lower())
for w in sorted(combined, key=lambda w: (-combined[w], order[w])):
    out.append("%s: %d" % (w, combined[w]))
print("\n".join(out))
