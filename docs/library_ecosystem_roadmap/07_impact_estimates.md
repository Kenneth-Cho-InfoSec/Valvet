# 07 Impact estimates

## 1. Evidence classes

Every number below is labeled:

- **Measured**: observed in the ports, generated C, disassembly, or the audit
  harness.
- **Estimated**: derived from measured line counts, occurrence counts, or
  explicit assumptions. The assumption is stated.
- **Unverified**: plausible but requires implementation or a benchmark.

The plan does not treat a source-line reduction as a performance result.

## 2. Baseline

| Metric | Value | Class | Source |
| --- | ---: | --- | --- |
| Total Valvet port lines | 14,956 | Measured | XRD 2.1 |
| Generated tableaus and batteries | at least 4,149 | Measured | XRD 2.1 |
| Hand-written port lines | about 10,807 or less | Derived | 14,956 minus at least 4,149 |
| Defensible duplicated lines | 1,200 to 1,500 | Estimated | XRD 2.5, after overlap removal |
| Duplicated share of all port lines | 8.0% to 10.0% | Derived | 1,200/14,956 to 1,500/14,956 |
| Duplicated share of hand-written lines | 11.1% to 13.9% | Derived | 1,200/10,807 to 1,500/10,807 |
| Complete-source ports | 07, 08, 09, 10 | Measured | XRD 2.1 |
| Report-only ports | 01, 02, 03, 04 | Measured | XRD 2.1 |
| Partial-source ports | 05, 06 | Measured | XRD 2.1 |
| Standard-library modules | 29 | Measured | `stdlib/*.valvet` file count |
| Standard-library lines | 8,623 | Measured | `wc -l stdlib/*.valvet` |
| Retained `as Int64` occurrences | 589 | Measured | Port source count; 241 generated |
| Compiler version | 0.4.0 | Measured | `crates/*/Cargo.toml` |
| Port 07 oracle labels | 10,374; 10,356 exact, 18 one-ulp | Measured | XRD 5 |

The total duplicated range is deliberately conservative. The gross ranges in
the next section overlap and sum to more than 1,500 lines.

## 3. Duplication reduction

### 3.1 Gross opportunity by pattern

| Pattern | Occurrences | Gross lines | Existing support | Confidence |
| --- | ---: | ---: | --- | --- |
| Correctly rounded `sqrt` | 6 ports, 6 copies | 750 to 780 | `std.math.sqrt`, tiny-normal gap | High for 05/08/09/10; report for 04/06 |
| Vector construction and algebra | 6 ports, 18+ helpers | 350 to 450 | Partial `Vec` methods | High for 05-09; report for 04 |
| RMS and scaled residuals | 5 ports, 5 implementations | 90 to 130 | None public | High |
| Scalar math shims | 7 ports, 10+ names | 50 to 80 | None beyond `sqrt`/`exp`/`log`/`pow` | High for 04/05/08/09/10; unverified 01/02 |
| Complex arithmetic | 1 port, 4 functions | 31 | `std.complex` | Measured |
| Fixed vectors and matrices | 4 ports, 4 patterns | 100 to 300 | `Vec`, real `Mat`, no fixed arrays/tuples | Unverified for 01/02 |
| ODE scaffolding | 7 ports | 150 to 250 | None public | Medium; structural not exact |
| Harness and RHS fixtures | 7 printers, 3 RHS fixtures | 100 to 200 | None; `print` deprecated | High for 07-10 |
| Domain helpers | 4 patterns | 40 to 80 | None | Unverified for 01/02 |
| Standard-library private helpers | 10 helpers, 17 abs patterns | 50 to 100 | Private | Estimated |

Gross total: about 1,700 to 2,400 lines. Do not use this as the savings
estimate. It double-counts helpers that appear in multiple rows. The
standard-library row is internal library code, not port code, and is excluded
from the deduplicated port total below.

### 3.2 Defensible deduplicated total

| Scope | Estimated removable lines | Share of all ports | Share of hand-written ports | Confidence |
| --- | ---: | ---: | ---: | --- |
| P0/P1 core (math, sqrt, vector, norms, complex) | 1,050 to 1,450 | 7.0% to 9.7% | 9.7% to 13.4% | Estimated; high for ports 05-10 |
| P1 representation (fixed arrays, tuples, matrix) | 100 to 300 | 0.7% to 2.0% | 0.9% to 2.8% | Estimated; unverified for 01/02 |
| P2 ODE protocol and harness | 250 to 450 | 1.7% to 3.0% | 2.3% to 4.2% | Estimated; partly test-only |
| Deduplicated total | 1,200 to 1,500 | 8.0% to 10.0% | 11.1% to 13.9% | Estimated |

The scope rows overlap with the total. Use the total, not the sum of rows,
when describing the overall opportunity.

### 3.3 What "removable" means

A removed helper does not remove every call site. A 128-line `sqrt` block
becomes a one-line import or a builtin call. A 15-line `axpy4` helper becomes a
one-line library call. The call site remains, so the net line reduction is
slightly less than the helper count.

The estimates above are deleted helper lines, not a claim that each port
shrinks by the same percentage. Ports with more generated data will shrink by
a smaller percentage.

