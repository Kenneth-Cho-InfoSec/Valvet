out = []
blocks = []  # block sizes allocated
n = 0
length = 0
def bsize(k):
    return 1 << k
i = 0
while i < 200:
    before = len(blocks)
    if length == sum(blocks):
        blocks.append(bsize(len(blocks)))
    length += 1
    after = len(blocks)
    if after != before:
        out.append("newblock")
        out.append(str(after - 1))
        out.append(str(bsize(after - 1)))
    i += 1
out.append("len")
out.append(str(length))
out.append("nblocks")
out.append(str(len(blocks)))
data = list(range(200))
out.append("gets")
for j in [0, 1, 2, 63, 64, 100, 127, 128, 199]:
    out.append(str(data[j]))
out.append("errors")
out.append("0")
out.append("sum")
out.append(str(sum(data)))
data[0] = 1000
data[100] = 2000
data[199] = 3000
out.append("gets2")
out.append(str(data[0]))
out.append(str(data[100]))
out.append(str(data[199]))
out.append("sum2")
out.append(str(sum(data)))
out.append("errors2")
err2 = 0
for w in range(200):
    want = 1000 if w == 0 else (2000 if w == 100 else (3000 if w == 199 else w))
    if data[w] != want:
        err2 += 1
out.append(str(err2))
print("\n".join(out))
