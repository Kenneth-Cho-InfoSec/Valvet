# 03 Performance and machine code

## 1. Baseline

The current backend emits one C11 translation unit and builds it with GCC.
The audit recorded:

- Development: `-O0 -g`.
- Release: `-O2`.
- Both: `-std=c11 -fwrapv -pthread -lm`.
- Optional: `-flto`, which has no effect with one translation unit.

No packed floating-point arithmetic was found in the port binaries. The 2D
ODE kernels are too small for vectorization to be a useful target. Bounds
checks are frequent but predictable. The biggest controllable costs are not
compiler flags.

Every performance recommendation in this plan must be read against that
baseline.

## 2. Measured findings

### 2.1 Printer cost

| Port | Labels | Output lines | Normal wall ms | Numeric wall ms | Output share | Mallocs per label |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 07 extrap | 10,374 | 31,122 | 5.261 | 0.998 | 81.0% | 8.4 |
| 08 BDF | 280 | 840 | 0.710 | 0.571 | 19.6% | 3.4 |
| 09 FIRK | 332 | 996 | 0.714 | 0.555 | 22.3% | 3.3 |
| 10 weak | 328 | 984 | 0.694 | 0.528 | 23.9% | 3.3 |

Port 7 also makes 87,033 `malloc` calls, essentially all label construction.
The numerical solver makes zero `malloc` calls. This is verification overhead,
not solver cost. Do not judge ODE kernel optimization by total port wall time.

### 2.2 Square root

| Measurement | Result |
| --- | ---: |
| Local helper per call | 31.56 ns |
| C `sqrt` per call | 2.71 ns |
| Ratio | 11.63x |
| BDF warm numeric loop, helper to C sqrt | 829.6 to 369.1 ns/step, 2.25x |
| FIRK warm numeric loop, helper to C sqrt | 663.5 to 528.6 ns/step, 1.26x |
| FIRK binary size, helper to C sqrt | 37,536 to 33,440 bytes, -11% |

The replacement was bit-exact for all BDF, FIRK, and weak oracle labels. Weak
was not measurable in the speed battery because `dt` is a compile-time
constant and GCC removes the loop. Do not cite a weak speedup.

### 2.3 Vector allocation

| Port | Full-run malloc | Full-run realloc | Numeric malloc | Numeric realloc | Per step |
| --- | ---: | ---: | ---: | ---: | ---: |
| 07 extrap | 87,033 | 9,213 | 0 | 9,213 | 61.4 |
| 08 BDF | 961 | 60 | 36 | 60 | 3.0 |
| 09 FIRK | 1,081 | 344 | 0 | 344 | 17.2 |
| 10 weak | 1,093 | 0 | 0 | 0 | 0 |

Port 7's 9,213 numeric reallocs request 538,656 bytes. The sizes are often 2
elements; coefficient vectors reach 11, 121, and 242 elements. The cause is
push-based construction and manual push-based clones. The runtime already has
an efficient clone that allocates once.

### 2.4 Bounds checks and vectorization

| Port | Bounds checks in generated C | Packed FP instructions |
| --- | ---: | ---: |
| 07 extrap | 1,051 | 0 `addpd`/`mulpd`/`divpd` |
| 08 BDF | 53 | 0 |
| 09 FIRK | 90 | 0 |
| 10 weak | 2 | 0 |

Constant indices still carry checks. The checks are predictable and are not
the top cost in these batteries. Do not remove them globally. Fixed arrays, a
proven-length path, or an explicit unchecked block are the safe options.

### 2.5 Development build size

| Port | `-O0 -g` text | With sections and gc | Reduction |
| --- | ---: | ---: | ---: |
| 08 BDF | 53,699 | 25,220 | 53.0% |
| 10 weak | 34,228 | 15,323 | 55.2% |

File sizes including debug info fell from 110,432 to 83,072 for BDF and
74,808 to 55,264 for weak. Output was byte-identical. Release `-O2` already
drops unused statics, so this is a development-build fix.

### 2.6 Compiler flags

| Port | `-O2` ms | `-O3` ms | `-O2 -flto` ms | `-O3 -march=native` ms |
| --- | ---: | ---: | ---: | ---: |
| 07 extrap | 5.273 | 4.831 | 5.277 | 4.788 |
| 08 BDF | 0.761 | 0.733 | 0.754 | 0.746 |
| 09 FIRK | 0.756 | 0.745 | 0.753 | 0.750 |
| 10 weak | 0.734 | 0.730 | 0.730 | 0.724 |

