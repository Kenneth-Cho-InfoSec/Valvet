# Plan: R parity gaps, the Valvet way (R-GAPS track)

Status: accepted; P0 in progress (M1 `Mat` + M2 `sort`/`order` shipped:
M1 is a dense row-major Float64 matrix with `mat_zeros`/`mat_identity`/
`mat_from_rows` constructors, `rows`/`cols`/`transpose`/`clone` methods,
and LU-backed `matmul`/`lu_solve`/`det`/`inverse`/`cholesky` (partial
pivoting; shape mismatches and singular/non-SPD inputs are `Err`, `det`
is NaN on singular; fixtures vs closed-form oracles, ASan-clean); M2 is a
stable bottom-up mergesort kernel, NaN sorts last, `order` returns stable
permutation indices; fixtures + unit tests green, ASan-clean). This is a parallel track to the 0.3.0
production scope in `IMPLEMENTATION_STATUS.md` (lazy plans, Parquet/Arrow,
SQL, par_iter, FFI/unsafe, BLAS, registry, LSP); items that land may ship
under whatever release is next. Source analysis: 10 gaps ranked by real R
usage (tidyverse download dominance confirmed), each verified absent from
`stdlib/` and `crates/valvet_core/src/` before planning.

Design law for all of it: no GC, ownership stays exact, first-order
wherever possible, semantics fixed in the frontend with thin C kernels
like the existing `Vec` ops. Anything needing closures says so
explicitly; everything else is designed to not need them.

## Cross-cutting decision: closures, in two stages

Three items want function values (apply family directly; custom
aggregations and plot mappings indirectly). Everything else below is
engineered to avoid them, but the apply family cannot be. Proposal:
- **Stage 1, function items as values.** A bare `fun` name in value
  position becomes a function pointer (no environment, nothing to drop).
  Unlocks `map/filter/fold` with named functions, which covers most
  apply use. Small change: new `Fn(...)` type, call-through-value in
  codegen, E0800 lifted only for capture-free references.
- **Stage 2, capturing closures.** Inferred move-capture into an inline
  environment struct with drop glue, reusing the existing poison
  machinery. Harder; scheduled last, only if Stage 1 proves insufficient.

## P0: foundations (unlocks modeling and verbs)

**M1. `Mat` type + kernels.** New builtin `Mat { rows, cols: Int64,
data: Vec<Float64> }` with C kernels (prelude pattern): `transpose`,
`matmul`, `lu_solve` (LU with partial pivoting, singular returns `Err`),
`det`, `inverse`, `cholesky`. Jacobi eigenvalue and one-sided Jacobi SVD
follow in P3. Verification: fixtures vs Python `numpy` oracles, ASan
clean.
```valvet
val a = mat_from_rows(vec![vec![2.0, 1.0], vec![1.0, 3.0]]);
val b = vec![5.0, 7.0];
val x = match (lu_solve(&a, &b)) { Ok(v) -> v, Err(e) -> vec![], };
```
**M2. `sort`/`order` kernel.** Missing primitive that `arrange`, rank
tests, and quantiles-adjacent code all need. `Vec::sort` (introsort or
timsort-lite, in place, on `&mut`) plus `order(v)` returning permutation
indices. Pure codegen kernel + fixtures with adversarial inputs (sorted,
reverse, duplicates).

## P1: independent stdlib (parallelizable, no language changes)

**F1. Factors.** SHIPPED: builtin `Factor` (levels `Vec<String>` +
1-based codes `Vec<Int32>`, code 0 = missing; `""` constructs missing,
`as_strings` renders missing as `"NA"`), `factor_from_strings`,
`levels`/`codes`/`as_strings`/`relevel`/`reorder_by_count`/`droplevels`/
`clone`, printable, bare-`Factor` annotations, `std::factor::factor_counts`
(table() semantics, NA excluded). 4 fixtures, 165/165 green, ASan-clean.
New builtin generic `Factor { levels: Vec<String>,
codes: Vec<Int32> }` (level 0 = missing). Constructors
`factor_from_strings`, `relevel`, `reorder_by_count`, `droplevels`,
`as_strings`, group-by integration so frames can group on factors with
level order preserved. Layouts + drop/clone in codegen; everything else
pure Valvet. Fixtures vs R `factor()` semantics on level ordering and NA
handling.
**F2. Strings.** SHIPPED Phase 1: three `String` byte primitives (`len`,
`byte_at` with OOB trap, `slice` with OOB trap) plus `std::str` verbs
(`str_contains`, `str_starts_with`/`str_ends_with`, literal `str_replace`,
`str_substring`), all differential-tested vs Python (empty-needle
interleave included). Also fixed a latent codegen double-free: moved call
args in statement-`if` conditions (flush_all_nulls drained poisons needed
by merge paths; now non-draining). 2 fixtures, 167/167 green, ASan-clean.
Phase 1 (stdlib, no compiler change): `contains`,
`starts_with`/`ends_with`, `replace` (literal), `substring` by byte
range with trap, `split` exists already. Phase 2: regex via Thompson NFA
over explicit tables (first-order, proven portable by the Parsec port):
`re_detect`, `re_extract`, restricted syntax
(`literals . * + ? [] () | ^ $`), leftmost-first match semantics pinned
by fixtures.
**F3. Dates.** SHIPPED as pure stdlib (`std::date`, no compiler change):
`Date` (days since epoch) and `Datetime` (UTC nanos) over Hinnant civil
algorithms with floored division (pre-1970 works); strict `YYYY-MM-DD`
and `YYYY-MM-DDTHH:MM:SSZ` parse/format, validated `Option` constructors,
day/second arithmetic, compares, and date bridges. Differential-tested
vs Python `datetime` (leap rules, epoch boundaries, cross-midnight
arithmetic). 2 fixtures, 169/169 green, ASan-clean. New `Date` (days since epoch, Int32) and `Datetime`
(nanos, Int64), UTC only (timezones explicitly deferred).
Parsing/formatting for fixed formats (`YYYY-MM-DD`, RFC3330 subset) in
pure Valvet over Int64 via Howard Hinnant's civil algorithms;
arithmetic (`date + days`, `date - date` in days), ordering, printing.
Fixtures vs Python `datetime` including leap years and epoch boundaries.

