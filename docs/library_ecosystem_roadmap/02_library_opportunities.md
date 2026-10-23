# 02 Library opportunities

## 1. Opportunity map

The table classifies every repeated capability before proposing a library. The
goal is to avoid adding code that already exists and to avoid calling a
language gap a missing library.

| ID | Capability | Status | Evidence | Proposed home | Priority |
| --- | --- | --- | --- | --- | --- |
| O1 | Scalar math (`abs`, `min`, `max`, rounding, `eps`, hyperbolic) | Missing | XRD D4, G1; GAPS port 04 items 1-7; ports 05, 08, 09, 10 local shims | `std.math` | P0 |
| O2 | Correctly rounded `sqrt` | Partial existing | XRD D1, G2; `stdlib/math.valvet:56-131`; AUD 4 | `std.math` plus compiler builtin | P0 |
| O3 | Exact-order vector algebra | Partial existing | XRD D2, G3; `codegen.rs:1897-1908,2541-2568`; AUD 5 | `std.numeric` plus `Vec` methods | P0 |
| O4 | Collection capacity and literals | Missing | AUD 5; `codegen.rs:5662-5680` | Compiler `Vec` methods | P0 |
| O5 | Norms and scaled residuals | Missing | XRD D3, G4; GAPS ports 04-09 | `std.numeric` | P0 |
| O6 | Complex arithmetic | Existing but unused | XRD D5, G5; `stdlib/complex.valvet:12-36` | `std.complex` | P1 |
| O7 | Fixed arrays, tuples, partitions | Language/API | XRD D6, G7; PORT-01, PORT-02, PORT-06 | Compiler `[T; N]` and tuples | P1 |
| O8 | Matrix access and complex solve | Partial existing | XRD G6, G11; `codegen.rs:2928-3047`; PORT-03, PORT-09 | `Mat` get/set, narrow `std.linalg` | P1 |
| O9 | ODE counters, FSAL, convergence | Missing, domain | XRD D7, G8; GAPS ports 04-09 | `ode` package | P2 |
| O10 | Label printer and harness | Missing, workflow | XRD D8; AUD 3 | Warning-free output plus `harness` package | P0/P2 |
| O11 | Weak SDE noise helpers | Missing, one consumer | XRD G9; GAPS port 10 | `stochastic` package | P2 |
| O12 | Trixi log-mean and polynomial helpers | Missing, two consumers | XRD G10; PORT-01, PORT-02 | `std.numeric` later | P2 |
| O13 | Generated tableaus and rationals | Deliberate non-gap | XRD G12; GAPS ports 04-07 | Keep generators | No action |
| O14 | Private stdlib helpers | Private/unexported | XRD G13; `stdlib/model.valvet`, `stdlib/plot.valvet`, `stdlib/infer.valvet` | `std.numeric` and `std.math` | P1 |

The first eight rows are the core of the plan. Rows 9 to 12 are domain or
workflow work that should not delay the core. Row 13 is explicitly not a
library. Row 14 is immediate proof that the new core has users beyond the
ports.

## 2. Scalar math foundation

### 2.1 Evidence

Ports 04, 05, 08, 09, and 10 define local `abs` or `max_abs` helpers. Port 04
also needs `floor`, `ceil`, `isodd`, `iseven`, `eps`, `sinh`, `cosh`, and
`acosh`. Ports 01 and 02 need `sincos`, `ln_mean`, `inv_ln_mean`, and 24 uses
of `positive_part` and `negative_part`. The standard library has only `sqrt`,
`exp`, `log`, and `pow`; `sin`, `cos`, and `atan2` are compiler builtins.

There are at least 17 abs-like `if (x < 0.0)` patterns inside `stdlib/*.valvet`
itself. This is not a port-only problem.

### 2.2 Proposed API

