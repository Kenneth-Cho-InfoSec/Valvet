# 04 Developer experience

## 1. What made the ports expensive

The port reports show a consistent pattern. The numerical algorithms were
translatable. The time went into shims, allocation management, borrow
workarounds, literal casts, and test scaffolding.

| Friction | Evidence | Frequency | Fix |
| --- | --- | --- | --- |
| Missing scalar math | GAPS port 04 items 1-7; ports 05, 08, 09, 10 | 10+ helper names, 30+ call sites in port 04 | `std.math` expansion |
| `sqrt` not trusted | XRD D1; CFB section 2 | 6 local blocks, about 770 lines | tiny-normal fix plus guarded builtin |
| Vector boilerplate | XRD D2; GAPS ports 04-09 | 18+ helpers | `std.numeric`, `Vec.with_capacity` |
| Residual boilerplate | XRD D3; GAPS ports 04-09 | 5 implementations | `std.numeric` norms/residuals |
| Complex duplicate | XRD D5; GAPS port 09 | 4 functions, 31 lines | import `std.complex` |
| Fixed vectors and tuples | XRD D6; PORT-01, PORT-02, PORT-06 | 4 patterns | fixed arrays and tuple returns |
| Matrix flattening | XRD G11; PORT-03 | 1 representation workaround | `Mat` get/set |
| Deprecated `print` for line output | GAPS ports 04, 06, 07, 08, 09, 10 | 2 W1002 per run | migrate to `println`; optional `write` |
| Integer literal typing | GAPS ports 06, 07, 08, 09; retained sources | 589 `as Int64` in retained port files, 241 generated | contextual literal inference |
| Borrowed temporaries rejected | GAPS port 09 item 11; port 07 item 17 | 23 complex broadcast sites, repeated hoists | reborrow and temporary lifetime rules |
| Self-assignment while borrowed | GAPS port 09 item 10; port 08 item 9 | 2 concrete examples | borrow checker improvement |
| Dotted import rejected | GAPS port 08 item 9 | 1 observed, likely common | accept `std.math::sqrt` as alias |
| Redundant cast warning | W1003 on `1 as Int32` | 2+ per port | contextual inference plus warning tuning |
| No package template | Repository tooling | Every port starts from scratch | porting guide and template |
| No benchmark harness | AUD 11 | Manual scripts per port | `bench` tooling |

The 589 `as Int64` count is from retained sources only: port 05 fragment 26,
port 06 fragment 78, port 07 body 114, port 07 generated 241, port 08 38, port
09 75, port 10 17. Ports 01 to 04 would add more. This is a strong DX signal
because the casts are not expressing numerical intent. They are satisfying a
literal typing rule.

## 2. Before and after

The examples below are illustrative. The "current" code mirrors patterns in
the retained ports. The "proposed" code is the target API, not implemented
yet.

### 2.1 Square root

Current, repeated in six ports:

```valvet
struct DD { hi: Float64, lo: Float64 }

fun two_sum(a: Float64, b: Float64): DD { ... }
fun two_prod(a: Float64, b: Float64): DD { ... }
fun binade(y: Float64): Float64 { ... }
fun dd_sub(...): Float64 { ... }
fun mant_even(...): Bool { ... }
fun wv_sqrt_cr(x: Float64): Float64 { ... }

// about 128 lines per port
```

Proposed:

```valvet
import std::math::sqrt;

val y = sqrt(x);
```

If `sqrt` is a compiler builtin, the import is not needed for ordinary calls.
The `std.math.sqrt` function remains for explicit edge-case semantics.

Estimated reduction: about 750 to 780 lines across six ports. The call sites
do not grow. This is the largest single source reduction.

### 2.2 Allocating a zero vector

Current:

```valvet
fun wv_zeros8(n: Int64): Vec<Float64> {
    var v: Vec<Float64> = vec![];
    var i = 0 as Int64;
    while (i < n) {
        v.push(0.0);
        i = i + (1 as Int64);
    }
    return v;
}
```

Proposed:

```valvet
import std::numeric::zeros;

val v = zeros(n);
```

or, at the collection level:

```valvet
val v = vec![0.0; n];
```