Warm numeric loops:

| Port | `-O2` ns/step | `-O3` ns/step | Change |
| --- | ---: | ---: | ---: |
| 07 extrap | 1,941 | 1,711 | -11.9% |
| 08 BDF | 882 | 859 | -2.6% |
| 09 FIRK | 666 | 538 | -19.1% |
| 10 weak | 90.8 | 68.4 | -24.6% |

`-O3` outputs were byte-identical, but binary sizes grew 10.9 to 40.3 percent.
End-to-end gain was up to about 8 percent because output and process startup
dominate. `-march=native` is not safe for cross-machine bit-exact work.

### 2.7 Large state copies

Port 7's step functions take and return `VState07` by value. The struct is 536
bytes. The caller performs a `memcpy` around each step. BDF and FIRK step
functions inline into their run functions, so they avoid per-step ABI copies
but still have large stack frames:

| Port | Run stack frame |
| --- | ---: |
| 07 extrap | 4,392 bytes |
| 08 BDF | 3,112 bytes |
| 09 FIRK | 4,584 bytes |
| 10 weak | 232 bytes |

This matters more for long simulations with large state and no diagnostic
printer. It is not the first fix for the current batteries.

## 3. Performance contract for every library

### 3.1 Exact arithmetic

The following are non-negotiable for bit-exact numerical code:

- No FMA unless the API explicitly says `fma` and both sides of the oracle
  agree.
- No reassociation of floating-point additions or multiplications.
- No `-ffast-math`, `-Ofast`, or approximate `sin`, `cos`, or `sqrt`.
- Left-to-right evaluation where the source depends on it.
- Preserve explicit parentheses in residual and norm formulas.
- `axpy3` and `axpy4` keep the source grouping; they are not nested `axpy`
  calls.
- `sqrt` must be correctly rounded for all inputs the library admits.

### 3.2 Allocation

- Constructors allocate once.
- `_into` functions do not allocate when the destination has enough capacity.
- `clone` uses the runtime clone, not a push loop.
- No function allocates a temporary per element.
- No function hides a growing vector inside a loop.

### 3.3 Inlining and specialization

The compiler should be able to inline or specialize:

- `Vec.push` in hot loops.
- Small `Vec.get`/index paths where the index is proven.
- Simple `std.math` wrappers.
- `std.numeric` functions over fixed-size or known-size vectors.

The current `Vec` kernels are out-of-line static C functions. That is fine for
large vectors but adds call overhead and prevents some optimization for
2-element kernels. The compiler work in PERF-02 should make simple kernels
inline-friendly without forcing inlining of large loops.

### 3.4 Data layout and locality

The current hot data is contiguous: `Vec<Float64>` buffers and row-major
`Mat`. Preserve that layout:

- Use one contiguous buffer per vector. Do not replace a 2-element vector with
  `Vec<Vec<Float64>>` or another pointer-chasing shape.
- Keep `Mat` row-major and iterate in storage order.
- Prefer inline `[Float64; N]` for small fixed state, but measure copy cost
  before changing argument passing.
- Reuse scratch buffers. A temporary per element or per stage defeats locality
  even when total allocation volume is small.
- Port 07 still copies a 536-byte state per step. Measure that copy before an
  ABI change.

The audit did not measure cache misses. Locality claims are unverified until
profiled; do not claim a speedup from layout alone.

### 3.5 Borrow and aliasing

- Read-only inputs are borrowed.
- Outputs are mutable and may not alias inputs unless documented.
- Generated C should eventually use `restrict` only where the language proves
  no aliasing. This is a later optimization, not a P0 change.
- Do not add `restrict` by default to APIs that permit aliasing.

### 3.6 Error paths

- Shape-checked APIs return `Result`; exact-order vector primitives document
  equal-length preconditions and do not return `Result` in hot loops.
- A shape check outside a hot loop is fine.
- A shape check inside a per-element loop is not acceptable unless the API is
  explicitly checked.
- Use one check per call and then a tight loop.

## 4. Per-library performance analysis

### 4.1 `std.math` and `sqrt`

**Measured benefit:** replacing the local `sqrt` is the largest verified
numeric win: 11.63x per call, 2.25x BDF, 1.26x FIRK.