```valvet
pub fun abs(x: Float64): Float64
pub fun min(a: Float64, b: Float64): Float64
pub fun max(a: Float64, b: Float64): Float64
pub fun floor(x: Float64): Float64
pub fun ceil(x: Float64): Float64
pub fun trunc(x: Float64): Float64
pub fun round(x: Float64): Float64
pub fun eps(x: Float64): Float64
pub fun sinh(x: Float64): Float64
pub fun cosh(x: Float64): Float64
pub fun tanh(x: Float64): Float64
pub fun acosh(x: Float64): Float64
pub fun asinh(x: Float64): Float64
pub fun atanh(x: Float64): Float64
pub fun clamp(x: Float64, lo: Float64, hi: Float64): Float64
pub fun pow_int(x: Float64, n: Int64): Float64
```

P1 additions:

```valvet
pub fun sincos(x: Float64): (Float64, Float64)
pub fun hypot(x: Float64, y: Float64): Float64
pub fun ldexp(x: Float64, n: Int64): Float64
pub fun evalpoly(x: Float64, coeffs: &Vec<Float64>): Float64
```

`sincos` returns a tuple, so its tuple form is available only after ARR-02.
Before then, use separate `sin` and `cos` calls or defer the helper.

`sinh`, `cosh`, and `acosh` can initially be built from `exp` and `sqrt`, as
the ports do. That preserves the current numerical behavior. A later libm
lowering is a separate performance decision.

### 2.3 Semantics that must be tested first

- `abs(-0.0)`, `abs(NaN)`, `abs(inf)`.
- `min(NaN, 1.0)`, `max(NaN, 1.0)`, signed zero for both argument orders.
- `floor`, `ceil`, `trunc`, and `round` at exact integers, exact halves,
  negative values, and large magnitudes.
- `eps(1.0)`, `eps(0.0)`, `eps(subnormal)`.
- `sinh`, `cosh`, `acosh` at the port 04 arguments.
- `pow_int` at `0`, positive, negative, and large exponents.

The current local shims do not necessarily match Julia `min` and `max` for
NaN. This must be fixed in the library before ports switch, not discovered
during migration.

### 2.4 Consumers

Ports 01, 02, 04, 05, 08, 09, and 10. Standard-library modules with
abs-like patterns can adopt `abs` after the port work. Port 06 uses `sqrt` and
`rms`, but does not use the scalar shims in its retained body.

## 3. Correctly rounded square root

### 3.1 Evidence

Six local `wv_sqrt_cr*` blocks exist in ports 04, 05, 06, 08, 09, and 10. The
bodies are the same algorithm after renaming. The standard library now uses
the same power-of-two seed, six Newton steps, and Dekker correction, but the
audit found 337 last-bit differences in 1,000,000 tiny normal inputs around
`2.3e-308` to `1.8e-305`.

Measured performance:

| Implementation | Per call |
| --- | ---: |
| Local correctly rounded helper | 31.56 ns |
| C `sqrt` | 2.71 ns |
| Ratio | 11.63x |

Replacing the helper with hardware `sqrt` was bit-exact for every BDF, FIRK,
and weak label. BDF warm numeric loops improved 2.25x and FIRK 1.26x.

### 3.2 Proposed API

```valvet
// Explicit library function, preserving edge semantics.
import std::math::sqrt;

// Compiler builtin, guarded for positive values outside the tiny-normal band.
val y = sqrt(x);
```

The compiler builtin should:

1. Check `x` is positive and outside the tiny-normal fallback band, or
   otherwise prove the hardware square root is already correct.
2. Emit C `sqrt` or `__builtin_sqrt`.
3. Fall back to the library wrapper for negative, zero, NaN, infinity, and tiny
   normals.

The library wrapper keeps:

- negative input gives NaN,
- `+0.0` gives `+0.0`,
- NaN propagates,
- `+inf` gives `+inf`.

The tiny-normal correction must be fixed before the builtin is enabled for
that range. Do not trade correctness for speed. Use `-fno-math-errno` only with
the builtin, and make `-ffp-contract=off` the default for exact-order code.

### 3.3 Consumers

All ten ports use `sqrt` directly or indirectly. The six local copies are the
first deletion target.

## 4. Exact-order vector algebra

### 4.1 Evidence

Ports 04 through 09 contain at least 18 vector helper implementations:
`wv_clone`, `wv_zeros`, `wv_fill`, `wv_scale`, `wv_axpy2` to `wv_axpy5`,
`wv_copyto`, `wv_div_scalar`, and `wv_scale_div`. Port 06 extracted a 246-line
helper block verbatim from port 05. Ports 07, 08, and 09 each carry another
pair of `zeros` and `clone`.

