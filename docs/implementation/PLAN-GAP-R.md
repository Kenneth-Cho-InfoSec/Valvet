# PLAN-GAP-R: R-parity gaps

Goal: close the R gaps worth closing under Valvet's constraints (first-order,
no closures/HOF values, no GC, explicit ownership, mode never changes type
meaning). Every item differential-tests against an R oracle (`Rscript`) or a
Python oracle where R is unavailable.

Conventions: `S(x)` = `crates/valvet_core/src/sema.rs`, `C(x)` = codegen.rs,
`P(x)` = parser.rs, `L(x)` = lexer.rs. Line numbers are 0.3.0-era anchors;
grep the quoted strings if they drifted.

Batch order: R1, R4a (seq, `%in%`) -> R2a (literal pinning) -> R5, R3 ->
R2b (recycling), R4b (outer/kronecker). R6/R7 are explicit non-goals.

---

## R1: `predict` for LmFit/GlmFit (stdlib only, no compiler change)

Gap: `stdlib/model.valvet` has `Formula` (`model.valvet:23`), `model_frame`
(`:78`), `lm` (`:188`), `lm_p`, `glm_binomial` (`:254`) but no prediction.

### Exact changes

1. `stdlib/model.valvet`, append after `lm_p` (~`:252`):
   - `pub fun predict_lm(fit: &LmFit, mf: ModelFrame): Result<Vec<Float64>, String>`
     recomputes `mf.x.mat_vec(fit.coef)` (same kernels `lm` uses,
     `model.valvet:195-208`); `Err` when `mf.x.cols() != fit.coef.len()`.
   - `pub fun predict_lm_frame(fit: &LmFit, f: &DynamicFrame, fo: Formula): Result<Vec<Float64>, String>`
     runs `model_frame` then `predict_lm` (mirrors `lm`'s own flow).
   - `pub fun predict_glm(fit: &GlmFit, mf: ModelFrame): Result<Vec<Float64>, String>`
     applies the sigmoid (`sigmoid`, `model.valvet:~250`) to the linear predictor.
   - All three follow the file's check-then-unwrap style (`match` + early
     `return Err`, provably-dead dummy arms), since `return` is rejected in
     match value-arms (see file header comment).
2. Fixtures: `valvet-tests/compile-pass/model_predict.valvet` + `.stdout`,
   differential-tested against `predict.lm` / `predict.glm` in R on the same
   CSV (tolerances: document float formatting `%.17g` vs R's shortest repr;
   compare with relative tolerance 1e-12, not text equality).
3. No core changes. No EBNF change.

Risks: none structural. `names` alignment between fit and new frame is by
position (document; R matches by name — note as limitation).

---

## R2a: literal pinning in mixed numeric binary ops (sema only)

Gap: verified `1 + (2 as Int64)` is E0201 (`S`, `"operator needs two equal
numeric types"` at `S:6485`/`S:6511`, help text `"convert explicitly; Valvet
has no implicit numeric conversions"`). Variables must never convert, but
unsuffixed literals can adopt (Rust precedent; sound; no mode change).

### Exact changes

1. `S`, numeric `BinOp::Add | Sub` arm (~`S:6470-6495`) and `BinOp::Mul |
   Div` arm (~`S:6495-6520`): when exactly one side is an unsuffixed
   integer/float literal (`Self::is_dimless_literal` exists — reuse it) and
   the other side is `Int32|Int64|Float64`, coerce the LITERAL side to the
   other's type instead of erroring. Suffixed literals (`1i32` → Cast node,
   Batch C1) keep erroring on mismatch (explicit means explicit).
   - Dimension behavior unchanged: pinned literal takes the other side's dim
     via the existing `set_dim` paths in those arms.
   - Update the help text on the remaining error to mention pinning:
     `"unsuffixed literals adopt the other side; suffixed literals and variables convert explicitly"`.
2. Fixtures: `arith_literal_pin.valvet` (`1 + (2 as Int64)`, `2.0 * (3 as
   Int64)`... note `2.0` Float64 vs Int64 still errors — pinning is
   int-literal-to-int/float and float-literal-to-float only; mixed
   int-literal with Float64 side pins to Float64, allowed). Reject fixtures:
   `1i32 + (2 as Int64)` still E0201; `x + y` mismatched vars still E0201.
3. Unit tests in `crates/valvet_core/tests/front_end.rs` for each case.

Risks: `is_dimless_literal` coverage (literals in parens/casts?) — read it
during implementation; restrict to direct literals on v1, document.

---

## R2b: explicit Vec recycling (sema + codegen methods)

Gap: `add`/`mul`/`add_scalar` are Vec methods (`S:4612-4670`); operands must
match exactly. R recycles length-1 (and warns on fractional recycling).

### Exact changes

1. `S`, `"add" | "mul"` arm (~`S:4641`): when element types agree but lengths
   are statically... lengths are runtime values, so this is a zero/static
   rule plus a runtime rule:
   - Static: keep the exact-type check as-is (no sema change needed for types).
   - Runtime: extend the elementwise C kernels (find the `vvec_*_add/mul`
     emitters near `C: emit_vec_fns`) with: equal lengths → loop; one side
     length 1 → broadcast it; else `fprintf` + `abort` (trap, matching the
     existing bounds-trap discipline; harness pins `.exit -1` for traps).
   - Scalar stays via `add_scalar` (no change).
2. No fractional recycling, ever (stricter than R, documented).
3. Fixtures: pass (equal, scalar-length) + trap fixture (`[1,2] + [1,2,3]`
   → abort, `.exit -1` sidecar) + Python oracle for values.

