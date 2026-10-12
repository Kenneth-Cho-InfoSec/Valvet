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
The 0.2.0 data layer (frames, CSV/JSON import, core stats) closes the
remaining gap between prototyping and production.

```valvet
fun mean(v: Vec<Float64>): Float64 {
    var total = 0.0
    for (x in v) {
        total += x
    }
    total / (v.len() as Float64)
}

fun main() {
    println(mean(vec![2.0, 4.0, 6.0, 8.0]))  // 5
}
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
fun main() {
    println("hello, valvet")
}
```

## Language tour

Functions, `if` expressions, loops, structs, vectors, `Option` matching:

```valvet
fun fib(n: Int32): Int32 {
    if (n < 2) { n } else { fib(n - 1) + fib(n - 2) }
}

struct Point { x: Float64, y: Float64 }

fun main() {
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
}
```

Types: `Int32`, `Int64`, `Float64`, `Bool`, `String`, `()` (Unit),
structs, enums, `Option<T>`, `Result<T, E>`, `Vec<T>`, and references
(`&T`, `&mut T`) with borrow checking. `println` handles primitives plus
`Vec`/`Option`/`Result` of printable types (`[1, 2]`, `Some(7)`/`None`,
`Ok(1)`/`Err(e)`); `print` remains as a deprecated alias (warns `W1002`).

Units of measure: annotate numeric bindings, parameters, and returns with
`@unit`. Same dimensions unify, scales convert silently (left side wins),
and `*`/`/` compose dimensions:

```valvet
fun speed(d: Float64 @m, t: Float64 @s): Float64 @m/s {
    d / t
}

fun main() {
    println(speed(100.0, 9.58))   // 10.438413361169102
    val v: Float64 @km/h = 72.0
    val w: Float64 @m/s = v       // auto-scales: 20
    println(w)
    println(w * 10.0)             // (m/s) * s = m: 200
}
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

Multi-file projects: the root is the entry file's directory:

```valvet
// main.valvet
import math::double;
fun main() {
    println(double(21))           // 42
}
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
| `valvet test [dir]` | Run the fixture suite (`valvet-tests` default) |
| `valvet new <name>` | Scaffold a single-dir project |
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

Pipeline: `.valvet` → tokens → AST → typed HIR → borrow-checked HIR →
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
Current gates: 100 unit tests, 93/93 fixtures (debug+release),
ASan/UBSan-clean output.

## Scope notes

- Single C translation unit per program; target is the local machine.
- No file I/O, closures, traits, generics beyond the builtins, or
  `Display` for user structs yet. Those are 0.2.0+ scope.
- `docs/implementation/SPECIFICATION_GAPS.md` records resolved ambiguities.
