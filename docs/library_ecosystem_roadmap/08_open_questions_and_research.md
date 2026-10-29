# 08 Open questions and research

## 1. Purpose

This document records the decisions that are not settled by the existing
reports. It separates:

- Questions that block a P0 task.
- Questions that can wait until a later phase.
- External references that are useful but not authoritative over repository
  evidence.
- Assumptions that must be checked before a claim becomes a commitment.

A question is not a reason to stop all work. It is a reason not to pretend the
answer is known.

## 2. Blocking questions

### Q1. What is the exact tiny-normal `sqrt` failure and fix?

**Why it matters:** FND-02 and FND-03 depend on it. If the library cannot be
made bit-exact for all inputs, the guarded builtin must not handle the tiny
range.

**What is known:** `std.math.sqrt` has 337 last-bit mismatches in 1,000,000
tiny normal inputs around `2.3e-308` to `1.8e-305`. Hardware `sqrt` was
bit-exact for all BDF, FIRK, and weak oracles. The local software helper is
11.63x slower per call.

**What is not known:** whether the defect is in the binade seed, the two-product
correction, the tie-break, or the final rounding path.

**Investigation:** build a bit-level sweep, compare against hardware `sqrt`,
and isolate the first failing exponent and mantissa. Test the corrected
algorithm on all normals, subnormals, zero, infinity, and NaN.

**Decision needed by:** Phase 0, before FND-03.

### Q2. What is the `std.math.min` and `max` contract for NaN and signed zero?

**Why it matters:** ports use local `max_abs` and `abs` shims. If the library
uses a naive comparison, it can disagree with Julia or with the local shim.

**What is known:** a naive `if (a < b) { a } else { b }` does not define NaN
or signed-zero behavior consistently with C `fmin`/`fmax` or Julia.

**Investigation:** write a truth table for `min` and `max` over normal values,
`+0.0`, `-0.0`, NaN, and infinity in both argument orders. Compare Julia,
C `fmin`/`fmax`, and the port shims. Choose one contract and test it before
publishing.

**Decision needed by:** Phase 0, FND-01.

### Q3. Does `vec![value; n]` support owned element types in v1?

**Why it matters:** COL-01 needs a parser and codegen rule. Cloning an owned
element `n` times requires a clone function or a `Copy`-like distinction that
Valvet may not have.

**Options:**

1. Restrict v1 to copy types (`Float64`, `Int32`, `Int64`, `Bool`).
2. Require an explicit clone function argument.
3. Reject the form for owned types with a diagnostic.
4. Introduce a general `Clone` mechanism as separate language work.

**Recommendation:** option 1 or 3 for P0. Do not turn a small allocation fix
into a trait system.

**Decision needed by:** Phase 0, COL-01.

### Q4. Is `print` deprecated, aliased, or retained?

**Why it matters:** `print` is a deprecated alias of `println`; both append a
newline. The oracle harness emits three lines per label and triggers W1002
twice. IO-01 needs a warning-free migration and a separate no-newline
primitive.

**Options:**

1. Keep `print` as a deprecated alias of `println`, add `write` for no-newline
   output, and migrate ports to `println`.
2. Remove `print` entirely. This breaks compatibility for little gain.
3. Redefine `print` as no-newline and add `println` only. This changes existing
   output and can break the three-line oracle format. Reject.

**Recommendation:** option 1. The three-line protocol stays on `println`;
`write` is for deliberate no-newline output. Do not substitute `write` in the
label protocol. Output must remain byte-identical.

**Decision needed by:** Phase 0, IO-01.

### Q5. Does `std.numeric` target only `Vec<Float64>` in v1?

**Why it matters:** user-defined generics do not exist. A generic-looking API
would either be a compiler builtin or a set of concrete overloads.

**Known evidence:** ports 04 to 09 use `Vec<Float64>` and scalar `Float64`.
Integer vector algebra is not a demonstrated need.

**Recommendation:** yes, `Vec<Float64>` only in v1. Add `Int64` variants only
for a real consumer.

**Decision needed by:** Phase 0, NUM-01.

