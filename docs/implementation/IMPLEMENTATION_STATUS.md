# Implementation Status: Valvet 0.5.0 local tree (2026-10-26)

Toolchain: rustc 1.99.0 + user-local GCC 16 (`~/.local/vgcc`, system binutils/ld/glibc).
Backend: C11 emission + GCC link (ADR-001, ARCHITECTURE.md). No LLVM/Cranelift in env.

## Test totals (0.5.0 local tree)
- `cargo test --workspace`: 223 front-end, 9 codegen FP, 6 core unit,
  5 CLI security, and 3 fuzz tests passed, 0 failed (246 total).
- `valvet test valvet-tests`: 432/432 through the debug driver and 432/432
  through the release driver. Generated C uses release flags either way.
- `valvet test --doc`: 17/17 through both drivers.
- `scripts/fuzz_binary_formats.py`: 266 deterministic mutation cases,
  0 crashes or hangs.
- ASan/UBSan/LSan: clean on 88 typed-frame, grouped, join, window, Parquet,
  Arrow, bytes, Snappy, and lazy-plan fixtures, with stdout identical to
  each `.stdout` sidecar. Clean on the earlier `string_index_*`,
  `string_nul_trigraph`, `mat_basic`, and `mat_linalg` set after the
  security fixes.
- `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets -- -D warnings`: pass.

## Security remediation (0.5.0 local tree)
The seven confirmed findings in
`docs/security/PRELIMINARY_SECURITY_ASSESSMENT.md` are fixed:
`--target` no longer uses a shell and validates its triple; matrix shapes
use checked multiplication, checked allocation, and a `1 << 28` element
cap; the parser has a depth budget and reports `E0006`; `--hermetic` denies
`read_file`, `read_file_limit`, `write_file`, and `sys_cwd`; `std::` imports
are pinned to stdlib roots unless `--allow-stdlib-shadow` is explicit; and C
string literals escape trigraphs and preserve embedded NUL bytes through
`vstr_from_bytes`. The Parquet and Arrow readers use the bounded
`read_file_limit` and range-checked `bytes_to_string` builtins. `--hermetic`
remains a compile-time policy gate, not an OS sandbox.

The broader VLV-008 through VLV-015 items in the security assessment are
candidate, process, or design risks rather than confirmed findings in this
release. They remain open follow-ups: URL and path validation for future
package tooling, lockfile enforcement, trusted-toolchain pinning, CI
sanitizer and fuzz jobs, and OS-level sandboxing for untrusted programs.

## StringIndex (0.5.0 local)
`StringIndex` is an ordered string-keyed index with `Int64` positions. The API
is `StringIndex::new()`, `len`, `contains`, `get -> Option<Int64>`,
`get_or_insert -> Int64`, `keys -> Vec<String>`, and deep `clone`. Keys are
unique and keep first-appearance order. The index is move-only and uses an
internal open-addressed table with cached FNV-1a hashes, power-of-two bucket
capacity, and a load factor of at most 0.5. It is not a general `Map<K,V>`:
deletion, non-string keys, map literals, `idx[key]`, printing the index itself,
and join multimaps remain out of scope. No measured speedup is claimed until
the stdlib migrates to it.

## Typed frames, binary formats, and lazy plans (0.5.0)
All six phases in `docs/implementation/PLAN-TYPED-FRAMES.md` are implemented
in the working tree:

- P1: `TypedFrame`, `Column`, `ColBuild`, strict/lenient typed CSV, typed
  JSON, schema inference, typed stats, `tf_describe`, and
  `DynamicFrame` round trips.
- P2: multi-key grouping with first-appearance order and a missing-key
  group, plus count, count non-missing, missing count, sum, mean, min, max,
  variance, and standard deviation aggregations.
- P3: inner, left, right, and full typed joins with missing keys never
  matching by default, cartesian duplicate keys, deterministic name
  suffixes, Factor keys, and an audit summary.
- P4: grouped lag, lead, row number, cumulative sum, and rank windows that
  preserve original row order.
- P5: bounded Parquet and Arrow readers, plus one-row-group Parquet and
  one-record-batch Arrow writers. Readers accept the documented primitive
  subsets and reject nested, dictionary-encoded, compressed Arrow, and
  malformed inputs with `Result` errors. Factor columns are written as
  strings.
- P6: flat `Vec<PlanStep>` lazy plans with select, filter, summarise, join
  to a materialized frame, arrange, limit, collect, optimize, and explain.
  Lazy-lazy joins are deferred.

The existing `DynamicFrame` SQL, plotting, validation, and model paths are
unchanged. `StringIndex` is used only for local grouping positions, not as
join storage. Typed columns use `Vec<Missing<T>>` in v1; no validity-mask
layout is claimed.

## Test totals (0.3.0)
- `cargo test --workspace`: 185 front-end passed, plus 2 fuzz-soup, 0 failed.
- `valvet test valvet-tests`: 258/258 debug, 258/258 release (clean tree).
- ASan/UBSan on generated C: clean on threaded/popen/decoder paths and new
  stdlib paths (SQL, dist extras, wrangling, selection, causal), outputs
  byte-exact. Every numeric claim oracled against Python/R at 1e-12
  relative (p-values 1e-10 absolute).
- Known issues (tracked): calls directly under `||`/`&&` LHS need
  bind-first (ASan double-free); `V[i].push(x)` on nested vecs silently
  drops (use clone-push-assign).

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

## Test totals (0.1.0 baseline, superseded by the 0.3.0 totals above)
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
0.1.0 = MVP shipped + language completion implemented and tested (match, Option/Result/?, for/loop/break, as-casts, Vec/vec!, structs/enums, &/&mut borrowck, modules/imports, println, fuzzing). 0.2.0 = analytical foundation (Missing/NA, frames, CSV/JSON, stats+RNG, plots, REPL). 0.3.0 = production (lazy plans, Parquet/Arrow/SQL, par_iter, FFI/unsafe, BLAS, registry, LSP). 0.5.0 local tree = 0.3.0 baseline plus `StringIndex`, `ArrayD<T>`, `Array<T, N>`, multi-index `Mat` indexing, typed frames with multi-key grouping, typed joins and windows, narrow Parquet/Arrow readers and writers, and flat lazy plans. 1.0+ = ecosystem + stability.
Partially: see `09_completion_status.md` for the current task matrix.
Specified but not implemented: remaining open tasks in `06_actionable_task_list.md`.
Blocked: see the external blockers in `09_completion_status.md`. Unresolved: backend swap (LLVM/Cranelift) future (1.0).

## Known limitations (honest)
- Multi-file programs via entry-dir imports (`module a.b;` + `import a::b::x`); entry file needs no `module` decl.
- `println` is the output builtin (primitives + `Vec`/`Option`/`Result`); `print` is a deprecated alias (W1002).
- Statement-`if` without `else` as a value still requires `else` (spec rule, tested).
- No cross-compilation; target = x86_64 Linux dev machine.
- The checkout is a local git repository, but no CI runner or hosted workflow
  is configured in this environment.

## Next task
The six typed-frame phases are complete in the working tree. Remaining
release follow-ups are VLV-008 through VLV-015 hardening, CI sanitizer and
fuzz jobs, and any further binary-format coverage. No blocker.

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
