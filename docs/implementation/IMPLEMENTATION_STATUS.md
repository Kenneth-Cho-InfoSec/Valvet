# Implementation Status — Valvet Core MVP 0.1.0 (2026-10-10)

Toolchain: rustc 1.99.0 + user-local GCC 16 (`~/.local/vgcc`, system binutils/ld/glibc).
Backend: C11 emission + GCC link (ADR-001, ARCHITECTURE.md). No LLVM/Cranelift in env.

## Phase results (all commands actually executed)
- Phase 0 audit/plan: done. Specs fully read (D1–D15, EBNF, revision register with 26 resolved VR items). Gaps recorded (E0205 val-reassign, E0401 unknown name, E0603 missing main, `print` builtin temp, String `==`).
- Phase 1 workspace/CLI/diags/harness/CI: done. `cargo build --workspace` clean.
- Phase 2 lexer/parser: done. Unit-tested; recovery never stalls (progress guarantee + NL lookahead).
- Phase 3 name/type checking: done. 18-fixture type suite green.
- Phase 4 ownership: done. Move/borrow subset (String moves, E0301, print/concat borrow) + 4 regression tests.
- Phase 5 typed IR (checked AST + type table) + native backend: done. ASan+UBSan clean on all 8 pass fixtures with byte-exact outputs.
- Phase 6 CLI: done. check/build/run/test/version/help; exit 0/1/2; run passes through program status.
- Phase 7 hardening: done. fmt clean, clippy `-D warnings` clean, debug+release green, robustness (5000-deep expr, 2000-statement file, unclosed/garbage inputs → controlled E0005, no hangs).
- Phase 8 expansion: done as 0.1.0 phases A–I (see completion progress below).

## Test totals
- `cargo test --workspace`: 100 passed (98 front-end + 2 fuzz), 0 failed.
- `valvet test valvet-tests`: 93/93 debug, 93/93 release.
- ASan/UBSan on generated C: clean on the full pass sweep, the multi-file
  projects, and nested composites (borrow/String/Vec/Option/Result paths),
  outputs byte-exact.
- MVP acceptance §12: tests 1–7 all verified (7 = independent native exe printing 42).

## 0.1.0 completion progress (PLAN-0.1.0-REMAINING.md)
- DONE: Phases A–I (0.1.0 language completion finished) (A: for/loop/break/continue + break-values, `as` casts + saturating helpers, fuzz-soup harness; B–F per prior entries; G: references/NLL borrowck).
- REMAINING: none in 0.1.0 (0.2.0 analytical foundation next).

## Feature matrix (revamped numbering 2026-10-10)
0.1.0 = MVP shipped + language completion implemented and tested (match, Option/Result/?, for/loop/break, as-casts, Vec/vec!, structs/enums, &/&mut borrowck, modules/imports, println, fuzzing). 0.2.0 = analytical foundation (Missing/NA, frames, CSV/JSON, stats+RNG, plots, REPL). 0.3.0 = production (lazy plans, Parquet/Arrow/SQL, par_iter, FFI/unsafe, BLAS, registry, LSP). 1.0+ = ecosystem + stability.
Partially: none (all 0.1.0 items complete).
Specified but not implemented (all E0800, documented): everything above 0.1.0.
Blocked: none. Unresolved: backend swap (LLVM/Cranelift) future (1.0).

## Known limitations (honest)
- Multi-file programs via entry-dir imports (`module a.b;` + `import a::b::x`); entry file needs no `module` decl.
- `println` is the output builtin (primitives + `Vec`/`Option`/`Result`); `print` is a deprecated alias (W1002).
- Statement-`if` without `else` as a value still requires `else` (spec rule, tested).
- No cross-compilation; target = x86_64 Linux dev machine.
- No git repo initialized in this environment (CI workflow provided for hosting).

## Next task
0.1.0 complete. Next: 0.2.0 analytical foundation per COMPILER_PLAN.md. Blocker: none.

## Post-0.1.0 codegen fixes (found via an R `cars` translation, 2026-10-11)
- Fn-body Copy tails materialize before scope frees (was: silent `inf`/wrong
  reads of owned locals). Fixture: `tail_owned_read.valvet`.
- `mat_str` no longer wraps deref lvalues in droppable temps (was: double-free
  on `(*v).len()`). Same fixture.
- One codegen-shape test updated (`codegen_int_main_returns`: tails now
  `return _vt`). Gates re-green: 100 unit (98+2), 91/91 fixtures debug+release,
  fmt/clippy clean, ASan clean incl. nested composites.
- All-Copy `Option`/`Result` mains linked a never-generated `v_drop_*`
  helper (wrappers only covered owned payloads). Fix: gate the wrapper drop
  on `type_needs_drop`. Fixtures: `try_main_option_copy` (exit 3),
  `try_main_result_copy` (exit 7). Gates re-green: 100 unit, 93/93 fixtures
  debug+release, fmt/clippy clean, ASan clean.

## Known limitations (appended 2026-10-11, Phase H)
- Qualified call paths (`m::f()`) are not parsed (import names into scope instead).
- Statement-`match` with `print()` tails miscompiles (pre-existing single-file bug, independent of H).
- Explicit `ShapeError` annotations only resolve in the entry (internal type).
- Adversarial `__`-laden type names could collide with qualifier sanitizing (documented, ignored).