## 4. Duplicate implementations eliminated

| Capability | Before | After | Reduction | Class |
| --- | ---: | ---: | ---: | --- |
| Correctly rounded `sqrt` | 6 | 1 | 5 implementations | Estimated from XRD |
| Vector helper families | 18+ | 1 library | 17+ implementations | Estimated from XRD |
| RMS/residual implementations | 5 | 1 contract | 4 implementations | Estimated from XRD |
| Scalar math shim names | 10+ | 1 module | 9+ names | Estimated |
| Complex arithmetic | 4 functions | Existing `std.complex` | 4 functions | Measured |
| Fixed-vector patterns | 4 | Compiler arrays/tuples | 4 workarounds | Estimated |
| ODE state/cache scaffolding | 7 | Narrow primitives | Depends on package | Estimated |
| Harness printers | 7 | Optional 1 | 6 | Estimated |
| Trixi log-mean helpers | 2 ports | 1 library | 1 implementation | Estimated |
| Stdlib private helpers | 10 | 2 public modules | 8 implementations | Estimated |

This is the maintainability case: one semantic contract, one test suite, one
place to fix a bug.

## 5. Ports benefiting

| Capability | Ports with evidence | Count | Future reach |
| --- | --- | ---: | --- |
| `std.math` scalar | 01, 02, 04, 05, 08, 09, 10 | 7 | All numeric code |
| `sqrt` | 01, 02, 04, 05, 06, 08, 09, 10 | 8 | All numeric code |
| `std.numeric` vector | 04, 05, 06, 07, 08, 09 | 6 | All vector code |
| Collection capacity/literals | 04, 05, 06, 07, 08, 09 | 6 direct allocation | All vector code |
| Norms/residuals | 04, 05, 06, 08, 09 | 5 | ODE/SDE code |
| `std.complex` | 09 | 1 now | Future FIRK |
| Fixed arrays/tuples | 01, 02, 03, 06 | 4 | Small-vector code |
| `Mat` get/set | 03 | 1 now | Matrix code |
| Complex 2x2 | 09 | 1 now | Future implicit solvers |
| ODE primitives | 04, 05, 06, 07, 08, 09, 10 | 7 | ODE packages |
| Harness | 04, 05, 06, 07, 08, 09, 10 | 7 | Future ports |
| Stochastic | 10 | 1 now | Future weak SDE |
| Trixi log-mean | 01, 02 | 2 | Future CFD |
| Tooling and docs | 01-10 | 10 | Entire ecosystem |

The strongest cross-project leverage is `std.math` (7 ports), `sqrt` (8 ports),
and `std.numeric`/collection work (6 ports). Complex and fixed arrays are lower
port counts but unlock whole code shapes.

## 6. Missing Julia-equivalent capabilities addressed

The cross-port study identifies 13 gap groups, G1 to G13.

| Phase | Gap groups addressed | Count |
| --- | --- | ---: |
| P0/P1 | G1 scalar math, G2 sqrt, G3 vector algebra, G4 norms, G5 complex, G6 linalg, G7 fixed/tuples, G11 Gridap matrix, G13 stdlib helpers | 9 |
| P2 | G8 ODE protocol, G9 stochastic, G10 Trixi helpers | 3 |
| No action | G12 rationals/tableaus, deliberately kept as generators | 1 |

This means 9 of 13 are addressed in P0/P1, and up to 12 of 13 if the P2
packages are justified. G12 should not be "fixed"; generated literals are the
right answer.

## 7. Measured performance effects

These are the only performance changes supported by the audit. They are
directional for the audit host and toolchain.

| Change | Result | Class |
| --- | ---: | --- |
| Local `sqrt` to C `sqrt` per call | 31.56 ns to 2.71 ns, 11.63x | Measured |
| BDF warm numeric loop with C `sqrt` | 829.6 to 369.1 ns/step, 2.25x | Measured |
| FIRK warm numeric loop with C `sqrt` | 663.5 to 528.6 ns/step, 1.26x | Measured |
| FIRK binary size with C `sqrt` | 37,536 to 33,440 bytes, -11% | Measured |
| Weak with C `sqrt` | Not measurable; loop removed by GCC | Measured limitation |
| Port 07 output share | 81.0% of wall time | Measured |
| BDF/FIRK/weak output share | 19.6%, 22.3%, 23.9% | Measured |
| Dev text size with sections/gc | BDF -53.0%, weak -55.2% | Measured |
| `-O3` numeric loop gain | 2.6% to 25% | Measured |
| `-O3` binary growth | 11% to 40% | Measured |
| `-O3` end-to-end gain | Up to about 8% | Measured |

The plan does not claim a weak `sqrt` speedup. It does not claim that
`std.numeric` is faster than a hand-written loop before it is measured. It
does not claim that `-O3` is a default win.

## 8. Measured allocation baseline and targets

