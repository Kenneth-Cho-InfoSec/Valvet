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

## Round 2 (papers 11–30)

Same method: F# sources verified over 200 lines in `papers/p2_*.fs`,
first-order Valvet kernels in `papers/p2_*.valvet`, Python oracles in
`papers/p2_*.py`, all run and compared.

| # | Paper | F# source (lines) | Valvet port | Result |
|---|---|---|---|---|
| 11 | Fredman et al. — Pairing heaps (1986) | `p2_pairheap.fs` (406) | `p2_pairheap.valvet` | byte-identical (heapq) |
| 12 | Akima — subspline interpolation (1970) | `p2_interpolation.fs` (2697) | `p2_akima.valvet` | worst rel 0.0 |
| 13 | Hughes — difference lists (1984) | `p2_dlist.fs` (333) | `p2_dlist.valvet` | byte-identical |
| 14 | Bagwell — ideal hash trees (2001) | `p2_hamt.fs` (1031) | `p2_hamt.valvet` | byte-identical (dict) |
| 15 | Brodnik et al. — resizable arrays (1999) | `p2_resizearray.fs` (512) | `p2_resizearray.valvet` | byte-identical |
| 16 | Nelder & Mead — simplex (1965) | `p2_neldermead.fs` (270) | `p2_neldermead.valvet` | bit-identical |
| 17 | Hyndman & Fan — sample quantiles (1996) | `p2_quantile.fs` (557) | `p2_quantile.valvet` | worst rel 0 |
| 18 | Marsaglia & Bray — polar normals (1964) | `p2_normal.fs` (267) | `p2_boxmuller.valvet` | worst rel 1.35e-16 |
| 19 | Knuth — LR parsing (1965) | `p2_lalr.fs` (687) | `p2_lalr.valvet` | exact (values+errors) |
| 20 | Rabin & Scott — finite automata (1959) | `p2_dfasim.fs` (317) | `p2_dfasim.valvet` | byte-identical |
| 21 | Ongaro & Ousterhout — Raft (2014) | `p2_raft.fs` (343) | `p2_raft.valvet` | byte-identical |
| 22 | Rosenblatt/Parzen — KDE (1956/1962) | `p2_kdensity.fs` (206) | `p2_kdensity.valvet` | worst rel 1.6e-16 |
| 23 | Sokal & Michener — UPGMA (1958) | `p2_hierclust.fs` (647) | `p2_hierclust.valvet` | worst rel 0.0 |
| 24 | Harris et al. — composable STM (2005) | `p2_stm.fs` (426) | `p2_stm.valvet` | byte-identical |
| 25 | Coulom / Kocsis & Szepesvari — MCTS/UCT (2006) | `p2_mcts.fs` (670) | `p2_mcts.valvet` | byte-identical |
| 26 | Cover & Hart — nearest neighbor (1967) | `p2_knn.fs` (235) | `p2_knn.valvet` | byte-identical |
| 27 | Kingma & Ba — Adam (2014) | `p2_adam.fs` (277) | `p2_adam.valvet` | bit-identical |
| 28 | Rockwood / Yergey — isotopic patterns | `p2_isotopic.fs` (451) | `p2_isotopic.valvet` | bit-identical |
| 29 | Driscoll et al. — persistence (1989) | `p2_persistvec.fs` (751) | `p2_persistvec.valvet` | byte-identical |
| 30 | Fisher — ANOVA (1925) | `p2_anova.fs` (279) | `p2_anova.valvet` | worst rel 0.0 |

Notes: #18's F# file implements Marsaglia–Bray polar, not Box–Muller
(paper swapped honestly). #2's skew-list file turned out to be a 32-way
persistent vector, so that slot became Akima (Newton divided differences
appear in the same file). #6's map file is AVL in practice though the port
implements Okasaki balancing per the paper.