## P2: verbs, plots, distributions (needs P0 sort only)

**D1. dplyr verbs + pipe.** SHIPPED: `|>` lowers in the parser to
`f(x, args)` (bare `x |> f` to `f(x)`); verbs as move-by-value frame funs
in `std::frame` (`frame_arrange` numeric-aware via M2 `order`,
`frame_mutate`/`frame_summarise` over a flat `ColExpr` ADT: `Col`, `Lit`,
`Count`, `Add/Sub/Mul/Div`, `MeanOf/SumOf/MinOf/MaxOf`; missing columns
follow the silent-empty precedent, unparseable cells are NaN-skipped like
`frame_group_sum`). ColExpr is flat (no Box exists, so nesting goes
through intermediate columns). 2 fixtures vs pure-Python oracles,
171/171 green, ASan-clean. New `|>` operator (pure sugar: `x |> f(y)`
lowers to `f(x, y)`). Verbs as frame methods (builder chaining needs no
closures): `arrange(col, dir)` on M2, `mutate`/`summarise` over a
**`ColExpr` ADT** instead of closures: first-order combinators
(`col("x")`, `lit(1.0)`, `add(a,b)`, `mean_of(col)`) evaluated by a
small interpreter over columns. Custom named-function aggregations
arrive with Stage 1 closures.
```valvet
val g = frame_group_sum(&f, "region", "sales");
val top = g |> frame_arrange("sum", false);
```
**D2. Plot grammar.** Not a ggplot clone: composable owned `Plot`
builder. `Plot` holds data + layer list (layers are DATA: enum of geom
specs with column-name mappings, never functions); `+` chains builders;
scales (`scale_log_x`) and `facet_wrap(col)` included; one SVG render at
the end. Builds on existing plot.valvet geoms.
**D3. Distributions.** New `std::dist`: Normal/Exponential/Uniform/
Binomial/Poisson with `pdf/pmf/cdf/quantile/sample`. Normal CDF via
Abramowitz-Stegun erf, quantile via Acklam's rational approximation,
sampling via the already-ported Marsaglia-Bray; discrete ones by
inversion over the seeded `Rng`. Pure Valvet. Fixtures vs Python
`statistics`/`math` to 1e-9.

## P3: modeling and testing (needs M1 + D3)

**S1. Formulas + `lm`/`glm`.** Formula as data: `Formula { response:
String, terms: Vec<Term> }` built by constructor fns (and later parsed
from `"y ~ x1 + x2"` strings with the existing split machinery). `lm`
via Cholesky on `X'X` (falls back to `lu_solve`), `glm_binomial` via
IRLS reusing the hand-built logistic pattern. `summary` returns a struct
(coefficients, std errors from the covariance diagonal, z, p via D3
normal CDF). Fixtures vs R's `cars`/Challenger reference values already
in the repo.
**S2. Hypothesis tests.** `t_test` (Student-t CDF via incomplete beta
continued fractions), `chisq_test` (gamma via Lanczos + series/CF),
`cor_test` on the existing `cor`. Rank-based Mann-Whitney as the
no-special-functions fallback if beta/gamma slip. Fixtures vs R
`t.test`/`chisq.test` outputs.

## P4: functionals (needs closure Stage 1)

**C1. `map/filter/fold`.** With function items as values: `map(v, f)`,
`filter(v, pred)`, `fold(v, init, f)` over `Vec`, plus `split_apply`
(group indices + named combine fn) covering the honest core of
`tapply`. Capturing closures (Stage 2) only if real ports demand it.

## Order and verification

P0 → P1 + D2/D3 in parallel → D1 → S1/S2 → C1. Every item ships with
`.stdout`-pinned fixtures plus Python oracles (bit-exact where +-*/
only, 1e-9 where transcendental), ASan/UBSan sweep of new C, and README
+ site docs updates like the 0.2.0 round. Biggest risks, honestly
stated: Jacobi eigen/SVD scope creep (cap at symmetric Jacobi +
one-sided Jacobi, defer the rest), t CDF edge cases near df extremes
(pin against R values, widen tolerance only with evidence), and closure
Stage 2 ballooning (time-box it; Stage 1 plus ColExpr may suffice).