### Q6. Does `std.numeric` return `Result` for length mismatch?

**Why it matters:** the existing `Vec.add` and `Vec.mul` return `Result`, and
ports avoid them because the type is inconvenient in exact-order kernels. The
new API must choose between safety and hot-loop ergonomics.

**Options:**

1. Check lengths once and abort on mismatch. Simple, but a library should not
   abort silently.
2. Return `Result` from every call. Safe, but noisy in expressions.
3. Document equal-length preconditions and provide a separate `checked_*`
   family.
4. Return a wrong-length result with a documented rule. Reject.

**Recommendation:** option 3 for same-length primitives, with a checked
variant for callers that need it. A length check may remain at the top of the
function, outside the element loop.

**Decision needed by:** Phase 0, NUM-01.

### Q7. What is the initial `std.numeric` exactness test set?

**Why it matters:** every migration depends on a shared bit-exact test suite.
Without it, a helper can pass one port and fail another.

**Investigation:** create cases for lengths 0, 1, 2, and a larger vector; mixed
signs; subnormals; infinities; NaNs; and the exact port formulas. Include
`axpy3` and `axpy4` grouping, `rms` sum order, `rms_parts` part order, and
residual parentheses.

**Decision needed by:** Phase 0, before NUM-02.

## 3. Architecture questions

### Q8. Should fixed arrays be compiler types or an interim `std.fixed`?

**Why it matters:** ARR-01 and ARR-02 are large. A library of `Vec2`, `Vec3`,
`Vec4`, and `Vec9` is faster to ship but recreates manual representation
workarounds.

**Recommendation:** compiler fixed arrays are the target. An interim
`std.fixed` is acceptable only if a port is blocked and the library is marked
as temporary. Do not build both.

**Decision needed by:** Phase 1 design review.

### Q9. What are the copy, move, and ABI rules for `[T; N]`?

**Why it matters:** a fixed array that is copied by value can be slower than a
`Vec` reference for large `N`. A fixed array that is always passed by pointer
needs lifetime and aliasing rules.

**Investigation:** decide a size threshold and test generated C for `N = 2`,
`N = 9`, and a larger array. Measure stack frames and copies. Do not rely on
intuition.

**Decision needed by:** Phase 1, ARR-01.

### Q10. Are tuples positional only, or can fields be named?

**Why it matters:** `sincos` returns two floats; `ArrayPartition` returns two
vectors. Positional tuples are enough for the evidence. Named fields would
require a different type design.

**Recommendation:** positional tuples in v1. Named structs remain the answer
for readable multi-field values.

**Decision needed by:** Phase 1, ARR-02.

### Q11. Should `Complex` become a compiler scalar?

**Why it matters:** C99 `_Complex` could reduce copies and map to native
complex arithmetic. It would also add a new ABI and edge-case contract.

**Known evidence:** one port uses complex arithmetic, and `std.complex` is
sufficient. No current consumer needs C99 `_Complex` or complex SIMD.

**Recommendation:** keep the struct. Revisit only when an implicit solver or
spectral kernel needs it.

**Decision needed by:** Phase 2 or later.

### Q12. Should `Mat` and `ArrayD` indexing return `Option`/`Result` or trap?

**Why it matters:** a general mutable index is the missing piece for port 03.
The error type affects ergonomics and codegen.

**Options:** `Option<Float64>`, `Result<Float64, ShapeError>`, a direct index
with a documented abort, or a checked/unchecked pair.

**Decision:** MAT-01 and ARR-03 use direct indexing with runtime bounds checks
and abort through the shared `index out of bounds` trap. `Vec`, `Mat`, and
`ArrayD` therefore share one indexing convention. A recoverable `Option` or
`Result` API is deferred until a consumer needs it. Do not add a second matrix
type to avoid the decision.

**Decision needed by:** Phase 1, MAT-01 and ARR-03.

### Q12a. What are the rank, layout, and view rules for N-dimensional arrays?

**Why it matters:** ports carry nested `Vec<Vec<...>>` grids, but nested vectors
are jagged and do not provide a row-major element buffer or a runtime rank.
The layout decision affects every later array task.