| Port | Numeric reallocs per step | Current cause | Estimated target | Class |
| --- | ---: | --- | --- | --- |
| 08 BDF | 3.0 | push-based clone and zeros | 0 to 1 per step | Estimated |
| 09 FIRK | 17.2 | repeated 2-element `zeros`/`clone` in stages | 2 to 5 per step | Estimated |
| 07 extrap | 61.4 | small vectors plus coefficient vectors | Near zero per step if scratch is reused; one allocation per coefficient set | Unverified |
| 10 weak | 0 | scalar branch | 0 | Measured |

The BDF and FIRK targets assume fixed arrays or exact-capacity vectors plus
scratch reuse. They are not measured results. Port 07 may still allocate for
coefficient vectors and for any dynamic dispatch that cannot be baked.

## 9. Developer experience impact

| Metric | Current | Expected after P0/P1 | Class |
| --- | ---: | --- | --- |
| Local `sqrt` blocks | 6 | 0 | Estimated |
| `as Int64` in retained port sources | 589 | Near zero in new code | Estimated |
| W1002 warnings per port run | 2 | 0 | Measured baseline; estimated fix |
| Scalar shim names | 10+ | 0 local | Estimated |
| Vector helper implementations | 18+ | 1 library | Estimated |
| Residual implementations | 5 | 1 contract | Estimated |
| Complex duplicate types | 1 | 0 | Measured |
| Harness printers | 7 | Optional 1 | Estimated |
| Stdlib private helper families | 3 modules | 2 public modules | Estimated |

The `as Int64` metric depends on DX-01. Without contextual inference, the
library work does not reduce those casts.

## 10. Future port effort

### Estimated

- A comparable explicit ODE kernel could need **10 to 25 percent less new
  code**. Basis: the duplicated helper share in ports 04 to 09, after removing
  generated data and algorithm-specific kernels.
- A scaffolding-heavy battery could need **25 to 40 percent less new code**.
  Basis: harness, vector, math, and residual helpers dominate the small ports,
  and those are the exact targets.
- A future small bit-exact port could go from a multi-file setup with a custom
  `sqrt`, `zeros`, `clone`, `abs`, residual, and printer to a file that imports
  `std.math`, `std.numeric`, and a label emitter.

These are estimates from source-share evidence, not measured porting times.
The repository has no recorded time-per-port data.

### Unverified

- A native data-analysis program outside the ODE domain may see a smaller
  benefit, because the evidence is concentrated in numerical kernels.
- A port with heavy generated tableaus may see little percentage reduction,
  because generated data dominates its line count.
- The effect on onboarding time is not measured.

## 11. Maintainability impact

Qualitative but directly supported:

- One `sqrt` implementation instead of six removes a class of last-bit
  divergence.
- One residual contract removes five chances to change summation order.
- One vector library removes 18+ chances to choose different allocation
  behavior.
- One complex library removes a duplicate type and formula set.
- One `Mat` access path removes a parallel flat-matrix representation.
- One benchmark harness turns ad hoc timing into a durable baseline.

The plan does not assign a dollar value or a percentage to maintainability.

## 12. What is not counted

- Generated tableaus and batteries. They are data, not duplicate logic.
- Algorithm-specific stage kernels. They should remain separate.
- Oracle-side Julia code. It must remain independent.
- Test-only harness lines in the total product code estimate. They are listed
  separately.
- `NUM-04`, `STOCH-01`, and the small domain helpers. They are outside the
  deduplicated port total and are counted only where a concrete package
  migration exists.
- Any speedup from `restrict`, range analysis, or large-struct ABI changes.
  These are unverified.
- Any compile-time improvement. It was not measured.
- Any improvement to unrelated data-science modules. The evidence is from
  numerical ports.

## 13. Impact scorecard

| Area | Measured evidence | Estimated impact | Main risk |
| --- | --- | --- | --- |
| Code reuse | 1,200 to 1,500 duplicated lines | 8% to 10% of all port lines; 11% to 14% of hand-written | Overlap and missing sources |
| Math correctness | 337 tiny-normal `sqrt` mismatches | 0 after fix, 6 copies removed | Edge cases |
| Numeric speed | 11.63x per `sqrt`, 2.25x/1.26x loops | Same or better in library paths | Edge-case fallback |
| Allocation | 61.4/17.2/3.0 reallocs per step | 0 to 5 per step for BDF/FIRK | Scratch reuse not implemented |
| Dev build size | 53% to 55% text reduction | Same for other ports | None measured |
| DX | 589 casts, 6 sqrt copies, 2 W1002 per run | Near-zero casts, one math foundation, zero W1002 | Literal inference and output migration |
| Ecosystem | 29 stdlib modules, path/git packages | Clear core/package split | Premature packages |
| Future ports | No measured time data | 10% to 40% less code for similar ports | Estimate, not a promise |

## 14. How to validate the estimates

After each phase:

1. Re-count duplicated helper lines in the migrated ports.
2. Re-run allocation counters.
3. Re-run the oracle harness and record exact and one-ulp labels.
4. Re-run warm numeric loops and end-to-end time.
5. Re-run dev and release size measurements.
6. Count `as Int64`, W1002, and local math helpers in new code.
7. Compare the result with this document and update the estimate class.

If an estimate is wrong, change the document. Do not preserve a number because
it was in the plan.
