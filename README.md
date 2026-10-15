# Valvet

<p align="center">
  <img src="docs/logo.png" width="250" alt="Valvet logo">
</p>

Valvet is a small, safe, compiled language for data work. It compiles to
native binaries through C11, with no garbage collector and no runtime:
owned values live in exactly one slot and are freed deterministically at
scope exit. It is for expressing numeric analyses, statistics,
iteration-heavy models, small simulations, directly and fast.

If your analyses currently live in an interpreted statistics environment,
Valvet is the compiled counterpart: the same small scripts for means,
models, and simulations, but checked before they run, freed
deterministically while they run, and shipped as dependency-free binaries.
The 0.3.0 modeling layer (formulas, `lm`/GLM, ANOVA, diagnostics,
contrasts, SQL over frames, validation contracts, causal and meta-analysis
helpers) closes the remaining gap between prototyping and production.

```valvet
fun mean(v: Vec<Float64>): Float64 {
    var total = 0.0
    for (x in v) {
        total += x
    }
    total / (v.len() as Float64)
}

println(mean(vec![2.0, 4.0, 6.0, 8.0]))  // 5
```

Vectors, loops, functions, floats, with borrow-checked memory safety.
The same core has reproduced textbook results end to end: ordinary least
squares (16/16 statistics match a reference implementation), logistic
regression on the Challenger O-ring data, PageRank power iteration, and
perceptron training, all identical to their references and ASan-clean.

## Quick start

Prerequisites: stable Rust and a C compiler (GCC 16 works out of the box;
`VALVET_CC` overrides detection).

```sh
cargo build --workspace          # debug compiler at target/debug/valvet
valvet run examples/hello.valvet
# hello, valvet
```

`examples/hello.valvet`:

```valvet
println("hello, valvet")
```

## Language tour

Script mode: the entry file runs top-level statements directly (imp is the
default; `mode imp;` states it explicitly, `mode exp;` opts back into the
strict classic rule). A shebang line works for direct execution:

```valvet
#!/usr/bin/env valvet
val x = 40 + 2;
println(x);
```

Top-level `return n;` sets the exit status; `?` propagates through a
synthesized `Result<Int32, String>` main whose message prints to stderr on
failure. Statements clash with `fun main` (E0604, entry-only like that).

Functions, `if` expressions, loops, structs, vectors, `Option` matching:

```valvet
fun fib(n: Int32): Int32 {
    if (n < 2) { n } else { fib(n - 1) + fib(n - 2) }
}

struct Point { x: Float64, y: Float64 }

println(fib(10))              // 55
var total = 0
for (i in 1..=5) {
    total += i                // 15
}
println(total)
val p = Point { x: 1.5, y: 2.5 }
println(p.x + p.y)            // 4
println(vec![1.0, 2.0, 3.0].len())  // 3
val o: Option<Int32> = Some(7)
println(match (o) { Some(x) -> x * 2, None -> 0, })  // 14
```

Types: `Int32`, `Int64`, `Float64`, `Bool`, `String`, `()` (Unit),
structs, enums, `Option<T>`, `Result<T, E>`, `Vec<T>`, `Mat`, `Factor`,
and references (`&T`, `&mut T`) with borrow checking. `println` handles primitives plus
`Vec`/`Option`/`Result`/`Mat`/`Factor` of printable types (`[1, 2]`, `Some(7)`/`None`,
`Ok(1)`/`Err(e)`); `print` remains as a deprecated alias (warns `W1002`).

Units of measure: annotate numeric bindings, parameters, and returns with
`@unit`. Same dimensions unify, scales convert silently (left side wins),
and `*`/`/` compose dimensions:

```valvet
fun speed(d: Float64 @m, t: Float64 @s): Float64 @m/s {
    d / t
}

println(speed(100.0, 9.58))   // 10.438413361169102
val v: Float64 @km/h = 72.0
val w: Float64 @m/s = v       // auto-scales: 20
println(w)
println(w * 10.0)             // (m/s) * s = m: 200
```

Dimensionless literals coerce (`d + 5.0` is meters); mismatched dimensions
are `E0201`, unknown units `E0401` (see `stdlib/units.valvet` for the known
units). `expr as @unit` asserts and converts explicitly.

Audited joins never silently drop rows: `frame_join_inner` / `frame_join_left`
return the frame plus its audit (matched / left-only / right-only counts),
and a missing key column is `Err`, not an empty frame:

