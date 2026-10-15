# PLAN-STAT-01: ANOVA, model summary, regression diagnostics

Goal: make `lm`/`glm` credible end-to-end — an F distribution, ANOVA tables,
human-readable summaries, and the standard diagnostic battery. All stdlib
(pure Valvet over existing Mat/dist kernels); no core changes. R oracle
(`anova.lm`, `summary.lm`, `cooks.distance`, `vif`, `durbinWatsonTest`,
`ncvTest`) at 1e-12 relative (p-values at 1e-10 absolute near tails).

Conventions: line anchors are 0.3.0-era; grep quoted strings on drift.

Batch order: A (F CDF + one-way ANOVA) -> B (`anova_lm` + `summary`) ->
C (Cook/VIF/DW/BP). Commit per batch; push only on your word.

---

## A. F distribution + one-way ANOVA (`stdlib/infer.valvet`)

Gap: `infer.valvet` has `t_cdf` (`:158`, via `beta_reg`, `:142`), `chisq_cdf`,
`chisq_p`, but no F; the ANOVA paper port lives off-main for want of it.

### Exact changes (`stdlib/infer.valvet`, append)

1. `pub fun f_cdf(x: Float64, d1: Float64, d2: Float64): Float64`:
   - Guards first (mirror `t_cdf`, `infer.valvet:159-163`): `d1 <= 0`,
     `d2 <= 0`, or any NaN → `nan_i()` (the file’s NaN idiom, `infer.valvet:19`).
   - `x <= 0` → `0.0`; else `beta_reg(d1 / 2.0, d2 / 2.0, d1 * x / (d1 * x + d2))`.
   - `x <= 0` → `0.0`; else `beta_reg(d1 / 2.0, d2 / 2.0, d1 * x / (d1 * x + d2))`.
2. `pub fun f_p(x: Float64, d1: Float64, d2: Float64): Float64` = `1.0 - f_cdf(...)`
   (mirrors `chisq_p` exactly).