The existing `Vec` methods are useful but incomplete for exact-order kernels:

- `clone` exists and is efficient, but the ports bypass it.
- `add` and `mul` allocate a fresh vector, use push, and return `Result`.
- `dot` is available but returns `Result`.
- `add_scalar` allocates.
- There is no in-place `axpy`, `fill`, or `scale`.

### 4.2 Proposed API

The `std.numeric` P0 surface is in `01_ecosystem_architecture.md`. The key
design rules are:

- `axpy(a, x, b, y)` computes `a*x[i] + b*y[i]` with two multiplies and one
  add.
- `axpy3` and `axpy4` preserve the source grouping. Do not compose two-term
  calls, because that rounds differently.
- `scale` and `axpy` have allocating and `_into` forms. The `_into` forms
  require a destination of the result length and never resize or allocate.
- `clone` uses `Vec.clone`.
- `zeros` and `filled` allocate once.
- No `Result` return for same-length vector operations. A length mismatch is
  a caller error or a documented precondition. If a checked form is needed,
  name it `checked_axpy` or return `Result` explicitly.
- No hidden `fma`.

### 4.3 Why not general broadcasting

A general `map` or broadcast engine with named-function callbacks would be
harder to read and may be slower. The ports need a small number of fixed
operations with exact evaluation order. Named `axpy` family functions are the
right abstraction. A future broadcast syntax can be considered after the
language has closures and generics, not before.

### 4.4 Consumers

Ports 04, 05, 06, 07, 08, and 09. Port 10's scalar branch does not need vector
allocation, but its vector branch would benefit if extended.

## 5. Collection capacity and literals

### 5.1 Evidence

Port 7 makes 9,213 numeric `realloc` calls across 30 cases, about 61 per
amortized step. FIRK about 17 per step, BDF about 3. Most are 2-element
vectors. The `vec![...]` macro is built by repeated push, so even a fixed
literal starts with capacity zero.

The runtime already has an efficient clone that allocates once and copies. The
ports do not use it for their own clones.

### 5.2 Proposed API

```valvet
val empty = Vec<Float64>::with_capacity(n)
val z = vec![0.0; n]
val fixed = vec![1.0, 2.0, 3.0]   // one allocation, capacity 3
v.reserve(n)
v.resize(n, 0.0)
v.fill(0.0)
v.copy_from(&src)
```

Rules:

- `with_capacity(n)` allocates once, `len == 0`.
- `vec![value; n]` allocates once and fills `n` copies.
- `vec![a, b, c]` allocates exactly three slots, then fills.
- `resize` and `fill` do not reallocate if capacity is sufficient.
- `copy_from` requires a destination length at least as large as the source
  and does not resize the destination.

The first version of `vec![value; n]` may be restricted to copy types
(`Float64`, `Int32`, `Int64`, `Bool`). Owned types need a clone policy.

### 5.3 Consumers

All ports, especially 04 to 09. BDF and FIRK can eliminate most 2-element
allocations. Port 7 still needs coefficient vectors, but can allocate once.

## 6. Norms and scaled residuals

### 6.1 Evidence

Five ports implement the same SciML contract:

```text
tmp[i] = x[i] / (alpha + max(abs(u0[i]), abs(u1[i])) * rho)
rms = sqrt(sum(tmp[i]^2) / n)
```

Port 06 has a two-part `ArrayPartition` sum order:
`sqrt((s1 + s2) / (n1 + n2))`. Ports 08 and 09 differ only in whether `atol`
and `rtol` are constants or parameters.

### 6.2 Proposed API

```valvet
pub fun rms(x: &Vec<Float64>): Float64
pub fun rms_parts(a: &Vec<Float64>, b: &Vec<Float64>): Float64
pub fun scaled_residual(x: &Vec<Float64>, u0: &Vec<Float64>, u1: &Vec<Float64>,
                        alpha: Float64, rho: Float64): Vec<Float64>
pub fun scaled_residual2(x: &Vec<Float64>, u0a: &Vec<Float64>, u1a: &Vec<Float64>,
                         u0b: &Vec<Float64>, u1b: &Vec<Float64>,
                         alpha: Float64, rho: Float64): Vec<Float64>
```

