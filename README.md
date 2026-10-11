# Valvet

Valvet is a small, statically typed, compiled programming language. No
garbage collector: owned values live in exactly one slot and are freed
deterministically at scope exit. The 0.1.0 core language is complete;
data-analysis foundations (frames, CSV/JSON, stats) come in 0.2.0.

Status: **0.1.0 implemented and tested** — 100 unit tests, 91/91 fixture
programs green in debug and release, generated C ASan/UBSan-clean.

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

Multi-file projects — root is the entry file's directory:

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

Errors never panic the compiler — they carry file, position, code, and help:

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

## Scope notes

- Single C translation unit per program; target is the local machine.
- No file I/O, closures, traits, generics beyond the builtins, or
  `Display` for user structs yet — these are 0.2.0+ scope.
- `docs/implementation/SPECIFICATION_GAPS.md` records resolved ambiguities.
