# Plan: 0.2.0 Analytical Foundation

Basis: `valvet-design/` D6 (stdlib) + D7 (data science) + D9 §1 (paths) +
D13 decisions; academic grounding below; lessons from the translation
sessions (`valvet-r-cars/LOG.md`, `valvet-papers/LOG.md`, `BUGHUNT.md`).

Dependency order: J0 → J1 → J2 → J3 → J4 → J5 → J6 → J7 (each lands green;
gates per phase: `fmt --check`, `clippy -D warnings`, `cargo test`,
`valvet test` debug+release, ASan/UBSan on new C paths).

## Academic grounding

- Wickham (2014), "Tidy Data", JSS 59(10): variables are columns,
  observations are rows, values are cells. Grounds Frame orientation,
  deterministic group order, and Wickham-style messy→tidy conformance tests.
- Rubin (1976), "Inference and Missing Data", Biometrika 63(3): MAR and
  ignorability. Grounds `Missing<T>` as distinct from `Option`/`NaN`, with
  explicit `Skip`/`Propagate` reduction policies.
- Blackman & Vigna (2021), ACM TOMS 47(4): scrambled linear generators.
  Grounds the xoshiro256++ default and seeded-determinism gates.
- Satyanarayan et al. (2017), IEEE TVCG: Vega-Lite grammar of graphics.
  Grounds the plot builder, restricted here to static SVG (no interaction
  grammar, no `show` server).

## Lessons carried forward (translation evidence)

- L1 Math first: translators hand-rolled Newton-sqrt (iteration counts are
  scale-dependent: 20 steps failed at ~1e11) and Taylor-exp (needs the
  negative-argument reciprocal). D6/D7 assume math but never list it.
- L2 No indexed assignment: PageRank rebuilt vectors O(n·m) via `push`.
  Frames need `v[i] = x` (place + borrowck write-conflict + codegen).
- L3 Single-value output: summaries print label/value line pairs; no
  number→string conversion. Needed for tables, CSV writing, plot labels.
- L4 File I/O was the top friction (datasets hardcoded). CSV-first ordering
  stands; row-attributed `CsvError` is an explicit deliverable.
- L5 RNG unlocks the untranslated parts (Dalal et al. bootstrap CIs).
  Seeded determinism must be gated: translations were deterministic by
  construction, RNG is the first nondeterminism risk.
- L6 `Missing<T>` ≠ `Option` confirmed by use (explicit policies, CSV
  `NaSpec` of `""`/`NA`/`null`).
- L7 Nested-`Vec` values are unverified ground: ASan spike must prove
  composite-element push/clone/drop before frames rest on them (fallback:
  schema-typed `Frame<S>` columns only).
- L8 Non-separable training loops rely on iteration caps to terminate
  (perceptron/XOR precedent); any 0.2.0 iterative fitter needs the same
  cap + convergence reporting.

## Standing constraints (from 0.1.0 scope)

- No closures, no traits, no user generics in 0.2.0 (COMPILER_PLAN
  non-goals). Consequence: the Frame API is closure-free: named
  aggregations, column selection by name, boolean-mask filtering
  (no predicate closures, no `{ r -> ... }` lambdas; those wait for 1.0+).
- No file I/O exists yet: `std::fs::read_to_string` + `readCsv`/`readJson`
  are builtins backed by the C runtime (same precedent as `Vec` methods).

## Phase J0: `std::math` + integer semantics (new; nothing in D6/D7)

- Spec: `sqrt/exp/log/pow` on Float64, tolerance-based iteration (never
  fixed counts; see L1), range reduction for exp/log, `%.17g`-compatible float
  formatting helpers for conversions (feeds J1).