Estimated reduction: 8 to 12 lines per helper, and one allocation instead of
several reallocations. BDF currently makes about 3 numeric reallocs per step;
FIRK about 17.

### 2.3 Cloning a vector

Current:

```valvet
fun wv_clone8(v: &Vec<Float64>): Vec<Float64> {
    var out: Vec<Float64> = vec![];
    var i = 0 as Int64;
    while (i < v.len()) {
        out.push(v[i]);
        i = i + (1 as Int64);
    }
    return out;
}
```

Proposed:

```valvet
val copy = v.clone();
```

`Vec.clone` already emits an allocating runtime clone that copies directly.
This removes the push loop and per-element bounds checks. Estimated reduction:
8 to 10 lines per helper, plus fewer reallocations.

### 2.4 AXPY

Current:

```valvet
fun wv_axpy4(a: Float64, x: &Vec<Float64>, b: Float64, y: &Vec<Float64>,
             c: Float64, z: &Vec<Float64>, d: Float64, w: &Vec<Float64>): Vec<Float64> {
    var out: Vec<Float64> = vec![];
    var i = 0 as Int64;
    while (i < x.len()) {
        out.push(a * x[i] + b * y[i] + c * z[i] + d * w[i]);
        i = i + (1 as Int64);
    }
    return out;
}
```

Proposed:

```valvet
import std::numeric::axpy4;

val out = axpy4(a, x, b, y, c, z, d, w);
```

Or, in a hot loop:

```valvet
axpy4_into(dst, a, x, b, y, c, z, d, w);
```

Estimated reduction: 10 to 15 lines per helper. More importantly, the
evaluation order is documented and tested once.

### 2.5 Residual and RMS

Current:

```valvet
fun wv_res9(x: &Vec<Float64>, u0: &Vec<Float64>, u1: &Vec<Float64>,
            alpha: Float64, rho: Float64): Vec<Float64> {
    var out: Vec<Float64> = vec![];
    var i = 0 as Int64;
    while (i < x.len()) {
        val m = max_abs9(u0[i], u1[i]);
        out.push(x[i] / (alpha + m * rho));
        i = i + (1 as Int64);
    }
    return out;
}

fun wv_rms9(x: &Vec<Float64>): Float64 {
    var s = 0.0;
    var i = 0 as Int64;
    while (i < x.len()) {
        s = s + x[i] * x[i];
        i = i + (1 as Int64);
    }
    sqrt(s / (x.len() as Float64))
}
```

Proposed:

```valvet
import std::numeric::scaled_residual;
import std::numeric::rms;

val res = scaled_residual(x, u0, u1, alpha, rho);
val err = rms(&res);
```

Estimated reduction: 18 to 30 lines per port. The exact sum order and
parentheses live in one tested place.

### 2.6 Scalar absolute value

Current:

```valvet
fun wv_abs_f8(x: Float64): Float64 {
    if (x < 0.0) {
        return 0.0 - x;
    }
    x
}
```

Proposed:

```valvet
import std::math::abs;

val a = abs(x);
```

Estimated reduction: 5 to 7 lines per helper and 4 direct copies in retained
ports, plus 17 abs-like patterns in the standard library itself.

### 2.7 Integer literals

Current:

```valvet
if (iter == (1 as Int64)) { ... }
st.s.max_order = 6 as Int64;
st.c.n_old = 4 as Int64;
while (i < (n as Int64)) { ... }
```

Proposed:

```valvet
if (iter == 1) { ... }
st.s.max_order = 6;
st.c.n_old = 4;
while (i < n) { ... }
```

The compiler should infer an integer literal's width from the expected type in
comparisons, assignments, arguments, and loop bounds. If a default is needed,
use the existing `Int64` default consistently.

Estimated reduction: 589 cast occurrences in retained port sources, plus
generated code noise. This is a language change with a broad blast radius, so
it needs its own fixtures and a careful migration note.

### 2.8 Borrowed temporary

Current, from port 09:

```valvet
val u1 = wv_add9v(&st.s.uprev, &st.c.z1);
wv_f9i(md, &mut st.c.ff1, &u1, ...);
```