**Risk:** the tiny-normal path and edge cases. The library wrapper must keep
the current contract. The builtin must not be used where it changes the last
bit.

**Design:** guarded builtin for positive inputs outside the tiny-normal
fallback band, library fallback for everything else. Do not use `-ffast-math`.
Use `-fno-math-errno` only with the builtin.

**Validation:** 1,000,000-value tiny-normal sweep, the existing math fixture,
BDF/FIRK/weak oracle comparisons, and emitted-C inspection for `sqrt`.

### 4.2 `std.numeric` vector primitives

**Measured problem:** 61.4 numeric reallocs per Port 7 step, 17.2 per FIRK
step, 3.0 per BDF step.

**Expected benefit (estimated):** exact-capacity allocation and `_into` forms
can remove most 2-element allocations and reduce coefficient-vector growth to
one allocation per case. Do not claim a specific speedup until measured.

**Risk:** a library function that allocates a fresh result on every call can
be slower than the current hand-written in-place loop. The `_into` API is the
mitigation. The allocating form is for readability, not hot loops.

**Machine code:** simple loops over `Vec<Float64>` should compile to the same
loads, multiplications, and adds as the hand-written loop. Check that no
`Result` branch, bounds check, or function call remains inside the inner loop.
The compiler may need a fast path for `Vec.get` and `Vec.push`, or the library
may need direct indexing.

**Validation:** allocation counters, warm numeric loops, generated C diff,
ASan, oracle equality.

### 4.3 Norms and residuals

**Measured problem:** five separate implementations, but no isolated speed
measurement. The formula is not the dominant cost in the current batteries.

**Expected benefit (estimated):** line reduction and one tested contract. Do
not promise a speedup.

**Risk:** summation order and `max`/`abs` semantics. A shared `rms` that uses
a different order changes bits. Keep the exact order and test it.

**Machine code:** the loop should be the same as the current hand-written
version. Avoid an allocation for the residual vector only when a measured hot
path needs it; an `_into` form can be added later with a concrete consumer.

### 4.4 Collection capacity and literals

**Measured problem:** push-based construction and cloning.

**Expected benefit (measured for the runtime clone, estimated for the new
APIs):** `clone` already allocates once. Exact literals and `with_capacity`
remove the remaining growth. Port 7 can drop from 9,213 reallocs toward one
allocation per coefficient vector. BDF and FIRK can drop most 2-element
allocations.

**Risk:** `with_capacity` changes allocation behavior but not semantics. The
main risk is a parser or codegen bug in `vec![value; n]` for owned types. Start
with copy types.

**Machine code:** push still has a capacity check. If the compiler can see the
capacity, it may eliminate the branch. Do not rely on that; measure.

### 4.5 Fixed arrays

**Expected benefit (estimated, unverified):** inline storage removes heap
allocation for 2- and 9-element states and gives PERF-03 constant-length
information for bounds elision. This is the clean fix for Ports 01, 02, and 06.

**Risk:** large fixed arrays copied by value can make things worse. The
language needs move/copy rules and an ABI that passes large arrays by pointer
or reference where appropriate.

**Machine code:** check that a `[Float64; 2]` field compiles to two doubles
inline, not a heap pointer. Check that constant indexing emits direct loads.

### 4.6 Complex arithmetic

**Expected benefit (estimated):** adopting `std.complex` removes 31 lines and
one duplicate type. The arithmetic is the same, so performance should be
neutral. Value-taking wrappers may add copies; check generated C.

**Risk:** changing `cdiv` to a robust algorithm changes bits. Do not do it.

### 4.7 Matrix access and complex 2x2

**Expected benefit (estimated):** `Mat.get/set` lets Port 03 use a real matrix
without flattening. It may add function-call overhead if not inlined. A
complex 2x2 wrapper removes 31 lines of local algebra but must preserve the
simple formulas.

**Risk:** a general matrix library would add allocation and layout complexity
without a consumer. Keep it narrow.

### 4.8 ODE package

**Expected benefit (estimated):** removes 150 to 250 lines of structural
scaffolding across seven ports, but much of that is not exact duplication. It
also makes counter asymmetry visible and testable.

**Risk:** a generic solver or cache would be larger than the duplication it
removes and could hide semantics. Keep to counters, FSAL rotation, convergence,
and norm adapters.

### 4.9 Harness and output