- `cc` driver gains `-fwrapv`: integer arithmetic wraps (two's complement).
  Rationale: xoshiro (J5) requires wrapping; today overflow is C UB.
  Only previously-UB cases change meaning.
- Decision (early, recorded in D13): stdlib ships as Valvet sources under
  `stdlib/` (`module std.math;`, …), imported explicitly
  (`import std.math::sqrt;`), resolved from the binary's install dir with
  `VALVET_STDLIB` override. No implicit prelude (avoids name-clash
  semantics). Pure-Valvet where possible (J0 math is); runtime-opaque
  state (J5 `Rng`) stays builtin-backed like `Vec` methods.
- Files: `stdlib/math.valvet`, `codegen.rs` (`-fwrapv`), sema arity/type
  checks for new builtins if any.
- Tests: property tests vs Python `math` (tolerances, special values
  incl. inf/nan, identity `exp(log(x))==x`); fixtures `math_sqrt`,
  `math_exp_log`.
- Done when: 0.1.0 gates still green + new tests green + ASan clean.

## Phase J1: language: index assign, conversions, strings (new)

- `v[i] = x` for `Vec<T>` (L2): parser place target, sema
  (bounds are runtime `None`-style? No: out-of-bounds is a panic-free
  `E0401`-class static rule where provable, else defined trap with row/col
  diagnostic; decide in D13), ownership write-conflict vs live loans,
  codegen lvalue store with drop-old discipline.
- Numeric `to_string` (L3): `Int32/Int64/Float64/Bool` → `String`
  (round-trip with J0 formatting); `println` stays unary.
- String methods for J2: `split`, `trim`, `parse_int`/`parse_float`
  (returning `Result<.., ParseError>` / `Option`).
- Tests: `index_assign`, `index_assign_borrow_conflict` (E0302),
  `tostring_roundtrip`, `split_parse`; ASan on overwrite-then-drop paths.
- Done when: pagerank-style rebuild loops expressible with `v[i] = x`.

## Phase J2: CSV/JSON import

- Spec (D7 §5, as written): `readCsv(path): Result<DynamicFrame, CsvError>`,
  `readCsvTyped`, `NaSpec`, `strict` vs `lenient{max_errors}`, schema
  inference (N=1000 rows, Int→Float→String promotion).
- Deliverable: row-attributed diagnostics (L4): every `CsvError` names
  file + row; malformed-row tests with exact positions.
- Tests: `csv_basic`, `csv_typed`, `csv_lenient_errors`, `csv_na_spec`,
  `json_basic`; fixtures carry their `.csv`/`.json` inputs beside them
  (`multi-file/`-style project dirs).
- Done when: R `cars` dataset loads from CSV and reproduces the 0.1.0
  hardcoded OLS (16/16), deleting the hardcoding, same numbers.

## Phase J3: Frame/DynamicFrame (eager)

- Spec (D7 §4, as written): columnar storage, `Frame<S>` + `DynamicFrame`,
  `clone` (Arc-share + `make_mut` per DD-07a), views, `summarise` with named
  agg args, deterministic (sorted) group order.
- Predecessor spike (L7): ASan proof of nested-composite element semantics
  first; restrict to flat columns if it fails.
- Closure-free API only (standing constraint): selection by name, masks.
- Tests: Wickham-style conformance (messy→tidy cases from the paper's
  examples); `melt`/`pivot` explicitly deferred to 0.3.0.
- Done when: group-by + summarise + filter-mask over a CSV-loaded frame,
  all ASan-clean.

## Phase J4: `Missing<T>` + Missing-aware stats

- Spec (D7 sections 2, 3 and 6, as written): `Missing::Present/Missing`, `NA` const,
  `MissingPolicy::{Skip, Propagate}`, Kahan/two-pass options,
  `mean/sum_v/median/min_v/max_v/sd/var/quantile/cor/cov` over
  `&[Missing<Float64>]`, `n_missing`, `Option`-yielding empties.
- Tests: Skip vs Propagate matrices, all-missing columns, empty inputs,
  `Missing == Missing` (differs from NaN), sort-first ordering, `"NA"`.
- Done when: stats on a gappy CSV column match a Python reference
  (pandas-style Skip semantics) to float tolerance.

## Phase J5: RNG + sampling

- Spec (D7 §8, as written): `Rng::seeded(u64)`, xoshiro256++,
  `next_f64` in [0,1), `sample`, `shuffle(&mut Rng, &mut Vec<T>)`,
  no global RNG. Internals opaque to Valvet code (builtin-backed; see J0
  decision); `-fwrapv` makes the arithmetic defined.
- Gates: seed determinism (`tests/run/rng-seeded` style: two runs, same
  stream, byte-identical), distribution smoke (mean/variance of 1e5 draws
  within tolerance), ASan over shuffle aliasing paths.
- Conformance: the Challenger parametric bootstrap (the part of Dalal et
  al. that 0.1.0 could not express) with fixed seed vs Python reference.
- Done when: bootstrap CIs reproduce to tolerance with a fixed seed.

## Phase J6: static-SVG plot builder (revised down)

- Vega-Lite grounding restricted: marks + x/y/color/size encodings,
  `title`, `save("o.svg")`; NO interaction grammar, NO `show` server,
  NO faceting beyond single views (those follow the grammar paper's
  composition algebra in 0.3.0+).
- Tests: golden SVG files for scatter/bar/line over a CSV-loaded frame
  (byte-exact), `PlotError` paths (bad column, empty frame).
- Done when: cars speed-vs-dist scatterplot renders from a three-line
  Valvet program.

## Phase J7: REPL (unchanged, last)

- Needs an incrementality spike first (0.1.0 is whole-program; REPL keeps
  a persistent env across snippets). Line editing via rustyline-class
  crate is an offline-vendoring question; spike decides, recorded in D13.
- Tests: scripted session fixtures (multi-line input, `:type`-style
  introspection if shipped, error recovery without dropping the env).
- Done when: a scripted REPL session defines, runs, and redefines across
  three prompts without restart.

## Deltas vs the old 0.2.0 line (explicit)

- Added: J0 math + `-fwrapv` + stdlib-source decision (was assumed, never
  specified); J1 index-assign/conversions/string methods (was assumed by
  Frame/RNG designs); nesting ASan spike; determinism gates; row-attributed
  CSV errors; REPL incrementality spike.
- Revised down: plots to static SVG (was: full builder with `show`).
- Deferred with evidence: `melt`/`pivot` (0.3.0), closures/lambdas in Frame
  APIs (1.0+), LazyFrame/Parquet/SQL (stay 0.3.0), interaction grammar (1.0+).
- Unchanged: Missing design, Frame model, stats signatures, RNG API,
  strict/lenient CSV, NaSpec, REPL placement last.
