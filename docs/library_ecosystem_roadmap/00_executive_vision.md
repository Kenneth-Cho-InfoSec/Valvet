# 00 Executive vision

## 1. Purpose

Valvet is a small, safe, compiled language for data work. It has a working
compiler, a substantial standard library, a package manifest with path and git
dependencies, and a growing fixture suite. The ten Julia-to-Valvet ports show
that the language can express serious numerical code and reproduce Julia
results bit for bit or within one ulp.

They also show where the ecosystem is still expensive to use. The problem is
not a lack of language features in general. It is a set of narrow but repeated
gaps between what the compiler provides, what the standard library exposes,
and what numerical code actually needs.

This plan turns those observations into a single engineering direction:

> Make the common numerical and collection operations reusable, allocation
> aware, exact about evaluation order, and cheap enough that library calls
> compile to code as good as the hand-written port code.

## 2. What the evidence says

### 2.1 The ports are successful but repetitive

The ten ports total about 14,956 Valvet lines. At least 4,149 of those are
generated tableaus or batteries, leaving roughly 10,800 hand-written lines.
The cross-port study identifies about 1,200 to 1,500 defensible duplicated
lines. That is about 9 percent of all port lines and roughly 11 to 14 percent
of the hand-written code. The defensible total is lower than the sum of every
duplication row because several patterns overlap. Ports 08, 09, and 10 are
bit-exact; port 07 has 10,356 exact labels and 18 one-ulp labels out of 10,374.

The duplication is concentrated in five places:

| Pattern | Occurrences | Estimated duplicated lines | Existing support |
| --- | ---: | ---: | --- |
| Correctly rounded `sqrt` block | 6 ports, 6 copies | 750 to 780 | `std.math.sqrt`, but incomplete for tiny normals |
| Vector construction, copy, scale, AXPY | 6 ports, 18 or more helpers | 350 to 450 | Partial `Vec` methods, bypassed by ports |
| RMS norms and scaled residuals | 5 ports, 5 implementations | 90 to 130 | None public |
| Scalar math shims | 7 ports, 10 or more names | 50 to 80 | None beyond `sqrt`, `exp`, `log`, `pow` |
| Complex arithmetic | 1 port, 4 functions | 31 | `std.complex` already exists, unused |

Fixed arrays and tuples, ODE scaffolding, harness code, and domain helpers add
further duplication, but are less exact and less broadly reusable. They should
not drive the first phase.

### 2.2 The strongest performance finding is not a compiler flag

The audit found four measured issues that matter more than flag tuning:

1. The hand-written correctly rounded `sqrt` is 31.56 ns per call versus 2.71
   ns for C `sqrt`, an 11.63x ratio. Replacing it in the ports was bit-exact
   for every oracle label. Warm numeric loops improved 2.25x for BDF and 1.26x
   for FIRK. The FIRK binary shrank 11 percent.
2. Port 7 makes 9,213 numeric `realloc` calls across 30 cases, about 61 per
   amortized step. FIRK makes about 17 per step, BDF about 3. Most are
   2-element vectors. `Vec::with_capacity`, exact-capacity literals, and
   fixed-size arrays address the cause.
3. Development builds keep the entire runtime prelude at `-O0`. Adding
   function and data sections plus `--gc-sections` halved the text section:
   BDF 53,699 to 25,220 bytes and weak 34,228 to 15,323 bytes.
4. The oracle printer dominates some runs: about 81 percent of Port 7 wall
   time and about 20 to 24 percent of BDF, FIRK, and weak. That is verification
   overhead, not solver cost. Port 7 also performs 87,033 label mallocs per
   run.

`-O3` helps numeric kernels by about 2.6 to 25 percent, but adds 11 to 40
percent binary size and changes end-to-end time by up to about 8 percent. It
is an optional profile, not the default. `-flto` does nothing with one
translation unit. `-march=native` is unsafe for a cross-machine bit-exact
contract.

### 2.3 The existing library is larger than the ports used

This is important for planning. Several gaps are usage or API shape problems,
not missing code:

| Capability | Existing location | Port behavior |
| --- | --- | --- |
| Complex arithmetic | `stdlib/complex.valvet:12-36` | Port 09 redefines `C09` and four functions |
| Efficient `Vec.clone` | `crates/valvet_core/src/codegen.rs:3081` | Ports 04 to 09 use push-based clones |
| `Vec.add`, `mul`, `dot`, `add_scalar` | `crates/valvet_core/src/codegen.rs:1897-1908,2541-2568` | Ports hand-roll loops; methods allocate and return `Result` |
| Real matrix LU, matvec, Cholesky, eigen, SVD | `crates/valvet_core/src/codegen.rs:2928-3047` | Ports 03 and 09 do not use it |
| Private vector and scalar helpers | `stdlib/model.valvet`, `stdlib/plot.valvet`, `stdlib/infer.valvet` | Not exported, duplicated inside stdlib |