**Measured problem:** printer cost is 81 percent of Port 7 wall time and about
20 to 24 percent of the other batteries. Literal `"="` allocates per label.

**Expected benefit (estimated):** a literal output fast path and a fused
reporter can remove most of those allocations. The oracle output must stay
exactly three lines per label.

**Risk:** changing output formatting or buffering changes the oracle. Test
byte-for-byte output.

**Machine code:** a literal `println("...")` or `write("...")` should compile
to `fwrite` or `fputs`, not `VStr` allocation. A three-call sequence may become
one buffered write only if the output is identical.

## 5. Compiler work that makes libraries cheap

| Compiler change | Library enabled | Evidence | Risk |
| --- | --- | --- | --- |
| Guarded `sqrt` builtin | `std.math.sqrt`, all numeric code | AUD 4 | Edge-case correctness |
| Exact `vec![...]` and repeat literal | `std.numeric.zeros/filled` | AUD 5 | Owned-type clone policy |
| `Vec.with_capacity`, `reserve`, `resize`, `fill`, `copy_from` | Allocation-aware vector code | AUD 5 | Layout and drop correctness |
| Inline-friendly `Vec.push`/`get` | Hot loops in `std.numeric` | AUD 5, 6 | Code size |
| Fixed arrays and tuples | Ports 01, 02, 03, 06 | XRD D6 | Type system and ABI |
| `Mat` get/set | Port 03, `std.linalg` | XRD G11 | Bounds behavior |
| Constant-index bounds elision or `@inbounds` | Fixed and small vectors | AUD 6 | Safety model |
| Large aggregate by pointer/move elision | Port 07 state | AUD 9 | ABI complexity |
| Literal output fast path and peephole | Harness, diagnostics | AUD 3 | Output exactness |
| Function and data sections plus gc | Dev build size | AUD 7 | Low |
| Optional `-O3` profile | Math-heavy workloads | AUD 8 | Binary size, reproducibility |
| `-ffp-contract=off` | Default exact-order correctness | AUD 8 | Slight codegen change |

## 6. Where a library could be slower

| Abstraction | Why it could be slower | Mitigation |
| --- | --- | --- |
| Allocating `axpy` in a stage loop | One allocation per call | Provide `axpy_into`; use it in hot loops |
| `Result`-returning vector ops | Branch and error layout in inner loop | Shape check once, then tight loop; separate checked API |
| Generic broadcast with callbacks | Indirect call, no inlining | Named fixed operations first |
| `Vec.get` returning `Option` | Tag check and branch per element | Direct indexing in trusted loops, or inline fast path |
| Bounds checks on constant indices | Extra compare and branch | Fixed arrays or proven-index path |
| Large fixed arrays by value | Copy cost | Move/reference conventions, ABI review |
| Complex value wrappers | Extra copies | Borrowed forms or inline wrappers |
| General matrix abstraction | Allocation and layout indirection | Keep `Mat` concrete; add get/set only |
| Generic ODE cache | Branching and union layout | Keep algorithm-specific caches |

The plan does not assume that fewer source lines automatically produce better
machine code. Each library needs a generated-C review and a benchmark.

## 7. Benchmark plan

### 7.1 Required measurements

Every performance-sensitive change reports:

- Kernel time for the changed function.
- Warm numeric loop time for the affected port, with output removed.
- End-to-end time, clearly labeled as output-bound or startup-bound.
- Explicit `malloc`, `calloc`, and `realloc` counts.
- Binary size and text size.
- Generated C line count and `<error>` count.
- Oracle labels bitwise equal, or the exact residual.
- ASan/UBSan/LSan status.

### 7.2 Benchmark cases

Start with the existing audit harness:

- `sqrt_bench`: local helper versus C `sqrt`.
- `purenumeric` builds for ports 07 to 10.
- Allocation counters for 07, 08, and 09.
- Port 07 printer isolation.
- BDF and FIRK warm numeric loops.
- Dev and release text size.

Add new cases for:

- `vec![value; n]` versus a push loop.
- `Vec.with_capacity` plus fill versus push.
- `std.numeric.axpy` versus a local loop.
- `std.numeric.rms` versus a local loop.
- Fixed array access versus `Vec` access.
- `Mat.get/set` versus flat indexing.
- Literal output versus `VStr` construction.
- Large state by value versus by reference.

### 7.3 Acceptance thresholds

These are proposed gates, not measured results:

- No oracle regression. Bit-exact ports stay bit-exact; ports with known
  residues keep the same labels and error bounds.
- No allocation regression. A library path may not allocate more than the
  hand-written path it replaces.
- No end-to-end regression greater than 2 percent when the benchmark is not
  output-bound. If it is output-bound, report the numeric loop separately.
- No binary size regression greater than 5 percent for a default release
  change. `-O3` is exempt because it is opt-in.
- No generated-C `<error>` markers. ASan clean.
- No new warnings in the port builds beyond the documented W1002/W1003
  baseline.

Thresholds can be revised only with a written measurement and a stated reason.

### 7.4 Machine-code checks

Use `objdump -d -Mintel --no-show-raw-insn` and `nm -S --size-sort`.

Check:

- `sqrt` lowers to `sqrtsd` or a libm call, not a Newton loop, on the normal
  path.
- `axpy` has no call to an allocating helper in its `_into` form.
- `Vec.push` is inlined or has a single capacity check in a known-capacity
  loop.
- No implicit FMA contraction or reassociation appears in exact-order paths.
  Packed instructions are acceptable only when each lane preserves the
  documented order and rounding.
- No `vfmadd`/`fmadd` appears in bit-exact arithmetic.
- Fixed-array storage is inline, not a heap pointer; argument passing follows
  the documented ABI.
- Port 07's 536-byte `memcpy` disappears if the state is passed by reference.
- Literal `println` or `write` does not call `vstr_from_c` for a constant
  string.

## 8. Evidence classification

| Claim | Class | Basis |
| --- | --- | --- |
| Local `sqrt` is 11.63x slower per call | Measured | AUD 4 microbenchmark |
| BDF warm loop improves 2.25x with C `sqrt` | Measured | AUD 4 A/B |
| FIRK warm loop improves 1.26x with C `sqrt` | Measured | AUD 4 A/B |
| Port 07 makes 61.4 numeric reallocs per step | Measured | AUD 5 |
| FIRK makes 17.2 numeric reallocs per step | Measured | AUD 5 |
| Dev text size halves with section flags | Measured | AUD 7 |
| `numeric_core` dev text falls 72.9 percent with section flags | Measured | `cc -std=c11 -O0 -g -fwrapv -pthread -fno-math-errno -ffp-contract=off`, then re-link with `-ffunction-sections -fdata-sections -Wl,--gc-sections`; `size` text 53,572 to 14,504 bytes; outputs byte-identical |
| Printer is 81 percent of Port 7 wall time | Measured | AUD 3 |
| `-O3` helps numeric loops 2.6 to 25 percent | Measured | AUD 8 |
| `std.numeric` removes 350 to 450 lines | Estimated | XRD D2, report-only ports |
| Fixed arrays remove most 2-element allocations | Estimated | AUD 5 plus representation reasoning |
| A future ODE port needs 10 to 25 percent less code | Estimated | Duplication share, not measured time |
| `restrict` helps larger loops | Unverified | No measurement |
| Range analysis removes most checks | Unverified | No measurement |

## 9. Do not do

- Do not use `-ffast-math`, `-Ofast`, or approximate `sin`/`cos`/`sqrt` for
  the bit-exact oracles.
- Do not use `-march=native` as a default.
- Do not remove bounds checks globally without a proof or explicit unchecked
  block.
- Do not chase vectorization of 2-element ODE kernels.
- Do not claim a speedup from a source-line reduction.
- Do not benchmark only total wall time when the printer or process startup
  dominates.
- Do not hide allocations inside a library function used in a stage loop.
- Do not change summation order or complex division to make an algorithm
  "cleaner."

## 10. Performance gates by phase

| Phase | Required evidence |
| --- | --- |
| Phase 0 | `sqrt` microbenchmark and BDF/FIRK A/B; allocation counts for BDF/FIRK; dev text size; oracle equality |
| Phase 1 | `std.numeric` kernel microbenchmarks; allocation counts; generated-C diff; port 08/10 oracle equality |
| Phase 2 | ODE/harness benchmarks; no regression in existing numeric loops |
| Phase 3 | Fixed-array codegen review; bounds-check counts; large-state copy review; optional `-O3` data |
| Phase 4 | CI benchmark baseline; regression thresholds; documentation of host and toolchain |

The audit host and GCC version must be recorded with every baseline. A result
from one host is not a universal performance guarantee.