3. `pub struct AnovaOneWay { ss_between, ss_within, df1, df2, f, p: Float64 }`
   plus `pub fun anova_oneway(groups: Vec<Vec<Float64>>) -> Option<AnovaOneWay>`:
   - `None` when fewer than 2 non-empty groups or total n <= groups (mirror
     the file's `Option` style for degenerate input rather than trapping).
   - Grand mean, SSB/SSW in one pass each (follow `vec_mean`/`sum_sq` style
     from `model.valvet`); `f = (SSB/df1) / (SSW/df2)`; `p = f_p(...)`.
   - Zero within-group variance with nonzero between → F is +inf: return
     `f = 1.0/0.0`, `p = 0.0` explicitly (no silent NaN; document).
4. Fixtures: `anova_oneway.valvet` vs R `oneway.test(var.equal=TRUE)` /
   `anova(lm(...))` on a 3-group CSV at 1e-12; degenerate fixture
   (one group → `None`, assert via `if let`).
5. EBNF/README: none (stdlib only). README tour: one line under a new
   "Modeling" bullet if the tour has one — check during implementation,
   keep to ≤5 lines.

Risks: `beta_reg` edge behavior at x=0/1 — probe `f_cdf(0.0, ...)` and
`f_cdf(huge, ...)` against R before locking the fixture (same discipline
as the Laplace/Stirling verifications: no fitted constants, oracles decide).

---

## B. `anova_lm` + `summary` (`stdlib/model.valvet`)

Gap: `LmFit` (`model.valvet`, fields `coef/se/fitted/resid/r2/sigma/names`)
carries everything except a variance decomposition and a rendering.

### Exact changes (`stdlib/model.valvet`, append)

1. `pub struct AnovaLm { ss_reg, ss_res, df_reg, df_res, f, p, r2 }` plus
   `pub fun anova_lm(fit: &LmFit): AnovaLm`:
   - `resid`/`fitted` are on the fit: `ss_res = sum_sq(resid, 0.0)`,
     `tss = sum_sq(fitted + resid, mean)` reusing file-local `sum_sq` /
     `vec_mean` (`model.valvet` helpers); `ss_reg = tss - ss_res`;
     `df_reg = coef.len() - 1`, `df_res = n - coef.len()` with `n =
     resid.len()`; guard `df_res <= 0` or `ss_res == 0` → `f = NaN`
     (`0.0/0.0`, the file's idiom) and `p = NaN`, documented.
   - `p` via `std::infer::f_p` — needs `import std::infer::f_p;` at the
     top of `model.valvet` (check current imports during implementation;
     `normal_cdf` is already imported from `std::dist`, same pattern).
2. `pub fun summary_lm(fit: &LmFit): String`: fixed-width table
   (`to_string` per value; check float rendering width — `%.17g` may need
   trimming for alignment; document as-is, do not round values, only pad):
   header with `n`, `r2`, `sigma`; per-coefficient rows
   `name, coef, se, z, p` reusing `lm_p`-style z-approximation
   (`model.valvet: lm_p`); footer notes the z (not t) approximation,
   matching the file header's existing honesty note.
3. Fixtures: `model_anova.valvet` (R `anova.lm` agreement), `summary`
   golden `.stdout` (exact-text match — regenerate deliberately, never
   blindly), degenerate (p >= n → NaN fields, assert `!=` self).
4. No core changes.

Risks: `import` cycles (model → infer; infer must not import model —
verify; it currently doesn't). Golden stdout brittleness across float
formatting — acceptable, owned by us.

---

## C. Cook's D, VIF, Durbin-Watson, Breusch-Pagan (`stdlib/model.valvet`)

Gap: fit has no influence/collinearity/autocorrelation/heteroscedasticity
diagnostics. All four reduce to existing kernels.

### Exact changes (`stdlib/model.valvet`, append)

1. `pub fun cooks_distance(mf: &ModelFrame, fit: &LmFit): Result<Vec<Float64>, String>`:
   - Hat diagonal via existing kernels only: `xtx_inv` from
     `mf.x.transpose().matmul(mf.x).inverse()` (mirror `lm`'s own chain,
     `model.valvet:~195-230`, including its singularity `Err`s);
     `h_ii = row_i(X) dot (XtXinv * row_i)` using `mat_vec` per row
     (O(n·p²), documented; no new kernels).
   - `D_i = resid_i² / (p · sigma²) · h_ii / (1 - h_ii)²`, `sigma` from
     the fit; `h_ii == 1` → NaN entry (documented, not a trap).
   - Returns `Err` unless `mf` matches the fit (`mf.x.cols() ==
     fit.coef.len()`, same guard as `predict_lm`).
2. `pub fun vif(mf: &ModelFrame): Result<Vec<Float64>, String>`:
   - Correlation matrix over design columns via a small local `cor_plain`
     (`std::stats::cor` takes `Vec<Missing<Float64>>`; design columns are
     plain `Vec<Float64>`, so mirror its formula without the Missing
     plumbing), then `VIF_j = (R⁻¹)_jj` via existing `inverse()`; non-PD → `Err`.
   - Excludes the intercept column (constant → singular by construction;
     document; R drops it too).
3. `pub fun durbin_watson(fit: &LmFit): Float64`: Σ(e_i − e_{i−1})² /
   Σe_i² over `fit.resid`; empty/singleton → NaN.
4. `pub fun breusch_pagan(mf: &ModelFrame, fit: &LmFit): Result<BpTest, String>`
   with `pub struct BpTest { statistic, df, p }`: auxiliary regression of
   squared residuals on the design (reuse `lm` on a synthesized frame?
   cleaner: inline the normal equations exactly like `lm` does — do NOT
   call `lm` to avoid rebuilding ModelFrame plumbing; ~25 lines mirroring
   `lm`'s solve chain); statistic `n·R²`, `df = p − 1`, `p = chisq_p`
   (exists in infer.valvet — import it).
5. Fixtures, one per diagnostic, vs R (`cooks.distance`, `vif`, `durbinWatsonTest`,
   `ncvTest`) at 1e-12 relative (p at 1e-10 absolute).
6. No core changes.

Risks: O(n·p²) clarity over speed (document; thin-kernel fast paths are a
later optimization, never a semantics change). `lm`'s solve chain
duplicated in BP — comment the mirror explicitly so future `lm` changes
propagate (or factor a private `ols_solve(x: &Mat, y: &Vec<Float64>)`
helper used by both; preferred — small refactor inside the same file,
covered by existing `model_lm` fixtures).

---

## Gates (every batch)

fmt, clippy `-D warnings`, unit tests (codes-only where diagnostics),
fixtures debug+release, ASan/UBSan, zero `<error>`, R-oracle agreement
noted in fixture headers. Commit per batch; push only on your word.