The plan therefore separates:

- **Missing**: no equivalent exists.
- **Partial**: an equivalent exists but not for the needed type or operation.
- **Existing but unused**: the library exists and should be adopted before a
  replacement is designed.
- **Private**: useful code exists but is not public.
- **Language/API**: the gap is syntax, type system, borrow rules, or compiler
  lowering rather than a function.

## 3. Root causes

The repeated code is a symptom of six architectural problems.

### R1. The mathematical foundation is incomplete

`std.math` exports only `sqrt`, `exp`, `log`, and `pow`. `sin`, `cos`, and
`atan2` are compiler builtins. There is no public `abs`, `min`, `max`, `floor`,
`ceil`, `eps`, `sinh`, `cosh`, `acosh`, `sincos`, or `evalpoly`. Port 04 needed
more than 30 shim call sites; ports 05, 08, 09, and 10 each carry another
`abs` or `max_abs` copy.

The `sqrt` situation is worse than a missing function because the standard
library now has the right algorithm but still has a tiny-normal last-bit gap.
Ports copied the algorithm six times rather than trust the library. A library
that is almost correct is not a foundation.

### R2. Generic collection operations live in the compiler, but the API is thin

`Vec<T>` is a compiler-instantiated type. It has `new`, `push`, `pop`, `get`,
`len`, `clone`, `slice`, `cast`, `add`, `mul`, `add_scalar`, `dot`, and `sort`.
That is enough to build code, but not enough to build allocation-aware
numerical code:

- No `with_capacity`, `reserve`, `resize`, `fill`, or `copy_from`.
- No repeat literal `vec![value; n]`.
- `vec![...]` is built by repeated push, so even a fixed literal reallocates.
- `add` and `mul` allocate a fresh vector and return
  `Result<Vec<T>, ShapeError>`, which is inconvenient in exact-order kernels.
- There is no in-place `axpy` or exact-order multi-term update.

This is why ports hand-roll loops and why the standard library cannot simply
provide a generic `std.numeric`: user functions cannot be generic over `T`
yet. Generic vector primitives must remain compiler kernels or wait for
monomorphization.

### R3. Small fixed-size values have no first-class representation

Ports 01 and 02 use `V2`, `V3`, `V4`, and `V9` structs to replace Julia
`SVector`. Port 06 uses paired `Vec<Float64>` fields for `ArrayPartition`.
Port 03 flattens `Matrix{T}` into a row-major `Vec<Float64>` because `Mat` has
no general mutable element index.

These are deliberate workarounds, but they leak into every kernel. They make
code longer, make allocation behavior worse, and prevent constant-index bounds
checks from disappearing. The clean fix is compiler support for `[T; N]` and
tuple returns, not a library of `Vec2`, `Vec3`, and `Vec4` types.

### R4. Useful numeric code is fragmented across the standard library

`std.model`, `std.plot`, and `std.infer` contain private vector and scalar
helpers that are not exported. There are at least 17 abs-like patterns across
the standard library. New numerical code has no obvious place to import
`vec_add`, `vec_mean`, `sum_sq`, `fmin`, `fmax`, or `fsd`, so it copies them.

This is a library architecture problem. The port work is simply making it
visible.

### R5. Code generation penalizes reusable code in several places

The audit found these concrete costs:

- Push-based construction and cloning cause measurable `realloc` traffic.
- Bounds checks appear even for constant indices. Port 7 has 1,051 bounds
  checks in generated C.
- Large state structs are passed by value. Port 7 copies a 536-byte state per
  step.
- The development profile emits and keeps the whole runtime prelude.
- Literal `print` and the three-line oracle protocol allocate heavily.
- `sqrt` is a software function rather than a guarded C builtin.
- `std.math.exp`, `log`, and `pow` are software implementations; the audit did
  not measure their full cost, so this remains a likely rather than verified
  bottleneck.

A library can be well designed and still be slower if it allocates, returns
`Result` on every element, or defeats inlining. The plan therefore treats
code generation as part of library design.

### R6. Developer feedback and porting workflow are underbuilt

The ports needed manual oracle generators, manual label printers, manual
comparison scripts, manual ASan commands, and manual allocation counters.
There is no repository benchmark harness, no standard generated-code snapshot,
and no standard porting template. The compiler emits warnings that the example
drivers cannot avoid: W1002 on `print` and W1003 on redundant `as Int32`.

The most visible syntax friction is integer literal typing. The retained port
sources contain 589 occurrences of `as Int64`, of which 241 are in generated
port 07 code. That is not a numerical requirement. It is a language ergonomics
problem that also makes generated code noisy.