**Decision:** `ArrayD<T>` has runtime rank 1 to 32, owns one row-major buffer,
stores shape and strides in elements, and is move-only. `Array<T, N>` is the
compile-time-rank form for 1 to 8. It reuses the `ArrayD<T>` layout and kernels
through a rank-specific typedef and wrappers; index arity is checked in sema, so
indexing has no runtime rank check. Shape dimensions remain runtime values, and
constructors validate that the shape vector length matches `N`. `Vec<T>` stays
1D and `Mat` stays 2D. Views, `slice`, `select`, stride-0 broadcasting, and
borrowed array views are deferred. Do not treat `Vec<Vec<T>>` as an
N-dimensional array.

**Decision needed by:** Phase 1, ARR-03 and ARR-04.

### Q13. What is the package namespace and module naming rule?

**Why it matters:** `ode`, `stochastic`, and `harness` need names that work
with the manifest and resolver. The repository has package examples with
simple module names, but no documented ecosystem convention.

**Investigation:** read the resolver rules for package name versus module name,
test a path dependency with a hyphenated manifest name, and document the
result. Reserve `std.*` for core modules.

**Decision needed by:** Phase 2 before the first package.

### Q14. Is a public registry needed at all in this roadmap?

**Current evidence:** the manifest supports path and git dependencies,
version bounds, a lockfile, and vendoring. There is no central registry.

**Recommendation:** no registry in Phases 0 to 4. Solve distribution only when
path and git dependencies are a demonstrated blocker. Do not confuse a missing
registry with a missing standard library.

**Decision needed by:** Phase 4, PKG-01.

### Q15. Which ODE primitives are safe to share?

**Why it matters:** counters, FSAL rotation, and convergence ratios are common,
but ConstantCache versus mutable-cache counter asymmetry is subtle.

**Known evidence:** ports 07, 08, and 09 reproduce an upstream behavior where
the final `fsallast` evaluation is not counted for ConstantCache. A helper that
increments automatically would hide this.

**Recommendation:** share only explicit primitives. Keep every counter mutation
visible at the call site. Test both cache modes.

**Decision needed by:** Phase 2, ODE-01.

### Q16. Does the `stochastic` package have a second consumer?

**Current evidence:** port 10 is the only weak-SDE port. The three-point random
helper is about 20 lines. A wider package would move complexity more than it
removes it.

**Recommendation:** build only the narrow v1 that port 10 proves: `Wiener` and
`three_point_increment`. Defer wider helpers until a second consumer exists.
Preserve the nested predicate and negative quantile exactly.

**Decision needed by:** Phase 2, STOCH-01.

### Q17. Should `std.numeric` include Trixi `log_mean`?

**Why it matters:** it is used 20 times in port 01 and 8 times in port 02, but
those sources are report-only and not retained. The exact polynomial branch is
numerically important.

**Recommendation:** yes, in P2, after `evalpoly` exists and a reconstructed
fixture can validate the branch. Do not put it in the P0 core.

**Decision needed by:** Phase 2, NUM-04.

### Q18. What is the safety model for unchecked indexing?

**Why it matters:** PERF-03 can remove bounds checks, but a general `@inbounds`
block can turn a bug into memory corruption. Valvet currently favors safety.

**Options:**

1. Only elide checks when the compiler proves the index in range.
2. Add an explicit unchecked block with a documented contract.
3. Add both, with proof first.

**Recommendation:** proof first. Add an explicit unchecked mechanism only if a
hot loop cannot be proven and the benchmark shows a real gain. Keep ASan
coverage for tests.

**Decision needed by:** Phase 1, PERF-03.

### Q19. What is the temporary lifetime rule for borrowed arguments?

**Why it matters:** `&wv_add9v(...)` is rejected today, forcing a local binding
at many stage-construction sites. Changing the rule affects drop order and
aliasing.

**Investigation:** write the lifetime and drop rules for temporaries in call
arguments, including nested calls and `&mut` receivers. Test moves, early
returns, and loops. Do not implement a rule that only handles the port pattern
and breaks elsewhere.