Risks: kernel lookup by instantiation — follow the existing `has_inst` /
`emit_vec_fns` pattern; audit every Vec kernel call site for the new length
contract (grep `vvec_.*_add\|vvec_.*_mul` in `C`).

---

## R3: formula depth — `*`, `:`, `I()`, `.` (stdlib only)

Gap: `Formula { response, terms }` (`model.valvet:23`) and `formula_parse`
(`:29`, splits on `~` then `+` only).

### Exact changes

1. `stdlib/model.valvet`:
   - Extend `Formula` with `interactions: Vec<(String, String)>` (for `*`
     and `:`) — keep `terms` for plain effects (backward compatible).
   - `formula_parse`: handle `a*b` → terms `[a, b]` + interaction `(a,b)`;
     `a:b` → interaction only; `I(expr)` → literal term (passed through,
     evaluated as a column expression — document limits); `.` → all columns
     except response (needs the frame: resolve at `model_frame` time, so
     keep `.` unexpanded in `Formula` and expand in `model_frame` via
     `frame_names`).
   - `model_frame`: append interaction columns (elementwise product of the
     two parsed columns, na.omit propagates: NaN in either drops the row).
2. Fixtures vs R oracle: `y ~ a*b`, `y ~ a:b`, `y ~ .` on a small CSV;
   compare coefficients at 1e-12 relative.
3. No core changes.

Risks: interaction of `.` with wholly-missing columns (existing `Err`
semantics apply).

---

## R4a: `seq()` (builtin, small)

Gap: `..` ranges work but stepped/length-out sequences don't.

### Exact changes

1. `S` `check_call`: new `seq` arm after `par_sum` (`S:6036` pattern):
   `seq(lo: Int, hi: Int, by: Int) -> Vec<Int64>` (Int32|Int64 accepted like
   `par_sum`'s thread count, `S:6062`). Arity exactly 3, E0201 otherwise.
   `by == 0` is a runtime trap (abort; document).
2. `C` Call chain: `vseq_wrap(lo, hi, by)` helper in `PRELUDE` (or next to
   the par helpers): allocates `Vec<Int64>` (record inst via
   `record_vec_inst(&Ty::Int64)` in sema, mirroring `S:4783`).
3. Fixtures: ascending, descending (negative step), empty (wrong direction
   → empty vec, R-compatible), trap on `by == 0` (`.exit -1`).

## R4b: `%in%`, outer, Kronecker (builtins, small)

1. `%in%`: `vec_in(v: Vec<T>, table: Vec<T>) -> Vec<Bool>`? Methods need a
   receiver type with methods; free builtins are simpler: `elem_in`? Name
   it `is_in(v, table)`. Sema: both args same `Vec<T>`, T scalar
   (Int32/Int64/Float64/Bool/String); record insts. Codegen + PRELUDE
   linear scan (document O(n*m); sorted fast path deferred).
2. `mat_outer(a: Vec<Float64>, b: Vec<Float64>) -> Mat` and
   `mat_kron(a: Mat, b: Mat) -> Mat` in the M1 section (`S` near
   `mat_zeros`, `C` near `vmat_zeros`): reuse `mat_ty()` recording and
   `vmat_*` allocation idioms.
3. Fixtures vs R oracle (`%in%`, `outer`, `kronecker`) at 1e-12.

---

## R5: schema-typed loading (stdlib only, no compiler change)

Gap: `infer_schema` exists but frame cells stay `String`; unparseable cells
are NaN-skipped at use sites. R/readr and F# providers parse typed on load.

### Exact changes

1. `stdlib/csv.valvet` (or new `stdlib/schema.valvet`; prefer new file,
   `module std.schema;`, to avoid touching the J2 kernels):
   - `pub fun parse_float_col(col: Vec<String>) -> Vec<Missing<Float64>>`
     (`Present`/`Missing` via the existing `String.parse_float` method used
     in `model.valvet`; same for `parse_int_col`, `parse_bool_col`).
   - `pub fun typed_frame(f: &DynamicFrame, schema: Vec<String>) ->
     Result<..., String>` mapping column names to `"int"|"float"|"bool"|"string"`.
   - Drive with the existing `infer_schema` output.
2. Fixtures vs R `readr::read_csv`/`col_types` oracle on a messy CSV
   (missing cells, ragged rows): compare per-column typed values.
3. No core changes. Optional C fast path later (rejected for v1: the
   NaN-skip policy already lives in stdlib and must stay identical).

Risks: `Missing<T>` printing/stats interop already exists (Missing-aware
stats); verify `frame_col` None handling mirrors `need_col` in model.valvet.

---

## R6/R7: deliberate non-goals (no work items)

- NSE/tidy eval, quosures, data masks: the `ColExpr` ADT + `|>` + stringly
  verbs are the first-order substitute; document the mapping
  (`subset(df, x > 1)` → `frame_filter_mask(f, ...)`), never implement NSE
  (Advanced R's own warning: NSE costs referential transparency).
- S3/S4/RC dispatch, promise/lazy args, environments-as-values, `...`,
  replacement functions, attributes: out of scope for a static no-GC
  language; UFCS plus named `fun`s cover the ergonomics.

---

## Gates (every batch)

fmt, clippy `-D warnings`, unit tests, fixtures debug+release, ASan/UBSan
(incl. generated-C sanitizers on new kernels), zero `<error>`, R-oracle
agreement logged in the fixture header comment. Commit per batch; push only
on your word.