## 4. Target state

The target is a layered ecosystem with clear ownership.

```
Layer 0  Compiler and runtime primitives
         Vec, Mat, [T; N], tuple returns, sqrt/sin/cos builtins,
         bounds checks, allocation, print/write, C11 backend

Layer 1  Core standard library
         std.math, std.numeric, std.complex, std.linalg

Layer 2  General data libraries
         std.stats, std.dist, std.frame, std.model, std.plot,
         std.sql, std.infer, std.re, and friends

Layer 3  Domain packages
         ode, stochastic, cfd, quadrature, harness
         (external packages by default, not std.*; benchmark tooling
         is part of Layer 4 and TEST-01)

Layer 4  Tooling and documentation
         benchmarks, generated-code snapshots, porting guide,
         API reference, package discovery
```

The layers have a strict dependency direction. Layer 1 may use Layer 0. Layer
2 may use Layers 0 and 1. Domain packages may use any lower layer. Lower layers
never import domain packages. `std.*` is reserved for broadly reusable,
stable APIs; domain-specific solvers stay outside it unless a second and third
consumer justify promotion.

The first release target is not a large ODE framework. It is a small set of
foundations that make the next ten ports cheaper:

- A complete, tested `std.math`.
- A correctly rounded, guarded `sqrt` builtin.
- An exact-order `std.numeric` for `Vec<Float64>`.
- Collection primitives that allocate once.
- A warning-free output path and a no-newline `write` primitive.
- Compiler fast paths for the operations above.
- A benchmark and oracle harness that makes regressions visible.

## 5. Key decisions

These decisions constrain the rest of the plan.

### D1. Fix foundations before building domain packages

`std.math` and `std.numeric` come before `ode`, `stochastic`, or `cfd`. The
ports prove the foundation gaps are reused; the wider domain packages are not
yet proven across enough consumers.

### D2. Keep exact evaluation order in the public contract

`std.numeric.axpy(a, x, b, y)` means `a*x[i] + b*y[i]` with two multiplies and
one add, in that order. `axpy3` and `axpy4` preserve their source grouping.
`rms` sums in increasing index order. `scaled_residual` keeps the source
parentheses. No FMA, reassociation, or fast-math is allowed in these APIs.

This is not an implementation detail. The ports depend on it for bit-exact
results.

### D3. Separate allocating and in-place APIs

The default function returns a fresh value and is easy to read. A suffix
`_into` writes into a caller-owned destination and is the hot-loop form:

```valvet
pub fun axpy(a: Float64, x: &Vec<Float64>, b: Float64, y: &Vec<Float64>): Vec<Float64>
pub fun axpy_into(dst: &mut Vec<Float64>, a: Float64, x: &Vec<Float64>, b: Float64, y: &Vec<Float64>)
```

The allocating form must not be used in a stage loop where the destination is
already available. This gives readers a safe default and performance-sensitive
code an explicit choice.

### D4. Generic operations belong in the compiler, concrete algorithms in the library

`Vec<T>` methods are monomorphized by the compiler. Those are the right place
for `with_capacity`, `reserve`, `resize`, `fill`, `copy_from`, and exact
capacity literals. `std.numeric` can only be concrete until generics land, so
its first version targets `Vec<Float64>`, with `Int64` variants only where a
real consumer exists.

Do not build a generic numeric framework on top of named-function callbacks.
It will be harder to read and may be slower.

### D5. Use existing capabilities before adding replacements

Port 09 should import `std.complex`. Ports should use `Vec.clone` instead of a
push loop. Real `Mat` LU, matvec, Cholesky, eigen, and SVD already exist and
should be used where their semantics fit. A new library must state which
existing capability it replaces and why the existing one is insufficient.

### D6. Fixed arrays and tuples are language work, not a `Vec2` library

The best fix for `SVector` and `ArrayPartition` is `[T; N]` and tuple returns.
An interim `std.fixed` is acceptable only if compiler work is delayed and a
port explicitly needs it. A general `Matrix<T>` should not be built before
`Mat` has mutable element access.

### D7. Domain packages stay outside `std.*` by default

`ode`, `stochastic`, `cfd`, `quadrature`, and `harness` are domain or workflow
packages. Benchmark tooling is part of `TEST-01`, not a separate package in
this roadmap. They can live in path or git dependencies using the existing
manifest support. No public registry is required for this plan. A domain
capability is promoted to `std.*` only after multiple independent consumers
and a stable API.

### D8. Performance claims need machine-code evidence

A source-line reduction is not a performance result. Every performance claim
must name one of:

- A measured benchmark from the audit.
- A generated-C or disassembly observation.
- An estimated target with a stated assumption.
- An unverified hypothesis with a planned benchmark.

