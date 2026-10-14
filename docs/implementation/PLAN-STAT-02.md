# PLAN-STAT-02: categorical contrasts and weighted least squares

Goal: make `y ~ group` actually work (today non-numeric columns are
silently dropped by na.omit) plus R-style `weights=`. All stdlib;
no core changes. R oracle (`model.matrix`, `lm(weights=)`, `contr.treatment`)
at 1e-12 relative.

Conventions: anchors are 0.3.0-era; grep quoted strings on drift.
Depends on PLAN-STAT-01 batch C only for the shared `ols_solve` helper
(if 01-C hasn't landed, inline the solve chain like `lm` does and refactor
when it lands — note the duplication in a comment either way).

Batch order: A (dummies + contrasts) -> B (factor auto-detection in
`model_frame`) -> C (WLS). Commit per batch; push only on your word.

---

## A. Dummy expansion + treatment contrasts (new `stdlib/factor_model.valvet`? No — extend `model.valvet`)

Gap: `model_frame` (`stdlib/model.valvet`) parses every term as float;
a `group` column of `"a"/"b"` drops every row. R treats strings as
factors with treatment contrasts.

### Exact changes (`stdlib/model.valvet`, append; no signature changes)

1. `pub fun factor_levels(col: Vec<String>) -> Vec<String>`: first-appearance
   order (matches the codebase's group-order precedent), skipping
   `is_na`-like cells — add `import std::csv::is_na;` (`model.valvet`
   currently imports frame/math/dist only).
2. `pub fun dummy_expand(col: Vec<String>, levels: Vec<String>) -> Vec<Vec<Float64>>`:
   one 0/1 column per level (reference level INCLUDED here; dropping
   happens at design assembly so the function stays total and testable).
   Unparseable-as-float is irrelevant — every cell maps by equality;
   NA-like cells produce all-zero rows (caller drops them via na.omit —
   see B).
3. `pub fun treatment_names(base: String, levels: Vec<String>) -> Vec<String>`:
   `base + level` for levels[1..] (reference = first-appearance level,
   documented; R defaults to alphabetical/first-factor — ours is
   first-appearance by the group-order law, note the difference in the
   fixture header).
4. Fixtures: levels order, all-zero NA rows, reference-drop naming —
   pure-Valvet, no oracle needed beyond eyeball values; R `model.matrix`
   agreement on a 3-level frame.
5. No core changes. No `Formula` change yet (terms stay strings; a
   `C(group)` wrapper syntax is explicitly deferred — plain names work).

Risks: level count explosion on high-cardinality strings (document: no
cap in v1; a `max_levels` guard is a one-line follow-up if wanted).

---

## B. Factor auto-detection in `model_frame` (behavior change, locked by fixtures)

Gap: the decision point. Today a string column in a term silently drops
rows; after this batch it expands to dummies.

### Exact changes (`stdlib/model.valvet`, `model_frame` only)

1. Per term column: scan non-NA cells; if every one parses as float
   (existing `parse_float` method), keep the numeric path byte-for-byte
   (no behavior change for numeric frames — existing `model_lm`/`model_glm`
   fixtures must pass untouched).
2. Else treat as factor: `levels = factor_levels(col)`; require
   `levels.len() >= 2`, else `Err("single-level factor ...")` (a constant
   column would singular-fail downstream — fail loudly here instead);
   expand via `dummy_expand`, DROP the reference level, append remaining
   columns to the row in level order; names gain `treatment_names`.
3. na.omit extends naturally: NA-like cells yield all-zero dummy rows —
   drop any row whose factor block is all zeros AND the original cell was
   NA (careful: a genuinely all-zero dummy row cannot occur for
   non-missing cells since exactly one level is 1 — so "all zeros" ==
   missing; no extra bookkeeping needed, comment this).
4. Interactions (`PLAN-GAP-R.md` R3) over factor terms: product of dummy
   columns — falls out of the existing product loop with zero new code;
   add one R-oracle fixture (`y ~ a*b` with `b` categorical).
5. Fixtures: `model_factor.valvet` (R `lm(y ~ group)` agreement incl.
   reference-level choice), unchanged numeric fixtures re-run green,
   single-level-factor `Err` fixture, all-missing-column still `Err` as
   before (existing semantics preserved).

Risks: THIS CHANGES `model_frame` behavior for string columns (rows kept
as dummies instead of dropped). That is the point, but any downstream
fixture relying on drop-strings breaks loudly at gate time — triage then
(it would be a genuine semantic decision, not an accident).

---

## C. Weighted least squares (new `wlm`, shared helper)

Gap: no `weights=` anywhere; heteroscedastic data has no honest path.

### Exact changes (`stdlib/model.valvet`, append)

1. Factor the solve (from PLAN-STAT-01 batch C if landed, else inline +
   TODO): `fun ols_solve(x: &Mat, y: &Vec<Float64>) -> Result<(Vec<Float64>,
   Mat), String>` returning `(beta, XtX_inv)` — check Valvet tuples first
   (grammar has `tupleLit`; verify value-tuples exist in this tree during
   implementation — if not, return a small `pub struct OlsSolve { beta,
   xtx_inv }` instead; do NOT add language features for this).
2. `pub fun wlm(mf: &ModelFrame, w: Vec<Float64>): Result<LmFit, String>`:
   - Length check `w.len() == mf.y.len()`, else `Err`; negative weights →
     `Err` (R silently... no, R errors downstream; we fail loudly, documented).
   - Scale rows by `sqrt(w)`: `mf.x.mat_scale_rows(...)` (exists, used in
     `glm_binomial`) and elementwise `y * sqrt(w)` (write a 10-line local
     helper — no new kernel; `sqrt` imported already).
   - Solve, then fill the SAME `LmFit` shape (coef/se/fitted/resid/r2/sigma
     /names) so `predict_lm`, `anova_lm`, `summary_lm`, Cook's, BP all work
     unchanged (state this in a comment + one integration fixture).
   - `sigma`/df use the same formulas as `lm` (document: weights-as-
     precision assumption, matching R's `lm(..., weights=)`).
3. Fixtures: R `lm(y ~ x, weights=w)` agreement at 1e-12; negative-weight
   and length-mismatch `Err` fixtures; integration fixture running the
   Batch-01 diagnostics on a `wlm` fit.
4. No core changes.

Risks: `mat_scale_rows` semantics (verify it scales rows in place/by value
from its `glm_binomial` use site before relying on it). Zero weights are
legal (row contributes nothing; singularity surfaces as the usual
`solve` Err — document, don't special-case).

---

## Gates (every batch)

fmt, clippy `-D warnings`, unit tests, fixtures debug+release, ASan/UBSan,
zero `<error>`, R-oracle agreement noted in fixture headers. Commit per
batch; push only on your word.
