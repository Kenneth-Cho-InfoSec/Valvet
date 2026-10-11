# Valvet Compiler — Implementation Plan (revamped 2026-10-10)

Spec authority: `/home/kenneth/valvet-design/` (D1–D15 + `grammar/valvet.ebnf` + `revision/`).
All design docs + grammar + D10 examples + contradiction register inspected in full.

## Version roadmap (revamped)

- 0.1.0 — Core MVP + hardening + language completion (complete). Shipped: lexer/parser, primitive types, `val`/`var`, functions/calls/`print` builtin, arithmetic/comparison/boolean ops, `if`/`else`, `while`, blocks + tail expr, diverging-block coercion, String move/borrow subset, C11+GCC native backend, CLI (`check/build/run/test`), 100 unit + 93 fixture tests green (debug+release), ASan/UBSan-clean output. Also shipped: fuzz-soup harness, `match` (+ exhaustiveness E0204), `Option`/`Result` + `?`, `for`/`loop`/`break`/`continue`, `as` casts, `Vec` + methods, structs/enums, `&T`/`&mut T` + NLL borrowck, multi-file modules/imports, `vec!`, `println` (`print` retired to a W1002 alias). Next: 0.2.0.
- 0.2.0 — Analytical foundation (design Stage B, old 0.4.0). `Missing`/`NA`, `Frame`/`DynamicFrame` eager ops, CSV/JSON import, `std::stats` core + seeded RNG, plot builder, REPL.
- 0.3.0 — Production (design Stage C, old 0.5.0). Lazy query plans, Parquet/Arrow/SQL, parallel iterators, FFI/`unsafe`, BLAS hook, package registry + lockfile, LSP.
- 1.0 + — Ecosystem (design Stage D). Advanced models, async, extra backends (LLVM/Cranelift to replace the C backend), R/Python bridges. 1.0 = stability commitment per the design doc's evolvability principle.

## 0.1.0 remaining scope (merged old 0.2.0 + old 0.3.0) — feature → spec → component

| Feature | Spec | Component | Status |
|---|---|---|---|
| `match` + arm guards + exhaustiveness (E0204) | D2 §4, D5 | parser/sema/codegen | Tested |
| `Option`/`Result`, `Some/None/Ok/Err` patterns, `if let` | D3 §7, D5 | parser/sema | Tested |
| `?` propagation (T-Prop: Result-in-Result, Option-in-Option) | D3 §2 | sema/codegen | Tested |
| `for (x in e)`, `loop`, `break`/`continue` (+ value) | D2 §4, D5 | parser/sema/codegen | Tested |
| `as` casts (wrap/truncate/saturate table; `try_as` deferred) | D3 §1 | sema/codegen | Tested |
| `Vec<T>` (`len/push/pop/get/clone/cast/add_scalar/add/mul/dot`), `vec!` builtin macro (`slice/mask/filter/map` deferred) | D7 §3, D5 | sema/codegen | Tested |
| Struct/enum decl + literals + field access + codegen | D2 §5 | parser/sema/ownership/codegen | Tested |
| References `&T`/`&mut T`, lifetimes (elided + explicit where required), NLL borrowck | D4 §2, D5 | sema/ownership | Tested |
| Multi-file modules/imports (`::` paths, E0601/E0602/E0701) | D9 §1 | resolver/sema | Tested |
| `println` stdlib, retire `print` builtin | D6 | stdlib/sema | Tested |
| Fuzzing (lexer/parser/pipeline) | TEST_STRATEGY | infra | Tested |

## Non-goals for 0.2.0
Generics, closures, traits/impl codegen, `defer`, async, data frames/stats (0.3.0), optimisation passes, cross-compilation.

## Standing decisions (unchanged)
- Backend: C11 emission + user-local GCC 16; semantics backend-independent (ARCHITECTURE.md ADR-001).
- Val-immutable violation E0205, unknown name E0401, missing main E0603 (SPECIFICATION_GAPS.md).
- Deferred features rejected with E0800, never silently accepted.