The plan does not claim that every abstraction is faster. It identifies where
a library must be paired with a compiler fast path to avoid a regression.

## 6. Expected results

### Measured or directly supported

- Replacing the local `sqrt` with hardware or libm `sqrt` was bit-exact for
  all BDF, FIRK, and weak labels, and improved warm numeric loops by 2.25x and
  1.26x for BDF and FIRK.
- The local helper is 11.63x slower per call than C `sqrt`.
- Port 7 numeric allocations are 9,213 `realloc` calls across 30 cases; FIRK
  about 17 per step; BDF about 3 per step.
- Development text size can be roughly halved with section flags.
- The oracle printer is the largest single cost in Port 7, about 81 percent.
- `-O3` is a size-for-speed trade, not a default win.
- Complex arithmetic, `Vec.clone`, and real matrix LU already exist.

### Estimated

- The P0 and P1 core library work, before fixed arrays and packages, can remove
  roughly 1,050 to 1,450 lines of port duplication.
- The full deduplicated opportunity is about 1,200 to 1,500 lines, roughly 11
  to 14 percent of hand-written port code.
- A comparable explicit ODE port could need 10 to 25 percent less new code;
  a scaffolding-heavy battery could need 25 to 40 percent less. These are
  estimates from duplication share, not measured porting times.
- Exact-capacity literals and fixed arrays can remove most 2-element vector
  allocations in BDF and FIRK, and reduce Port 7's allocation count. The
  exact reduction is unverified until implemented.

### Unverified

- The full cost of software `exp`, `log`, and `pow` in ports 01 and 02.
- The benefit of `restrict` or range-based bounds-check elision on larger
  vectors.
- The effect of large-struct-by-pointer on long simulations.
- The benefit of a public `std.ode` or `std.stochastic` module before a
  second consumer exists. The roadmap keeps both in packages, and the
  `stochastic` v1 is limited to the two helpers port 10 proves.
- The exact line counts for fixed-vector workarounds in ports 01 and 02,
  because their sources are not retained.

## 7. Non-goals

- A generic ODE solver framework. The stage kernels, caches, tableaus, and
  counter asymmetries are algorithm-specific.
- A general broadcasting engine with callbacks or closures.
- A BLAS replacement, general complex linear algebra, or a `Matrix<T>` before
  a concrete consumer exists.
- A public package registry in the first phases. Path and git dependencies
  already work.
- Default `-O3`, `-ffast-math`, `-Ofast`, approximate transcendental
  functions, or `-march=native`.
- Global removal of bounds checks.
- Copying Julia APIs without adapting them to Valvet's ownership, borrow, and
  exactness rules.
- Committing ported code.

## 8. Success measures

The plan succeeds when these are true:

1. Six local `sqrt` implementations can be replaced by one public function or
   builtin with no oracle loss.
2. `std.math` covers the scalar operations used by ports 01, 02, 04, 05, 08,
   09, and 10, with tested NaN, signed-zero, zero, infinity, and
   subnormal behavior.
3. `std.numeric` has one tested exact-order vector contract and one residual
   contract, used by at least two ports before it is declared stable.
4. `vec![value; n]`, exact-capacity `vec![...]`, and `Vec.with_capacity`
   remove repeated `realloc` in at least BDF and FIRK.
5. The warning-free output path preserves the three-line oracle format, and
   `write` does not append a newline.
6. Development builds no longer carry unused runtime code.
7. A benchmark harness can report kernel time, end-to-end time, allocations,
   binary size, and oracle equality for every port.
8. Fixed arrays and tuples are designed and, if implemented, eliminate the
   `V2`/`V3`/`V4`/`V9` workarounds and paired `ArrayPartition` fields.
9. Every phase passes the repository gates and every arithmetic change has a
   bit-exact or 1e-12 oracle.
10. A future port author can start from a template, import the foundations,
    and avoid reimplementing math, vector algebra, residuals, and output.

## 9. How to read the rest

- `01_ecosystem_architecture.md` defines the module and package boundaries.
- `02_library_opportunities.md` gives the detailed evidence and API proposals.
- `03_performance_and_machine_code.md` defines the performance contract.
- `04_developer_experience.md` shows the current and proposed code.
- `05_implementation_roadmap.md` orders the work.
- `06_actionable_task_list.md` is the execution backlog.
- `07_impact_estimates.md` separates measured from estimated impact.
- `08_open_questions_and_research.md` records the decisions that remain open.

The first authorized implementation step, if the user requests it, is P0:
finish and test `std.math`, add the guarded `sqrt` builtin, add the first
`std.numeric` primitives and collection allocation APIs, retire the `print`
warnings and add `write`, and migrate ports 08 and 10 as the smallest
bit-exact oracles.