Exactness:

- Sum in increasing index order.
- Divide by `n` before `sqrt`.
- `rms_parts` sums part 1 fully, then part 2, then combines.
- Keep the exact parentheses in the residual denominator.
- Use `std.math.sqrt` or the builtin only after the tiny-normal fix.

### 6.3 Consumers

Ports 04, 05, 06, 08, and 09. Ports 07 and 10 can use `rms` where the adaptive
path is not exercised.

## 7. Complex arithmetic

### 7.1 Evidence and decision

Port 09 defines `C09` and four functions that are formula-identical to
`std.complex`:

```valvet
pub struct Complex { re: Float64, im: Float64 }
pub fun cadd(a: &Complex, b: &Complex): Complex
pub fun csub(a: &Complex, b: &Complex): Complex
pub fun cmul(a: &Complex, b: &Complex): Complex
pub fun cdiv(a: &Complex, b: &Complex): Complex
```

This is an existing-unused capability, not a missing library. The port should
import `std::complex` or use small value-taking wrappers if borrow syntax is
too noisy at the 23 broadcast sites.

Do not change `cdiv` to a scaled or robust algorithm. The Radau3 oracle uses
the simple denominator `b.re*b.re + b.im*b.im`. Add `cdiv_robust` if a future
consumer needs overflow safety.

### 7.2 Additions

- `std.math.pow_int(x: Float64, n: Int64)` is the scalar recursion source; it
  lands with FND-01, not with the complex migration.
- Value-taking wrappers only if needed.
- A complex 2x2 solve in `std.linalg` for the current port.
- No general complex matrix until an implicit solver exists.

### 7.3 Consumers

Port 09 now. Future FIRK or implicit extrapolation ports later.

## 8. Fixed arrays, tuples, and partitions

### 8.1 Evidence

- Ports 01 and 02 use `V2`, `V3`, `V4`, and `V9` structs to replace
  `SVector`; `Pair` and `V4Pair` replace tuple returns.
- Port 06 uses paired `Vec<Float64>` fields for `ArrayPartition((du, u))` and
  rejects nested vector literals.
- Port 03 flattens `Matrix{T}` into a row-major `Vec<Float64>` because `Mat`
  has no general mutable element index.

These are real representation workarounds, not simple missing functions. They
are the largest effort item and must not block P0.

### 8.2 Proposed API

```valvet
val p: [Float64; 2] = [x, y]
val q: (Float64, Float64) = (x, y)
```

Required semantics:

- Inline storage, no heap allocation.
- Constant-index access without a bounds check when the index is proven in
  range.
- Element assignment.
- Copy behavior for copyable element types.
- Destructuring for tuples.
- Struct fields and return values without hidden allocation.
- A documented ABI so generated C does not introduce surprising copies.

### 8.3 Interim option

If compiler arrays are delayed, `std.fixed` may provide `Vec2`, `Vec3`,
`Vec4`, and `Vec9` as concrete structs. This is a workaround, not the target
architecture. It must not become a generic small-vector framework.

### 8.4 Consumers

Ports 01, 02, 03, and 06. Ports 07, 08, and 09 also use 2-element vectors and
would benefit from fixed storage, but their current ports already pass their
oracles (port 07 has 18 one-ulp labels) and should not be rewritten first.

## 9. Matrix access and linear algebra

### 9.1 What exists

`Mat` supports real matrix construction, transpose, outer product, Kronecker
product, matmul, LU solve, inverse, Cholesky, row scaling, diagonal, column
extraction, eigen, and SVD. This is substantial and should not be duplicated in
`std.linalg`.

### 9.2 What is missing

- General mutable element get/set.
- A complex matrix type.
- A complex LU solve.
- A forward-difference Jacobian abstraction. This needs function pointers or
  closures, which Valvet does not have. Keep the Jacobian in the port for now.

### 9.3 Proposed API