```valvet
val s = match (frame_join_inner(&l, "id", &r, "id")) { Ok(j) -> audit_summary(&j.audit), Err(e) -> e, };
println(s)  // matched=1 left_only=0 right_only=0 out=1
```

Frames can carry provenance: source hash plus transform log, printable
with any result (`stdlib/provenance.valvet`):

```valvet
val p = provenance_of_text("demo", "a,b\n");
println(prov_report(&p))  // source=demo hash=6376667723524501508 ops=[]
```

Forward-mode autodiff via dual numbers (`stdlib/autodiff.valvet`): write the
loss once over `Dual`, seed each dimension, read `.d`. A logistic-regression
gradient descent in the suite is bit-identical to its Python reference:

```valvet
println(sq(seed(vec![3.0], 0 as Int64)).d)  // d(x^2)/dx at 3 = 6
```

Deterministic parallelism: `par_sum(v, nthreads)` sums `Vec<Float64>` on a
thread pool with fixed blocks and ordered combining, so every thread count
gives bit-identical results (a fixture pins 1/2/3/4/8-thread agreement).

R-parity track (0.3.0): dense `Mat` matrices with LU-backed kernels
(`matmul`, `lu_solve`, `det`, `inverse`, `cholesky`, symmetric Jacobi
`eigen_*`, one-sided Jacobi `svd_*`; shape errors are `Result`, never
traps), `Factor` levels/codes with level verbs, `|>` pipes and dplyr-style
frame verbs (`frame_arrange`, `frame_mutate`/`frame_summarise` over a flat
`ColExpr`), layered `Plot` specs with facets and log scales, `map`/`filter`/
`fold`/`split_apply` over named functions, and pure-Valvet stdlib for strings
(`std::str`, incl. Thompson-NFA `std::re`), dates (`std::date`), distributions
(`std::dist`), modeling (`std::model`: formulas, `lm`, binomial `glm`), and
hypothesis tests (`std::infer`: t, chi-square, correlation, Mann-Whitney,
Fisher exact, hypergeometric):

```valvet
import std::frame::DynamicFrame;
import std::model::formula_parse;
import std::model::formula_new;
import std::model::model_frame;
import std::model::ModelFrame;
import std::model::lm;
import std::model::LmFit;

fun no_mf(): ModelFrame {
    return ModelFrame { x: mat_identity(1 as Int64), y: vec![0.0], names: vec![""] };
}

fun main() {
    val f = DynamicFrame { names: vec!["x", "y"], cols: vec![vec!["1", "2", "3"], vec!["2", "4", "5"]], nrows: 3 as Int64 };
    val fo = match (formula_parse("y ~ x")) { Some(v) -> v, None -> formula_new("?", vec!["?"]), };
    val mf = match (model_frame(&f, fo)) { Ok(v) -> v, Err(_) -> no_mf(), };
    val fit = match (lm(mf)) { Ok(v) -> v, Err(_) -> LmFit { coef: vec![0.0], se: vec![0.0], fitted: vec![0.0], resid: vec![0.0], r2: 0.0, sigma: 0.0, names: vec![""] }, };
    println(fit.coef)   // [0.66666666666666663, 1.5]
    println(fit.r2)     // 0.96428571428571419
}
```

Statistics track (0.3.0): one-way and model ANOVA (`f_cdf`, `anova_oneway`,
`anova_lm`), printable model summaries, and regression diagnostics (Cook's
distance, VIF, Durbin-Watson, Breusch-Pagan), all oracled against Python.
Categorical predictors auto-expand to treatment contrasts, and `wlm` fits
weighted least squares through the same pipeline:

```valvet
import std::infer::anova_oneway;
val g1: Vec<Float64> = vec![1.0, 2.0, 3.0];
val g2: Vec<Float64> = vec![4.0, 5.0, 6.0];
let Some(a) = anova_oneway(vec![g1, g2]) else {
    println("need two groups");
    return 1;
};
println(a.f)  // 27
```

Query and validation (0.3.0): a SQL subset over frames (`std::sql`:
SELECT/WHERE/GROUP BY/ORDER BY/LIMIT planning down to frame verbs),
Pandera-shaped validation contracts (`std::validate`), window verbs
(`Lag`/`Lead`/`RowNum`/`Cumsum`/`Rank` on `ColExpr`), wrangling
(`frame_cut`/`frame_qcut`/`frame_crosstab`/pivots), model selection
(`std::select`: splits, k-fold ids, AIC/BIC/AICc, jackknife, permutation),
causal helpers (`std::causal`: DiD, Wald 2SLS, propensity matching, HC0
robust SE), meta-analysis and adjustments (`std::meta`), feature
engineering (`std::features`), random test frames (`std::synth`), and
LaTeX/HTML model tables (`std::report`):