Proposed:

```valvet
wv_f9i(md, &mut st.c.ff1, &wv_add9v(&st.s.uprev, &st.c.z1), ...);
```

This requires temporary lifetime extension for borrowed call arguments. It is
a language design change, not just a library addition. It must be sound under
moves and drops. The benefit is one fewer binding at each inline stage
construction. Port 09 has many such sites.

### 2.9 Self-assignment while borrowed

Current:

```valvet
st.c.J = wv_calc_j9(&st);   // rejected: assignment target borrowed by call
```

Workaround:

```valvet
val J9 = wv_calc_j9(&st);
st.c.J = J9;
```

Proposed: allow a call to read the receiver, compute a value, and assign after
the borrow ends, or provide a clearer diagnostic and a standard two-step
pattern. This is a borrow-checker ergonomics issue, not a library issue.

### 2.10 Complex arithmetic

Current:

```valvet
struct C09 { re: Float64, im: Float64 }

fun wv_add9(a: C09, b: C09): C09 { ... }
fun wv_sub9(a: C09, b: C09): C09 { ... }
fun wv_mul9(a: C09, b: C09): C09 { ... }
fun wv_div9(a: C09, b: C09): C09 { ... }
```

Proposed:

```valvet
import std::complex::Complex;
import std::complex::cnew;
import std::complex::cadd;
import std::complex::csub;
import std::complex::cmul;
import std::complex::cdiv;
```

Estimated reduction: 31 lines in port 09 and one duplicate type. If borrow
syntax is noisy, add value-taking wrappers to `std.complex`, not a second
type.

### 2.11 Fixed vector

Current:

```valvet
struct V2 { x: Float64, y: Float64 }

fun v2_add(a: V2, b: V2): V2 {
    V2 { x: a.x + b.x, y: a.y + b.y }
}
```

Proposed:

```valvet
val a: [Float64; 2] = [1.0, 2.0];
val b: [Float64; 2] = [3.0, 4.0];
val c = [a[0] + b[0], a[1] + b[1]];
```

or an interim `std.fixed.Vec2` if compiler arrays are delayed. The target is
inline storage and constant-index access without heap allocation. Estimated
reduction for ports 01 and 02 is unverified because their sources are missing.

### 2.12 Matrix access

Current, from port 03's flattened representation:

```valvet
val idx = i * cols + j;
m[idx] = value;
```

Proposed:

```valvet
m.set(i, j, value)?;
val maybe_x: Option<Float64> = m.get(i, j);
```

`get` returns `Option`; the caller decides whether an out-of-range read is an
error. `set` returns `Result<Unit, ShapeError>`. The gain is safety and
readability, not necessarily lines. It also lets code use the existing `Mat`
kernels instead
of a parallel flat representation.

### 2.13 Deprecated `print` and no-newline output

Current:

```valvet
print(label);       // W1002
print("=");         // W1002
println(value);
```

Proposed:

```valvet
println(label);
println("=");
println(value);
```

or:

```valvet
import std::harness::emit_label;
emit_label(label, value);
```

Estimated reduction: 2 deprecation warnings per run and a cleaner protocol.
The output must remain exactly three lines per label. Use `write` only for a
deliberate no-newline fragment; it is not a replacement for these three calls.

### 2.14 Import spelling

Current accepted spelling:

```valvet
import std::math::sqrt;
```

Observed first guess:

```valvet
import std.math::sqrt;   // rejected E0602
```

Proposed: accept both, normalize internally, and keep the diagnostic that
teaches the canonical form. This is a small change with a large discoverability
benefit.

## 3. Error messages and failure handling

The reports record borrow errors that are technically correct but hard to act
on:

- E0302 self-assignment while borrowed.
- E0303 cannot borrow a temporary.
- E0301 moved value after a conditional return.
- E0201 argument width mismatch.
- E0602 dotted import path.

Improvements:

1. Every borrow error should print the borrow's start, the conflicting use,
   and one concrete rewrite. For E0303, suggest a local binding. For E0302,
   suggest reading into a local first.
2. Every literal width error should say which side is `Int64` and which is
   `Int32`, and suggest removing the cast if contextual inference lands.