**Decision needed by:** Phase 1, DX-03.

### Q20. Should the compiler infer integer literal width everywhere?

**Why it matters:** DX-01 removes 589 casts in retained sources, but literal
inference can change overload resolution and error messages.

**Recommendation:** infer only where an expected type exists (comparison,
assignment, argument, loop bound). Keep a default for unconstrained literals.
Do not add implicit narrowing. Add a design note before implementation.

**Decision needed by:** Phase 0, DX-01.

## 4. Performance questions

### Q21. Is `-O3` ever a default?

**Current evidence:** numeric loops improve 2.6 to 25 percent, binaries grow 11
to 40 percent, and end-to-end gain is up to about 8 percent because output and
startup dominate.

**Recommendation:** no. Keep `-O2` default. Offer an opt-in profile after
TEST-01 and PERF-05.

**Decision needed by:** Phase 3, PERF-05.

### Q22. Which exact FP flags define the bit-exact contract?

**Current baseline:** `-std=c11 -O2 -fwrapv -pthread -lm`. `-ffp-contract=off`
is not a default. FMA contraction can change results on targets that fuse.

**Investigation:** verify that `-ffp-contract=off` is accepted by every
supported compiler wrapper and that no target silently contracts. Record the
rule in the numeric contract.

**Recommendation:** off by default for exact-order code. Do not rely on FMA in
`std.numeric`. A future fast profile may opt in only with explicit semantics and
its own oracle.

**Decision needed by:** Phase 0, FND-03.

### Q23. Is `restrict` safe for any library signature?

**Current evidence:** no measurement. Most hot vectors are read-only inputs and
distinct outputs, but the language does not prove no-alias.

**Recommendation:** do not add `restrict` by default. Add it only after an
aliasing design and a measured benefit.

**Decision needed by:** Phase 3 or later.

### Q24. How should benchmark thresholds be normalized?

**Why it matters:** timing varies by host, GCC version, CPU frequency, and
background load. A naive CI threshold will be flaky.

**Recommendation:** make deterministic checks blocking and timing checks
advisory unless the runner is dedicated. Block on oracle equality, allocation
counts, generated-C diffs, and binary size. Report timing with a moving
baseline.

**Decision needed by:** Phase 1, TEST-01.

### Q25. Does large-struct ABI work belong in the compiler or the port?

**Current evidence:** Port 07 copies a 536-byte state per step, but the
batteries are output-bound. BDF and FIRK inline their step functions.

**Options:** compiler move elision, a `&mut State` port rewrite, or no change.

**Recommendation:** measure a long run first. If the copy is not material, do
not change the ABI. If it is, prefer a port-level reference pattern before a
global compiler ABI change.

**Decision needed by:** Phase 3, PERF-04.

## 5. Evidence and scope questions

### Q26. How much can be claimed for ports 01 to 04?

**Known:** their reports and line counts are available. Their sources are not
retained. Ports 05 and 06 have partial sources. Ports 07 to 10 are complete.

**Rule:** claims about exact helper bodies in ports 01 to 04 are unverified.
Their reported occurrence counts are usable only as report evidence. Do not
report a precise removable-line number for them without reconstructing the
source.

**Decision needed by:** before updating impact estimates after a migration.

### Q27. Should the working evidence reports be copied into the repository?

**Why it matters:** the roadmap cites `/tmp/opencode/ports/...`, which may not
exist for a future maintainer. The repository has durable source paths and
reports, but the cross-port and audit reports are working artifacts.

**Options:**

1. Copy the three reports into `docs/implementation/` or a `docs/evidence/`
   directory.
2. Keep the roadmap self-contained with tables and repository anchors.
3. Add a short evidence appendix only.

**Recommendation:** copy the reports only if the user wants durable evidence
in the repository. The roadmap already includes the key measured tables and
source anchors. Do not silently rewrite the original reports.

**Decision needed by:** before the first implementation batch.

### Q28. Is exact order required for every new numeric API?

**Current evidence:** all bit-exact ports depend on it. A future native
application might prefer faster reassociation.