```valvet
m.get(i: Int64, j: Int64): Option<Float64>
m.set(i: Int64, j: Int64, value: Float64): Result<Unit, ShapeError>
```

`get` and `set` are methods on `Mat`. `ShapeError` carries the offending index
and the matrix dimensions.

`std.linalg` v1:

```valvet
pub fun solve2x2_complex(a: &CMat2, b: &CVec2): CVec2
```

`CMat2` and `CVec2` are narrow structs for this wrapper, not general matrix
types. If they add copies, use a flat six-argument function with the same
evaluation order.

The wrapper must use the same real-arithmetic expansion as the Radau3 oracle.
Do not call LAPACK. Do not add a second complex type.

### 9.4 Consumers

Port 03 for `Mat` access; port 09 for the 2x2 complex solve; future implicit
ports for broader linear algebra.

## 10. ODE protocol and harness

### 10.1 ODE protocol

Ports 04 to 10 share counters, FSAL rotation, convergence ratios, and residual
adapters. They do not share stage kernels, caches, tableaus, or branches. A
generic solver would be a union of every family and should not be built.

The `ode` package should export only:

- `OdeStatus` with `nf`, `nsolve`, `nnonliniter`.
- `increment_nf`.
- `rotate_fsal`.
- `convergence_ratio`.
- Re-exports of `std.numeric.rms` and residual adapters.

The ConstantCache `nf` asymmetry stays explicit. Do not count the final
`fsallast` automatically.

### 10.2 Harness

The label protocol is `print(label); print("="); println(value)`. The
deprecated `print` alias and `println` both append a newline, so this emits
three lines per label: label, `=`, and value. It triggers W1002 twice per run.
It also allocates a string for `"="` on every label.

The warning-free migration is:

```valvet
println(label)
println("=")
println(value)
```

This preserves the exact three-line output. A no-newline `write` primitive is
also useful, but substituting it here would produce one line and break the
oracle format. Do not use `write` for this protocol. The per-label `"="`
allocation is a separate literal-output issue for `PERF-06`; migrating `print`
to `println` does not remove it.

Once the migration exists, an optional `harness` package can provide
`emit_label(label, value)` around the three line calls. Do not hide W1002
behind a library.

### 10.3 Consumers

Ports 04 to 10 for the harness. Ports 04 to 09 for ODE counters. Port 07 has
the strongest cost evidence.

## 11. Weak stochastic helpers

Port 10 implements `calc_threepoint_random` as `wv_three10`, with the exact
nested predicate and negative quantile. It is the only current consumer. A
narrow `stochastic` v1 is justified for `Wiener` and `three_point_increment`;
wider helpers wait for a second weak-SDE port.

The v1 surface:

- Keep scalar and vector branches separate.
- Preserve the nested predicate:
  `abs(dW_scaled) > -quantile`, then `dW_scaled < quantile`.
- Keep Wiener increments as types, not loose fields. A future `Ihat2` should
  also be a type, but it is not part of v1.
- Do not replace deterministic noise with an RNG in oracle tests.
- Do not add `two_point_increment` or `is_diagonal_noise` without a second
  concrete consumer.

## 12. Domain helpers

### 12.1 Trixi log-mean

`ln_mean` and `inv_log_mean` appear 20 and 8 times across ports 01 and 02.
They are a real two-port reuse case. The exact polynomial branch matters near
`f2 < 1e-4`. Add them to `std.numeric` only after `evalpoly` exists, and only
when ports 01 and 02 are restored or another Trixi port is planned.

`positive_part` and `negative_part` do not need public names. They are
`max(x, 0.0)` and `min(x, 0.0)` after the scalar foundation.

### 12.2 Gridap quadrature

Port 03 uses `Matrix`, `axes`, `size`, `iszero`, and tuple branches. The
practical fix is `Mat` get/set, not a quadrature library. Do not build a
quadrature container for one port.

### 12.3 Tableaus and rationals

Ports 04 to 07 use generated tableaus and rational-derived coefficients. Valvet
has no rationals, bignums, symbols, parametric tableau types, or tuples. The
correct answer is to keep generators that run the unmodified upstream
constructors and emit `%.17g` literals. Do not add `std.rational` or
`std.tableau` without a concrete second use.