3. Import errors should show the accepted spelling.
4. Warnings should have a suppression or migration path. A warning that every
   oracle harness must trigger is a warning that loses meaning.
5. Numeric domain errors should name the operation and the argument, not only
   "NaN."

The current diagnostics already carry file, line, column, code, message, and
help. The work is content and consistency, not a new diagnostic system.

## 4. Testing and benchmarking DX

The ports currently use manual `compare*.py` scripts, manual ASan commands,
and manual allocation counters. A standard tool should provide:

```sh
valvet test valvet-tests
valvet test --filter math
valvet bench <file> --json
valvet build <file> --emit-c
valvet build <file> --emit-asm
```

The benchmark runner should report:

- Wall time, with output and startup separated where possible.
- Warm numeric loop time.
- Allocation counts.
- Binary and text size.
- Oracle labels compared, exact and within tolerance.
- Generated C `<error>` count.
- ASan/UBSan/LSan status.

This does not need to be a language feature. A repository script can do most of
it. The important part is a stable JSON schema and a checked baseline.

## 5. Documentation and discoverability

The current standard library is discoverable by reading source files. That is
not enough for a growing ecosystem.

Add:

- A generated reference of every `pub fun`, `pub struct`, and `pub enum` with
  signatures, semantics, allocation behavior, and exact-order notes.
- A short "numeric cookbook" with vector operations, norms, residuals, and
  matrix access.
- A porting guide that maps Julia constructs to Valvet idioms and states which
  gaps are deliberate.
- A template for a new port with an oracle runner and a label printer.
- Examples that use `std.numeric`, not private helper blocks.

The reference can be generated from source with a small script. It does not
need a documentation compiler in Phase 0.

## 6. Package and project workflow

The manifest already supports path and git dependencies, version bounds, a
lockfile, and vendoring. The DX gaps are discoverability and convention:

- `valvet new` should produce a project with a manifest, a source file, a test
  file, and a README.
- The template should show a path dependency on a local package.
- Documentation should state which offline and hermetic workflows are
  supported today and which are not.
- Package naming and module naming rules should be documented.
- A package should be able to declare its Valvet version requirement.

Do not build a public registry until there is a real distribution problem.

## 7. Migration guide for port authors

A future port should follow this sequence:

1. Read the Julia source and identify the exact slice.
2. Record the Julia commit or MD5.
3. Write an independent oracle. Do not share helpers with the Valvet port.
4. Write a small Valvet battery before porting the full kernel.
5. Import `std.math`, `std.numeric`, and `std.complex` before writing local
   helpers.
6. Use `Vec.with_capacity` or exact literals for known sizes.
7. Keep algorithm-specific caches and branches local.
8. Use `println` for the three label lines; use `write` only for a deliberate
   no-newline fragment.
9. Run `valvet fmt`, `valvet check`, and `valvet build`.
10. Compare every label, then run ASan/UBSan/LSan on the generated C.
11. Record allocation counts and generated-code `<error>` count.
12. Delete local helpers only after the oracle passes.

This sequence is the same discipline the existing ports used. The difference
is that steps 5 to 8 should not require custom code.

## 8. DX measures

Track these over time:

- Number of local helper lines per port.
- Number of `as Int64` and `as Int32` casts in new code.
- Number of W1002 and W1003 warnings in the fixture suite.
- Number of distinct `sqrt`, `abs`, `max_abs`, `zeros`, `clone`, and residual
  implementations in the repository.
- Time from a fresh checkout to a passing first port battery. This is not
  currently measured; introduce it as a manual stopwatch metric, not a
  fabricated benchmark.
- Number of standard-library private helpers that have a public equivalent.
- Number of API reference entries with allocation and exactness notes.

## 9. DX non-goals

- Do not add closures just to make `map` look like Julia.
- Do not add operator overloading for vector arithmetic before the language
  design supports it.
- Do not add a package registry to solve a library API problem.
- Do not hide warnings instead of fixing the underlying API.
- Do not introduce a large documentation framework before the content exists.
- Do not make every diagnostic friendlier by making it vague. Precision first.