**Recommendation:** exact order is the default contract for `std.numeric`.
A separate `fast` or `approx` API can be added later with explicit semantics.
Do not make exactness optional through a global flag.

**Decision needed by:** Phase 0, NUM-01.

### Q29. What is the first public `std.numeric` versioning promise?

**Why it matters:** once ports and standard-library modules import it, changing
semantics is expensive.

**Recommendation:** mark it experimental through Phase 1, then freeze the P0
surface after two port migrations. Additions are allowed; exact-order or
allocation changes require a migration note.

**Decision needed by:** Phase 1 exit.

### Q30. Does any of this require a language generics feature?

**Current evidence:** no. Concrete `Vec<Float64>` APIs cover the ports. Fixed
arrays and tuples are representation features, not generics. A generic
`std.numeric` would be nicer but is not required for the first payoff.

**Recommendation:** do not block P0/P1 on generics. Research them in LANG-01
for future native libraries.

**Decision needed by:** Phase 4 research only.

## 6. Assumptions to validate

| Assumption | Why it matters | Validation |
| --- | --- | --- |
| `Float64` is IEEE 754 binary64 on all supported targets | Exactness contract | Document target ABI and test |
| GCC C11 remains the default backend | Performance and codegen plan | Confirm no alternate backend target |
| `Vec<T>` remains compiler-instantiated | Where generic ops live | Compiler architecture review |
| No user generics in P0/P1 | Concrete API choice | Language roadmap review |
| Ports 01-04 line counts are accurate to the source used | Impact estimates | Restore or re-fetch sources if needed |
| The audit host is representative | Performance direction | Re-run on at least one other host |
| The oracle printer is not production cost | Performance priorities | Separate numeric from output in every benchmark |
| Path/git dependencies are sufficient | Package plan | Build a real two-package example |
| Existing stdlib modules can adopt the new core without API break | Consolidation | Run all stdlib fixtures |
| `std.complex` formulas match port 09 | Complex migration | 332/332 oracle |
| Fixed arrays can be inline without an ABI disaster | Representation plan | Prototype and measure |
| `std.numeric` can meet the hand-written loop performance | Library viability | Kernel benchmarks after implementation |

## 7. External references

These references are used to resolve specific design questions. Repository
evidence takes precedence when they conflict.

### Julia semantics

- Julia Mathematics documentation: https://docs.julialang.org/en/v1/base/math/
  Used for `abs`, `min`, `max`, rounding, `sincos`, `evalpoly`, and special
  function semantics. Do not copy Julia names that do not fit Valvet.
- Julia Performance Tips: https://docs.julialang.org/en/v1/manual/performance-tips/
  Used for the general lesson that type stability and allocation avoidance
  matter more than surface syntax. Valvet's static types already remove some of
  Julia's dynamic dispatch problems.
- Julia Style Guide: https://docs.julialang.org/en/v1/manual/style-guide/
  Used for naming and API consistency questions, not as a mandate.

### Collections and API design

- Rust `Vec`: https://doc.rust-lang.org/std/vec/struct.Vec.html
  Used for `with_capacity`, `reserve`, `resize`, `fill`, and exact allocation
  semantics. Valvet does not need Rust's full API.
- Rust `Vec::with_capacity`: https://doc.rust-lang.org/std/vec/struct.Vec.html#method.with_capacity
  Used for the allocation contract.
- Rust API Guidelines, naming: https://rust-lang.github.io/api-guidelines/naming.html
  Used for consistent naming and `_into`-style mutation conventions.
- C++ `std::span`: https://en.cppreference.com/w/cpp/container/span
  Used as a reference for borrowed contiguous views. Not adopted in this plan.
- C++ `std::mdspan`: https://en.cppreference.com/w/cpp/container/mdspan
  Used as a reference for future matrix views. Not adopted before `Mat` get/set
  and a concrete consumer.

### Floating point and code generation

- LLVM Language Reference, fast-math flags: https://llvm.org/docs/LangRef.html#fast-math-flags
  Used to justify why `-ffast-math` and reassociation are excluded from the
  bit-exact contract.