## 13. Existing capabilities to adopt before replacing

| Capability | Existing location | Action |
| --- | --- | --- |
| Complex arithmetic | `stdlib/complex.valvet:12-36` | Port 09 imports it; do not add a second type |
| `Vec.clone` | `crates/valvet_core/src/codegen.rs:3081` | Ports use `.clone()` instead of push loops |
| `Vec.add/mul/dot/add_scalar` | `crates/valvet_core/src/codegen.rs:1897-1908,2541-2568` | Keep method behavior; `std.numeric` supplies exact-order free functions and `_into` forms |
| Real `Mat` LU and matvec | `crates/valvet_core/src/codegen.rs:2980-3047` | Use where semantics fit; add get/set |
| Real `Mat` Cholesky, eigen, SVD | `crates/valvet_core/src/codegen.rs:3019-3047` | Keep; do not duplicate in `std.linalg` |
| `std.dist` RNG and normal sampling | `stdlib/dist.valvet:140-236,504-561` | Use for stochastic tests; add Wiener type only when needed |
| `std.stats` missing-value statistics | `stdlib/stats.valvet:36-290` | Keep separate from raw vector numerics |

## 14. Standard-library consolidation

The private helpers in `std.model`, `std.plot`, and `std.infer` are the first
non-port consumers of `std.numeric`:

- `stdlib/model.valvet:27-35`: `vec_add` becomes `add`.
- `stdlib/model.valvet:526-559`: `vec_sub`, `vec_mean`, and `sum_sq` become
  `sub`, `mean`, and `sum_sq`.
- `stdlib/plot.valvet:342-409`: `fmin` and `fmax` become `min_vec` and
  `max_vec`; `fmean` and `fsd` become `mean` and `sd`. `fmin2` becomes
  `std.math.min` only if the NaN and signed-zero fixture proves identical
  behavior; otherwise it stays local.
- `stdlib/infer.valvet:192`: `mean_i` becomes `mean`.
- At least 17 abs-like patterns move to `std.math.abs`, not `std.numeric`.

Move only generic operations. Keep missing-value statistics in `std.stats`,
formula-specific functions in `std.model`, and plotting-specific functions in
`std.plot`. The migration must not change public behavior or output.

The first public vector helpers are deliberately narrow. `add`, `sub`, `sum`,
`mean`, `min_vec`, `max_vec`, `sum_sq`, and `sd` each have an existing stdlib
consumer. All current `model` call sites for `vec_add`/`vec_sub` pass equal
lengths, matching the new precondition. `min_vec` and `max_vec` require
non-empty input. `sd` is the sample standard deviation with denominator `n - 1`
and returns `0.0` for `n < 2`. Empty `mean` returns NaN. These contracts are
pinned before publication.

## 15. Deliberate non-opportunities

- **Euler and MHD flux functions.** Same names, different physics. Do not
  merge.
- **Runge-Kutta stage kernels.** Coefficients, stage counts, estimators, and
  branches differ. A shared `rk_step` would become a union of families.
- **Cache structs.** Fields and defaults are algorithm-specific.
- **Generated tableaus.** Data, not duplicated logic. Keep generators.
- **Oracle code.** The Julia oracle must stay independent of the Valvet port.
- **`@muladd`/FMA.** The ports intentionally avoid fusion. Do not add it.
- **Threading.** Out of scope for the current ports.
- **A generic matrix type.** `Mat` is sufficient until fixed arrays and
  generics exist.
- **A public registry.** Path and git dependencies are enough for now.

## 16. Recommended first slice

The first implementation slice is:

1. `std.math` scalar foundation.
2. `sqrt` tiny-normal fix and guarded builtin.
3. `Vec.with_capacity`, exact `vec![...]`, `vec![value; n]`.
4. `std.numeric` P0 vector primitives.
5. Warning-free output and literal print fast path.
6. `std.numeric` norms and residuals.
7. Migrate port 08 and port 10, then port 09 and port 07.

Do not start with complex, fixed arrays, or ODE packages. They are real, but
they are not the critical path.
