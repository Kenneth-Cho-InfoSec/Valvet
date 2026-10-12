# Paper translations: advanced F# programs ported to Valvet

Each entry pairs an academic paper with its F# implementation (every `.fs`
source below is over 200 lines; counts verified at staging) and a Valvet
port of the paper's core algorithm plus a Python differential oracle.

Valvet has no closures, function values, higher-order functions, objects,
or generics, so these are first-order kernels, not line ports. Deliberate
adaptations are disclosed in each file's header comment.

| # | Paper | F# source (lines) | Valvet port | Oracle | Result |
|---|---|---|---|---|---|
| 1 | Claessen, Hughes — QuickCheck (ICFP 2000) | `quickcheck_runner.fs` (727) | `qc_props.valvet` | `qc_props_ref.py` | 4 props OK/100, broken demo caught |
| 2 | Leijen, Meijer — Parsec (2001) | `parsec_charparsers.fs` (1581) | `parsec_arith.valvet` | `parsec_arith_ref.py` | 18/18 agree (10 values, 8 errors) |
| 3 | Petricek et al. — Types from Data (PLDI 2016) | `fsharpdata_jsongenerator.fs` (789) | `shape_infer.valvet` | `shape_infer_ref.py` | 7 shapes exact |
| 4 | Baydin et al. — DiffSharp (2015) | `diffsharp_tensor.fs` (3462) | `diffsharp_rosen.valvet` | `diffsharp_rosen_ref.py` | gradients + GD bit-identical |
| 5 | Dzik et al. — MBrace (PLOS 2013) | `mbrace_cloudworkflows.fs` (617) | `mbrace_wordcount.valvet` | `mbrace_wordcount_ref.py` | byte-identical |
| 6 | Okasaki — Red-Black Trees (JFP 1999) | `fsharp_map.fs` (1290, AVL in practice) | `rbtree.valvet` | `rbtree_ref.py` | byte-identical |
| 7 | Okasaki — Batched Deque | `fsharpx_deque.fs` (292) | `deque.valvet` | `deque_ref.py` | byte-identical |
| 8 | Needleman-Wunsch (1970) / Smith-Waterman (1981) | `biofsharp_alignment.fs` (422) | `align.valvet` | `align_ref.py` | byte-identical |
| 9 | Lloyd — K-Means (1982) | `mbrace_kmeans.fsx` (315) | `kmeans.valvet` | `kmeans_ref.py` | worst rel diff 0.0 |
| 10 | Shannon — Chess Programming (1950) | `fsharpchess_moves.fs` (272) | `alphabeta.valvet` | `alphabeta_ref.py` | exact + pruning proof |

Run a port plus its oracle, e.g.:

```sh
./target/debug/valvet run papers/qc_props.valvet
python3 papers/qc_props_ref.py
```

`bug_*.valvet` are minimal Valvet compiler-bug repros found during porting
(all fixed; each is locked by a fixture under `valvet-tests/`).