- LLVM Auto-Vectorization: https://llvm.org/docs/Vectorizers.html
  Used to explain why 2-element ODE kernels should not be judged by packed-FP
  instruction counts.
- LLVM `llvm.fmuladd`: https://llvm.org/docs/LangRef.html#llvm-fmuladd-intrinsic
  Used as a reference for explicit fused operations. The plan forbids implicit
  FMA in exact-order APIs.
- GCC Optimize Options: https://gcc.gnu.org/onlinedocs/gcc/Optimize-Options.html
  Used for `-O3`, `-ffp-contract`, `-ffunction-sections`, and related flags.
- GCC Warning Options: https://gcc.gnu.org/onlinedocs/gcc/Warning-Options.html
  Used for the output and warning policy.
- C `sqrt`: https://en.cppreference.com/w/c/numeric/math/sqrt
  Used for edge-case behavior. The library contract still follows Valvet and
  Julia port requirements.
- C `fmin`/`fmax`: https://en.cppreference.com/w/c/numeric/math/fmin
  Used for the NaN and signed-zero discussion. Do not assume C semantics match
  Julia without a truth table.

### Numerical methods

- SciML OrdinaryDiffEq documentation: https://docs.sciml.ai/OrdinaryDiffEq/stable/
  Used only for terminology around residuals, FSAL, and integrator caches. The
  port sources are the authority for exact formulas.
- Julia `evalpoly` documentation: https://docs.julialang.org/en/v1/base/math/
  Used for Horner-order expectations. The coefficient order must be explicit in
  the Valvet API.

## 8. Research tasks

| ID | Question | Method | Output | Blocks |
| --- | --- | --- | --- | --- |
| R-01 | Tiny-normal `sqrt` defect | Bit sweep and differential test | Corrected algorithm and regression set | FND-02 |
| R-02 | `min`/`max` NaN table | Truth table across Julia, C, and shims | Chosen contract and tests | FND-01 |
| R-03 | Repeat literal for owned types | Parser/codegen spike | v1 restriction decision | COL-01 |
| R-04 | Fixed-array ABI | Prototype `[Float64; 2]` and `[Float64; 9]` | Copy/move/pointer rule | ARR-01 |
| R-05 | Temporary lifetime extension | Borrow-checker design and fixtures | Sound rule or rejection with better diagnostic | DX-03 |
| R-06 | Bounds-check proof | Compiler analysis spike | Proven-index scope | PERF-03 |
| R-07 | Package naming | Resolver experiment | Naming convention | PKG-01 |
| R-08 | `-O3` profile value | Benchmark matrix | Opt-in or no profile | PERF-05 |
| R-09 | Large-state copy cost | Long-run benchmark | Compiler vs port fix | PERF-04 |
| R-10 | Generics and closures | Compiler design memo | Future path, no implementation | LANG-01 |
| R-11 | Port 01-04 source reconstruction | Re-fetch or restore | Better impact estimates | Impact review |
| R-12 | Benchmark host normalization | Repeat audit on a second host | CI threshold policy | TOOL-01 |

## 9. Decision log template

When a question is resolved, record:

```text
Decision:
Date:
Context:
Options considered:
Chosen option:
Why:
Evidence:
Consequences:
Follow-up:
```

Do not resolve a question by silently changing code and updating a table. The
plan and the implementation should agree.

## 10. What remains deliberately unanswered

- The exact calendar schedule. The repository has no measured porting velocity.
- The exact speedup from fixed arrays, `restrict`, large-struct ABI changes, or
  range analysis. These require implementation and measurement.
- The exact future demand for complex linear algebra, stochastic packages, or
  a CFD package. One port each is not enough evidence for a stable public API.
- The best long-term generic abstraction. Generics and closures are a research
  topic, not a prerequisite for the first payoff.
- The final package registry design. Path and git dependencies are sufficient
  for the current ecosystem.
- The exact effect on onboarding time. It must be measured, not guessed.

These gaps are acceptable. The roadmap is executable without them, and it
marks where new evidence should change the plan.