```valvet
import std::sql::sql;
let Ok(q) = sql(&f, "SELECT g, SUM(x) FROM f GROUP BY g ORDER BY g") else {
    println("bad query");
    return 1;
};
println(q.cols[1 as Int64])  // per-group sums
```

Sources come in three equivalent extensions (`.valvet`, `.vv`, `.vvt`,
resolved in that preference order), and the CLI grows `run --watch`
(re-run on change) plus a whitespace-normalizing `fmt`:

Ownership stays exact throughout (no GC): owned values move by value
(`push` mutates in place; `get`/indexing/`for` clone out); pass `.clone()`
to keep a copy. Method receivers borrow; `&T` borrows support field reads
but stored `&`-borrows freeze reassignment for the scope, so stdlib threads
state with `q = f(q)` move-plus-reassign and clones at divergence points.

Multi-file projects: the root is the entry file's directory:

```valvet
// main.valvet
import math::double;
println(double(21))           // 42
```

```valvet
// math.valvet
module math;

pub fun double(n: Int32): Int32 {
    n * 2
}
```

Cross-module access requires `pub` (default-private). Imports use `::`
(dotted imports are rejected); cycles and missing modules are compile errors.

Errors never panic the compiler. They carry file, position, code, and help:

```
error[E0205]: cannot assign through this place (immutable binding or shared reference) --> prog.valvet:3:5
  help: use `var` bindings and `&mut` for mutation
```

## CLI

| Command | Effect |
|---|---|
| `valvet check <file>` | Parse and analyse, no linking |
| `valvet build <file>` | Compile to `target/valvet/<name>` |
| `valvet run <file>` | Build, execute, exit with program status |
| `valvet run <file> -- args` | Run with script arguments in `args` |
| `valvet run --watch <file>` | Re-run on file change |
| `valvet fmt <file>` | Normalize whitespace and trailing newline |
| `valvet test [dir]` | Run the fixture suite (`valvet-tests` default) |
| `valvet new <name>` | Scaffold a single-dir project |
| `valvet repl` | Interactive session (definitions persist) |
| `valvet -e 'expr'` | Evaluate a scriptlet and print its value |
| `valvet --version` / `--help` | Version / usage |

Exit codes: 0 success, 1 compilation failure, 2 usage error.

## Repository layout

```
crates/valvet_core/   lexer → parser → sema → ownership → C11 codegen
crates/valvet_cli/    check/build/run/test/new
valvet-tests/         fixture suite (compile-pass/fail, runtime, multi-file, …)
examples/             small runnable programs
docs/implementation/  plans, architecture (ADR-001), gaps, test strategy
```

Pipeline: sources (`.valvet`, `.vv`, `.vvt` — equivalent, resolved in that
preference order) → tokens → AST → typed HIR → borrow-checked HIR →
C11 → system `cc` → native binary. Semantics live in the front end;
codegen is a thin lowering (an LLVM/Cranelift backend could replace it
without touching typing or ownership).

## Testing

```sh
cargo test --workspace   # unit + integration + fuzz-soup (no-panic property)
cargo fmt --check && cargo clippy --workspace -- -D warnings
valvet test              # end-to-end fixtures, debug and release
```

See `docs/implementation/TEST_STRATEGY.md` for the gate policy and
`docs/implementation/IMPLEMENTATION_STATUS.md` for current totals.
Current gates: 185 unit tests, 258/258 fixtures (debug+release),
ASan/UBSan-clean output.

## Match guards (active-pattern style)

Named recognizer functions plus `if` guards on match arms partition data
the way F# active patterns do (no special syntax needed):

```valvet
fun is_even(n: Int64): Bool {
    n % (2 as Int64) == (0 as Int64)
}

match (v) {
    x if is_even(x) -> "even",
    x if x > (10 as Int64) -> "big",
    _ -> "other",
}
```

## Scope notes

- Single C translation unit per program; target is the local machine.
- File I/O (`read_file`/`write_file`), shell-out (`std::sys`, imp scripts
  only), plots (SVG), and `.vv`/`.vvt` sources all exist as of 0.3.0.
  Still deferred: closures, traits, generics beyond the builtins, user
  FFI, macros, lazy frames, and `Display` for user structs.
- `docs/implementation/SPECIFICATION_GAPS.md` records resolved ambiguities.
