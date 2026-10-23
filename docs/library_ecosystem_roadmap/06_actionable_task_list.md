# 06 Actionable task list

## How to use this list

- A task is independently actionable only when all dependencies are complete.
- "Affected ports" names the ports that have direct evidence or would benefit.
- "Evidence" uses the shorthand from `README.md`.
- Every implementation batch must pass the gates in
  `05_implementation_roadmap.md`.
- No task authorizes committing ported code.
- Effort is S, M, or L, not a calendar estimate.
- "Parallel" names tasks that can proceed without waiting for this one.
- The `Exact File Changes Required` table at the end is the per-task file
  inventory. The phase tables in `05_implementation_roadmap.md` carry the
  compact path list and point to this table for the full edit.

### Large-task splits

Some L tasks must be executed as separate gated sub-batches even though they
share one task ID:

- `ARR-01`: design and ABI, parser and sema, codegen and layout, tests.
- `ARR-02`: layout and construction, destructuring, owned-element drop tests.
- `DX-03`: lifetime and drop rules, borrow-checker implementation, positive
  and negative fixtures.
- `PERF-03`: proof design, codegen implementation, safety and ASan tests.
- `DX-01`: expected-type design, sema implementation, overload-resolution
  fixtures.

Do not land an entire L task as one omnibus commit if an intermediate stage
does not build or cannot be validated. Each sub-batch still needs the full
gate set.

## Priority summary

| Priority | Tasks | Purpose |
| --- | --- | --- |
| P0 | FND-01, FND-02, FND-03, COL-01, COL-02, NUM-01, NUM-02, IO-01, PERF-01, DX-01, DX-02, VER-01 | Trusted foundations and first migrations |
| P1 | FND-04, CPLX-01, NUM-03, ARR-01, ARR-02, MAT-01, LINALG-01, PERF-02, PERF-03, PERF-06, DX-03, DX-04, TEST-01 | Core library, representation, and developer experience |
| P2 | ODE-01, HARNESS-01, STOCH-01, NUM-04, PERF-04, PERF-05, DOC-01, DOC-02, PKG-01, LANG-01, TOOL-01 | Packages, tooling, and research |

---

# P0 tasks

## FND-01: Expand `std.math` scalar foundation

| Field | Value |
| --- | --- |
| Task ID | FND-01 |
| Title | Expand `std.math` scalar foundation |
| Category | Library Expansion |
| Priority | P0 |
| Dependencies | None |
| Affected ports | 01, 02, 04, 05, 08, 09, 10 |
| Evidence | XRD D4/G1; GAPS port 04 items 1-7; ports 05, 08, 09, 10 local `abs`/`max_abs`; `stdlib/math.valvet` |
| Proposed API | `abs`, `min`, `max`, `floor`, `ceil`, `trunc`, `round`, `eps`, `sinh`, `cosh`, `tanh`, `acosh`, `asinh`, `atanh`, `clamp`, `pow_int` |
| Implementation steps | 1. Add a math semantics test fixture with NaN, signed zero, zero, infinity, and subnormal cases. <br> 2. Implement `abs`, `min`, `max` with a documented NaN and signed-zero contract, checked against Julia and the port shims. <br> 3. Implement rounding with ties-to-even for `round`, plus `eps`, hyperbolic, and `clamp` using existing `exp`/`sqrt` where no libm lowering exists. <br> 4. Add `pow_int` by exponentiation by squaring, preserving operation order. <br> 5. Export and document allocation-free behavior. <br> 6. Do not migrate ports in the same batch as the library addition. |
| Expected benefit | Removes 10+ local shim names, 4 direct `abs` copies, 3 `max_abs` copies, and 30+ port 04 call sites. Estimated 50 to 80 port lines plus standard-library duplication. |
| Effort | M |
| Acceptance criteria | All listed functions are public; NaN, signed-zero, and exact-half rounding tests pass; `valvet test valvet-tests` passes; no existing output changes. |
| Validation | New fixture, `cargo test --workspace`, debug/release fixture suite, ASan on numeric fixtures, manual comparison to the port shim behavior. |
| Parallel | FND-02, COL-01, COL-02, IO-01, PERF-01, DX-01, DX-02 |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/math_scalar.valvet` and `.stdout`; modify `stdlib/math.valvet`. Full edit, sources, completion, and risk: the `FND-01` row in the `Exact File Changes Required` table below. |

## FND-02: Fix `sqrt` for tiny normals

| Field | Value |
| --- | --- |
| Task ID | FND-02 |
| Title | Fix `std.math.sqrt` for tiny normals |
| Category | Library Expansion |
| Priority | P0 |
| Dependencies | None |
| Affected ports | 01, 02, 04, 05, 06, 08, 09, 10 |
| Evidence | XRD D1/G2; AUD 4; `stdlib/math.valvet:56-131`; CFB section 2; 337 mismatches in 1,000,000 tiny normal inputs |
| Proposed API | No API change. Preserve `sqrt(x)` negative, zero, NaN, and infinity behavior. |
| Implementation steps | 1. Build a bit-level sweep around `2.3e-308` to `1.8e-305` against hardware `sqrt`. <br> 2. Identify whether the error is in the binade seed, two-product correction, or tie-break. <br> 3. Fix the correction without changing normal-range results. <br> 4. Add a regression fixture with the failing bit patterns. <br> 5. Re-run ports 08, 09, and 10 bit-exact comparisons. |
| Expected benefit | Makes the standard library trustworthy for all inputs and enables deletion of six local copies. Measured potential: 11.63x per-call `sqrt` improvement and 2.25x/1.26x BDF/FIRK warm loops when replaced with hardware. |
| Effort | S |
| Acceptance criteria | 0 mismatches in a 1,000,000-value tiny-normal sweep; edge cases unchanged; ports 08, 09, 10 remain bit-exact; no `<error>` markers. |
| Validation | Sweep harness, `math_sqrt_exact` fixture extension, port 08/09/10 oracles, ASan/UBSan/LSan, generated-C review. |
| Parallel | FND-01, COL-01, COL-02, IO-01, PERF-01, DX-01, DX-02 |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `scripts/sqrt_sweep.py`, `valvet-tests/compile-pass/math_sqrt_tiny.valvet` and `.stdout`; modify `stdlib/math.valvet` only if the sweep fails. Full edit, sources, completion, and risk: the `FND-02` row in the `Exact File Changes Required` table below. |

## FND-03: Guarded `sqrt` builtin and FP flags

| Field | Value |
| --- | --- |
| Task ID | FND-03 |
| Title | Add a guarded `sqrt` builtin and safe FP flags |
| Category | Compiler |
| Priority | P0 |
| Dependencies | FND-02 |
| Affected ports | 01, 02, 04, 05, 06, 08, 09, 10 |
| Evidence | AUD 4; `crates/valvet_core/src/codegen.rs:4838` (`sin`/`cos` precedent); CFB section 2 |
| Proposed API | `sqrt(x)` recognized as a builtin on the safe positive path outside tiny normals; `std.math.sqrt` remains the explicit edge-case function. |
| Implementation steps | 1. Add a codegen arm next to `sin`/`cos` that emits guarded C `sqrt` or `__builtin_sqrt`. <br> 2. Route negative, zero, NaN, infinity, and tiny normals to the library wrapper. <br> 3. Add `-fno-math-errno` only with the builtin. <br> 4. Add `-ffp-contract=off` to the default compile path; Q22 may refine the flag spelling, not the no-implicit-FMA rule. <br> 5. Add a fixture that inspects generated C for the safe positive path. |
| Expected benefit | Removes the software call and loop from hot paths. Measured 11.63x per call and 2.25x/1.26x warm BDF/FIRK loops when hardware `sqrt` was substituted. |
| Effort | M |
| Acceptance criteria | Safe positive path emits `sqrt`/`__builtin_sqrt`; edge cases match `std.math.sqrt`; ports 08, 09, and 10 are bit-exact; default build records `-ffp-contract=off`; no implicit FMA in exact-order fixtures; generated C has zero `<error>` markers. |
| Validation | Generated-C diff, `objdump` check for `sqrtsd`/libm call, BDF/FIRK warm loops, all port oracles, ASan/UBSan/LSan. |
| Parallel | COL-01, COL-02, NUM-01, PERF-01, DX-01, DX-02 |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `crates/valvet_core/tests/codegen_fp.rs`, `valvet-tests/compile-pass/sqrt_builtin.valvet` and `.stdout`; modify `crates/valvet_core/src/sema.rs`, `crates/valvet_core/src/codegen.rs`. Full edit, sources, completion, and risk: the `FND-03` row in the `Exact File Changes Required` table below. |

## COL-01: Exact `vec![...]` and repeat literal

| Field | Value |
| --- | --- |
| Task ID | COL-01 |
| Title | Allocate exact capacity for `vec![...]` and add `vec![value; n]` |
| Category | Compiler |
| Priority | P0 |
| Dependencies | None |
| Affected ports | 01-10 |
| Evidence | AUD 5; `crates/valvet_core/src/codegen.rs:5662-5680` |
| Proposed API | `vec![a, b, c]` allocates exactly 3 slots once; `vec![value; n]` allocates exactly `n` slots once. |
| Implementation steps | 1. Lower a fixed-element literal to one allocation followed by direct stores, not repeated push. <br> 2. Parse and lower the repeat form for copy element types. <br> 3. Define the owned-element policy or reject it with a clear diagnostic in v1. <br> 4. Add allocation-count tests and ASan drop tests. |
| Expected benefit | Removes reallocations from every fixed literal. Combined with COL-02, attacks the measured 61.4, 17.2, and 3.0 per-step allocation counts. |
| Effort | M |
| Acceptance criteria | A 3-element literal makes one allocation; `vec![0.0; n]` makes one allocation; drop and borrow tests pass; output unchanged. |
| Validation | Allocation counter, generated-C inspection, ASan/UBSan/LSan, fixtures for copy and owned elements. |
| Parallel | FND-01, FND-02, COL-02, IO-01, PERF-01, DX-01, DX-02 |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/vec_repeat.valvet` and `.stdout`, `valvet-tests/compile-pass/vec_literal_owned.valvet` and `.stdout`; modify `crates/valvet_core/src/ast.rs`, `crates/valvet_core/src/parser.rs`, `crates/valvet_core/src/sema.rs`, `crates/valvet_core/src/codegen.rs`. Full edit, sources, completion, and risk: the `COL-01` row in the `Exact File Changes Required` table below. |

## COL-02: `Vec` capacity and buffer operations

| Field | Value |
| --- | --- |
| Task ID | COL-02 |
| Title | Add `Vec.with_capacity`, `reserve`, `resize`, `fill`, `copy_from`, `capacity` |
| Category | Compiler |
| Priority | P0 |
| Dependencies | None |
| Affected ports | 04, 05, 06, 07, 08, 09 |
| Evidence | AUD 5; `crates/valvet_core/src/codegen.rs:2488-2530`; runtime `v_clone_*` at `codegen.rs:2834-2856` |
| Proposed API | `Vec<T>::with_capacity(n)`, `v.reserve(n)`, `v.resize(n, value)`, `v.fill(value)`, `dst.copy_from(&src)`, `v.capacity()` |
| Implementation steps | 1. Add runtime kernels with overflow checks. <br> 2. Preserve drop and clone semantics for owned elements. <br> 3. Decide whether `resize` requires `value` for all element types. <br> 4. Add fixtures for zero length, growth, shrink-free fill, and owned elements. <br> 5. Keep `Vec.add/mul` behavior unchanged. |
| Expected benefit | Enables allocation-aware `std.numeric` and removes avoidable growth. Measured current realloc counts are 61.4/17.2/3.0 per step for ports 07/09/08. |
| Effort | M |
| Acceptance criteria | Capacity reports correctly; no realloc when capacity is sufficient; `copy_from` requires a large-enough destination and does not resize it; owned-element drops are correct; ASan clean; no output change. |
| Validation | Allocation counters, drop fixtures, ASan/UBSan/LSan, generated-C inspection. |
| Parallel | FND-01, FND-02, COL-01, IO-01, PERF-01, DX-01, DX-02 |
| Status | Complete with a documented fixture deviation. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/vec_capacity.valvet` and `.stdout`, `valvet-tests/compile-pass/vec_owned_ops.valvet` and `.stdout`, `valvet-tests/compile-fail/vec_copy_from_short.valvet` and `.error`; modify `crates/valvet_core/src/ast.rs`, `crates/valvet_core/src/parser.rs`, `crates/valvet_core/src/sema.rs`, `crates/valvet_core/src/codegen.rs`. Full edit, sources, completion, and risk: the `COL-02` row in the `Exact File Changes Required` table below. |

## NUM-01: `std.numeric` exact-order vector core

| Field | Value |
| --- | --- |
| Task ID | NUM-01 |
| Title | Add `std.numeric` P0 vector primitives |
| Category | New Library |
| Priority | P0 |
| Dependencies | FND-01, COL-01, COL-02 |
| Affected ports | 04, 05, 06, 07, 08, 09 |
| Evidence | XRD D2/G3; PORT-05 `wv_logic.valvet:199-295`; PORT-06 246-line helper block; ports 07/08/09 `zeros`/`clone` copies |
| Proposed API | `zeros`, `filled`, `clone`, `copy_from`, `fill`, `scale`, `scale_into`, `axpy`, `axpy_into`, `axpy3`, `axpy3_into`, `axpy4`, `axpy4_into`, `dot`, `norm2` |
| Implementation steps | 1. Create `stdlib/numeric.valvet`. <br> 2. Use `Vec.clone` and exact-capacity constructors. <br> 3. Implement `_into` and allocating forms. <br> 4. Document exact order and forbid FMA. <br> 5. Add unit fixtures for length 0, 1, 2, and larger vectors. <br> 6. Migrate only the helper family, one port at a time. |
| Expected benefit | Replaces 18+ helpers and 350 to 450 estimated lines. Removes push-based allocation in the covered paths. |
| Effort | M |
| Acceptance criteria | Port 08 passes 280/280 bit-exactly after vector migration; port 10 remains 328/328 through the shared math path; length and `_into` preconditions are tested; no allocation regression; no output change; public docs state exact order. |
| Validation | Port 08 280/280 and port 10 328/328; allocation counters; generated-C diff; ASan; `cargo test`. |
| Parallel | NUM-02 after the API shape settles; CPLX-01, IO-01, PERF-01 |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `stdlib/numeric.valvet`, `valvet-tests/compile-pass/numeric_core.valvet` and `.stdout`. Full edit, sources, completion, and risk: the `NUM-01` row in the `Exact File Changes Required` table below. |

## NUM-02: Norms and scaled residuals

| Field | Value |
| --- | --- |
| Task ID | NUM-02 |
| Title | Add exact-order RMS and scaled residual helpers |
| Category | Library Expansion |
| Priority | P0 |
| Dependencies | NUM-01, FND-03 |
| Affected ports | 04, 05, 06, 08, 09 |
| Evidence | XRD D3/G4; PORT-05 `wv_rms`/`wv_residuals`; PORT-06 `wv_rms2`/`wv_residuals2`; PORT-08 `wv_ndz8`; PORT-09 `wv_res9` |
| Proposed API | `rms`, `rms_parts`, `scaled_residual`, `scaled_residual2` |
| Implementation steps | 1. Implement the SciML residual formula with exact parentheses. <br> 2. Preserve increasing-index sum order and divide before `sqrt`. <br> 3. Implement the two-part sum order for `rms_parts`. <br> 4. Add tests for zero length and part-length mismatch policy. <br> 5. Migrate port 08 then port 09. |
| Expected benefit | Replaces 5 implementations and 90 to 130 estimated lines with one tested contract. |
| Effort | S |
| Acceptance criteria | Port 08 280/280 and port 09 332/332; `rms_parts` matches port 06's documented order; no allocation regression. |
| Validation | Port 08/09 oracles, focused two-part fixture, ASan, generated-C review. |
| Parallel | FND-04, CPLX-01, MAT-01, IO-01 |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Modify `stdlib/numeric.valvet`; create `valvet-tests/compile-pass/numeric_residuals.valvet` and `.stdout`. Full edit, sources, completion, and risk: the `NUM-02` row in the `Exact File Changes Required` table below. |

## IO-01: Warning-free output and no-newline `write`

| Field | Value |
| --- | --- |
| Task ID | IO-01 |
| Title | Retire `print` warnings and add a no-newline `write` primitive |
| Category | Compiler |
| Priority | P0 |
| Dependencies | None |
| Affected ports | 04, 05, 06, 07, 08, 09, 10 |
| Evidence | AUD 3; GAPS port 07 item 19; W1002 on `print`; `print` is a deprecated alias of `println`; 2 warnings per port run |
| Proposed API | `write(x)` with no newline; `println(x)` remains the line output; `print` stays a deprecated alias. |
| Implementation steps | 1. Add `write` with the same formatting dispatch as `println`, without the trailing newline. <br> 2. Keep `println` output exactly byte-identical. <br> 3. Migrate the oracle harness from `print` to `println`; do not use `write` for the three-line protocol. <br> 4. Migrate one port's reporter and compare output lines. |
| Expected benefit | Removes 2 W1002 warnings per run and adds a deliberate no-newline primitive. Literal-allocation removal is PERF-06, not this task. |
| Effort | M |
| Acceptance criteria | No W1002 in migrated ports; output remains exactly 3 lines per label; port 07 remains 31,122 lines; `write("x")` does not append a newline; `println("x")` does; output is byte-identical. |
| Validation | Byte-for-byte output comparison, label count, W1002 count, ASan; record the allocation baseline for PERF-06. |
| Parallel | FND-01, FND-02, COL-01, COL-02, PERF-01, DX-01, DX-02 |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/write_no_newline.valvet` and `.stdout`; modify `crates/valvet_core/src/sema.rs`, `crates/valvet_core/src/codegen.rs`. Full edit, sources, completion, and risk: the `IO-01` row in the `Exact File Changes Required` table below. |

## PERF-01: Development function and data sections

| Field | Value |
| --- | --- |
| Task ID | PERF-01 |
| Title | Add function/data sections and gc to development builds |
| Category | Compiler |
| Priority | P0 |
| Dependencies | None |
| Affected ports | 01-10 |
| Evidence | AUD 7; `crates/valvet_core/src/codegen.rs:12,736`; `codegen.rs:6828` build flags; dev text reduction 53.0% and 55.2% |
| Proposed API | No API change. Add `-ffunction-sections -fdata-sections -Wl,--gc-sections` to the dev profile. |
| Implementation steps | 1. Add flags only to the dev build path. <br> 2. Measure text and file size on BDF and weak. <br> 3. Confirm output is byte-identical. <br> 4. Document that release already drops unused statics. |
| Expected benefit | Roughly halves dev text size on measured ports. Low risk. |
| Effort | S |
| Acceptance criteria | Dev text size reduction at least 40% on BDF and weak; release unchanged; output byte-identical; no link failures. |
| Validation | `size`, `stat`, output comparison, full fixture suite. |
| Parallel | All P0 tasks |
| Status | Complete for the repository fixture; BDF and weak binaries are external. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `crates/valvet_core/tests/codegen_fp.rs`; modify `crates/valvet_core/src/codegen.rs` (`cc_build_profile`, new `cc_flags`). Full edit, sources, completion, and risk: the `PERF-01` row in the `Exact File Changes Required` table below. |

## DX-01: Integer literal contextual inference

| Field | Value |
| --- | --- |
| Task ID | DX-01 |
| Title | Infer integer literal width from expected type |
| Category | DX |
| Priority | P0 |
| Dependencies | None |
| Affected ports | 01-10 |
| Evidence | GAPS port 06 item 9, port 07 item 18, port 08 item 10; 589 `as Int64` in retained port sources, 241 generated |
| Proposed API | No new syntax. `iter == 1`, `st.s.max_order = 6`, and argument positions infer `Int32`/`Int64` from context. |
| Implementation steps | 1. Write a sema design note for expected-type propagation in comparisons, assignments, arguments, and loop bounds. <br> 2. Implement inference behind a focused change. <br> 3. Keep explicit casts legal and avoid new ambiguity. <br> 4. Add fixtures for overload resolution and mixed-width errors. <br> 5. Update W1003 behavior if a cast becomes redundant. |
| Expected benefit | Removes hundreds of noise casts in new code and generated ports; improves readability. |
| Effort | L |
| Acceptance criteria | New fixtures need no `as Int64` for same-width comparisons and assignments; existing tests pass; no silent narrowing; error messages name both widths. |
| Validation | Unit tests, fixture suite, cross-module overload tests, generated-C diff. |
| Parallel | FND-01, FND-02, COL-01, COL-02, IO-01, PERF-01, DX-02 |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/literal_context.valvet` and `.stdout`, `valvet-tests/compile-fail/literal_narrowing.valvet` and `.error`; modify `crates/valvet_core/src/sema.rs`, `crates/valvet_core/tests/front_end.rs`. Full edit, sources, completion, and risk: the `DX-01` row in the `Exact File Changes Required` table below. |

## DX-02: Accept dotted import spelling

| Field | Value |
| --- | --- |
| Task ID | DX-02 |
| Title | Accept `std.math::sqrt` as an alias for `std::math::sqrt` |
| Category | DX |
| Priority | P0 |
| Dependencies | None |
| Affected ports | All ports that import stdlib modules |
| Evidence | GAPS port 08 item 9; E0602 on `import std.math::sqrt`; accepted spelling `import std::math::sqrt` |
| Proposed API | Both spellings resolve to the same module path. |
| Implementation steps | 1. Normalize the import path in the parser or resolver. <br> 2. Keep the canonical spelling in diagnostics. <br> 3. Add fixtures for both forms and a mixed-form diagnostic. |
| Expected benefit | Removes a common first-use error with no semantic change. |
| Effort | S |
| Acceptance criteria | Both forms compile; duplicate imports do not double-load; diagnostics still show the canonical form. |
| Validation | Import fixtures, duplicate-module tests, full fixture suite. |
| Parallel | All P0 tasks |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Modify `crates/valvet_core/src/parser.rs`, `valvet-tests/multi-file/dotted/main.valvet`; add `valvet-tests/multi-file/dotted/math.valvet` and `valvet-tests/multi-file/dotted/main.valvet.stdout`; delete `valvet-tests/multi-file/dotted/main.valvet.error`; add `valvet-tests/multi-file/dotted-std/main.valvet` and `valvet-tests/multi-file/dotted-std/main.valvet.stdout`. Full edit, sources, completion, and risk: the `DX-02` row in the `Exact File Changes Required` table below. |

## VER-01: Port oracle harness

| Field | Value |
| --- | --- |
| Task ID | VER-01 |
| Title | Standardize the port oracle runner |
| Category | Tooling |
| Priority | P0 |
| Dependencies | None |
| Affected ports | 07, 08, 09, 10; later 01-06 |
| Evidence | XRD 5.4; CFB; existing `compare7.py` to `compare10.py`; manual ASan commands |
| Proposed API | A repository script or tool that runs a port, compares labels, reports bit-exact/one-ulp/missing, allocation counts, and ASan status. |
| Implementation steps | 1. Define a small label-comparison format. <br> 2. Wrap the existing `compare*.py` scripts without changing oracle data. <br> 3. Add a JSON summary. <br> 4. Add a command to run all available ports. <br> 5. Document how to add a port. |
| Expected benefit | Makes every migration repeatable and reduces manual verification errors. |
| Effort | M |
| Acceptance criteria | One command reports all four complete ports; results match the existing reports; no port output changes. |
| Validation | Run on ports 07-10 and compare the JSON to the known results. |
| Parallel | FND-01, FND-02, COL-01, COL-02, IO-01, PERF-01, DX-01, DX-02 |
| Status | Checker complete; port verification blocked on external oracles. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `scripts/compare_oracles.py`, `scripts/oracle_schema.json`, `scripts/README.md`, `scripts/tests/test_compare_oracles.py`; no repository oracle data. Full edit, sources, completion, and risk: the `VER-01` row in the `Exact File Changes Required` table below. |

---

# P1 tasks

## FND-04: Math special functions

| Field | Value |
| --- | --- |
| Task ID | FND-04 |
| Title | Add `sincos`, `hypot`, `ldexp`, and `evalpoly` |
| Category | Library Expansion |
| Priority | P1 |
| Dependencies | FND-01, ARR-02 |
| Affected ports | 01, 02; future 04-10 |
| Evidence | XRD G1/G10; PORT-01/PORT-02 `sincos`, `ln_mean`, `@evalpoly`; Trixi polynomial branches |
| Proposed API | `sincos(x): (Float64, Float64)`, `hypot(x, y)`, `ldexp(x, n)`, `evalpoly(x, coeffs)` |
| Implementation steps | 1. Add `sincos` using both `sin` and `cos` at one argument, avoiding a second argument evaluation. <br> 2. Add `hypot` with overflow-safe scaling. <br> 3. Add `ldexp` as an exact power-of-two scale. <br> 4. Add `evalpoly` with Horner order and documented coefficients order. <br> 5. Add tests at extreme exponents. |
| Expected benefit | Covers Trixi polynomial and trigonometric helpers and enables NUM-04. Hyperbolic paths belong to FND-01. |
| Effort | M |
| Acceptance criteria | Functions pass edge-case tests; `evalpoly` matches the documented Horner order; no existing output changes. |
| Validation | Focused fixtures, port 01/02 reconstructed helper checks, ASan. |
| Parallel | CPLX-01, NUM-03, ARR-01, MAT-01, TEST-01 |
| Status | Not started; depends on ARR-02. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Modify `stdlib/math.valvet`; create `valvet-tests/compile-pass/math_special.valvet` and `.stdout`. Full edit, sources, completion, and risk: the `FND-04` row in the `Exact File Changes Required` table below. |

## CPLX-01: Adopt `std.complex` in port 09

| Field | Value |
| --- | --- |
| Task ID | CPLX-01 |
| Title | Replace port-local complex arithmetic with `std.complex` |
| Category | Library Expansion |
| Priority | P1 |
| Dependencies | FND-01, FND-03 |
| Affected ports | 09; future FIRK |
| Evidence | XRD D5/G5; `stdlib/complex.valvet:12-36`; PORT-09 `C09`/`wv_add9` to `wv_div9`; 31 lines |
| Proposed API | Import `Complex`, `cnew`, `cadd`, `csub`, `cmul`, `cdiv`; add value wrappers if needed; use `std.math.pow_int` for scalar recursion. |
| Implementation steps | 1. Add value-taking wrappers only if borrow syntax is noisy. <br> 2. Do not change `cdiv`'s simple formula. <br> 3. Use `std.math.pow_int` from FND-01; do not add a second implementation. <br> 4. Migrate port 09's four functions. <br> 5. Compare all 332 labels. |
| Expected benefit | Removes 31 lines and one duplicate type; avoids a second complex library. |
| Effort | S |
| Acceptance criteria | Port 09 332/332 bit-exact; no second complex type; `cdiv` bits unchanged. |
| Validation | Port 09 oracle, generated-C diff, ASan. |
| Parallel | NUM-03, ARR-01, MAT-01, TEST-01 |
| Status | Complete in-repository fixture; port migration external. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | No `stdlib/complex.valvet` change confirmed; create `valvet-tests/compile-pass/complex_pow.valvet` and `.stdout`. Port 09 source is external and not in this checkout. Full edit, sources, completion, and risk: the `CPLX-01` row in the `Exact File Changes Required` table below. |

## NUM-03: Consolidate private stdlib helpers

| Field | Value |
| --- | --- |
| Task ID | NUM-03 |
| Title | Move generic private stdlib helpers into `std.numeric` and `std.math` |
| Category | Library Expansion |
| Priority | P1 |
| Dependencies | NUM-01, NUM-02 |
| Affected ports | Indirect: 01, 02, 04-10 through stdlib behavior |
| Evidence | XRD G13; `stdlib/model.valvet:27-35,526-559`; `stdlib/plot.valvet:342-409`; `stdlib/infer.valvet:192`; 17 abs-like patterns |
| Proposed API | Public `add`, `sub`, `sum`, `mean`, `min_vec`, `max_vec`, `sum_sq`, and `sd`; use `std.math.abs` and `std.math.min` for scalar helpers |
| Implementation steps | 1. Pin behavior for NaN, signed zero, empty input, and `n < 2` in the existing stdlib fixtures. <br> 2. Add only helpers with behavior-identical semantics. <br> 3. Map `model` helpers to `add`, `sub`, `mean`, and `sum_sq`; map `plot` `fmin`/`fmax`/`fmean`/`fsd` to the new functions. <br> 4. Use `std.math.min` for `fmin2` only if the NaN fixture is identical; otherwise retain `fmin2` and document why. <br> 5. Update imports and remove private copies. <br> 6. Add regression fixtures for existing stdlib output. Do not migrate missing-value statistics. |
| Expected benefit | Gives `std.numeric` immediate non-port users, removes internal duplication, and applies the P0 exact-order contract to standard-library paths. |
| Effort | M |
| Acceptance criteria | Standard-library fixtures and numeric outputs unchanged; private copies removed; no public API break; `fmin2` either migrates with identical NaN/signed-zero behavior or stays local with a recorded reason. |
| Validation | `valvet test valvet-tests`, model/plot/infer fixtures, output diff, ASan. |
| Parallel | CPLX-01, ARR-01, MAT-01, TEST-01 |
| Status | Partial. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Modify `stdlib/model.valvet`, `stdlib/plot.valvet`, `stdlib/infer.valvet`, `stdlib/numeric.valvet`; add `valvet-tests/compile-pass/numeric_reductions.valvet` and `.stdout`. Full edit, sources, completion, and risk: the `NUM-03` row in the `Exact File Changes Required` table below. |

## ARR-01: Fixed arrays `[T; N]`

| Field | Value |
| --- | --- |
| Task ID | ARR-01 |
| Title | Add compiler fixed-size arrays |
| Category | Compiler |
| Priority | P1 |
| Dependencies | None; design is part of this task |
| Affected ports | 01, 02, 03, 06; also 07, 08, 09 for small state |
| Evidence | XRD D6/G7; PORT-01 `V2`/`V4`; PORT-02 `V9`/`V3`/`V2`; PORT-06 paired `Vec` fields; no `[T; N]` in the language |
| Proposed API | `[Float64; 2]`, `[Float64; 9]`, element read/write, inline storage, constant-length access. |
| Implementation steps | 1. Write the type, layout, copy, move, and drop design. <br> 2. Add parsing and sema for fixed array types and literals. <br> 3. Emit inline C arrays or structs. <br> 4. Record constant-length metadata and add fixtures; general bounds elision is PERF-03. <br> 5. Add fixtures for struct fields, returns, and nested ownership. <br> 6. Do not add generic small-vector types first. |
| Expected benefit | Removes the `V2`/`V3`/`V4`/`V9` workarounds, inline storage for 2- and 9-element states, and many 2-element allocations. Estimated 100 to 300 lines, unverified for ports 01 and 02. |
| Effort | L |
| Acceptance criteria | Fixed arrays compile, run, and pass drop/borrow tests; constant indices have a proof path; generated C stores elements inline; no allocation for `[Float64; 2]`. |
| Validation | New fixtures, generated-C inspection, ASan, port 01/02 focused reconstruction, port 06 pattern test. |
| Parallel | ARR-02 design, MAT-01, PERF-02 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/fixed_array_basic.valvet` and `.stdout`, `valvet-tests/compile-pass/fixed_array_owned.valvet` and `.stdout`, `valvet-tests/compile-fail/fixed_array_len_mismatch.valvet` and `.error`; modify `crates/valvet_core/src/ast.rs`, `crates/valvet_core/src/parser.rs`, `crates/valvet_core/src/sema.rs`, `crates/valvet_core/src/ownership.rs`, `crates/valvet_core/src/codegen.rs`, `docs/implementation/ARCHITECTURE.md`. Full edit, sources, completion, and risk: the `ARR-01` row in the `Exact File Changes Required` table below. |

## ARR-02: Tuple returns and destructuring

| Field | Value |
| --- | --- |
| Task ID | ARR-02 |
| Title | Add tuple returns and destructuring |
| Category | Compiler |
| Priority | P1 |
| Dependencies | ARR-01 |
| Affected ports | 01, 02, 03, 06 |
| Evidence | XRD D6/G7; PORT-01 `Pair`/`V4Pair`; PORT-02 `Pair`; PORT-03 tuple branch; PORT-06 `ArrayPartition` |
| Proposed API | `(Float64, Float64)`, `(Vec<Float64>, Vec<Float64>)`, positional access and destructuring. |
| Implementation steps | 1. Define tuple layout and copy/move/drop rules. <br> 2. Add type syntax, construction, and destructuring. <br> 3. Decide whether tuple fields are named or positional only in v1. <br> 4. Add tests for nested tuples and owned elements. |
| Expected benefit | Removes `Pair` workarounds and enables `sincos` and partition returns without ad hoc structs. |
| Effort | L |
| Acceptance criteria | Tuple returns and destructuring pass fixtures; owned elements drop correctly; no hidden allocation for scalar tuples. |
| Validation | New fixtures, generated-C review, ASan, port 01/02 pattern test. |
| Parallel | ARR-01 implementation after design, MAT-01, PERF-02 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/tuple_return.valvet` and `.stdout`, `valvet-tests/compile-pass/tuple_owned.valvet` and `.stdout`, `valvet-tests/compile-fail/tuple_arity.valvet` and `.error`; modify `crates/valvet_core/src/ast.rs`, `crates/valvet_core/src/parser.rs`, `crates/valvet_core/src/sema.rs`, `crates/valvet_core/src/ownership.rs`, `crates/valvet_core/src/codegen.rs`. Full edit, sources, completion, and risk: the `ARR-02` row in the `Exact File Changes Required` table below. |

## MAT-01: `Mat` element get/set

| Field | Value |
| --- | --- |
| Task ID | MAT-01 |
| Title | Add general mutable element access to `Mat` |
| Category | Compiler |
| Priority | P1 |
| Dependencies | None |
| Affected ports | 03; future matrix work in 07, 08, 09 |
| Evidence | XRD G11; PORT-03 flat row-major `Vec<Float64>`; `codegen.rs:2928-3047` has kernels but no general index |
| Proposed API | `m.get(i, j): Option<Float64>`, `m.set(i, j, value): Result<Unit, ShapeError>`, or an equivalent index form. `ShapeError` carries the offending index and matrix dimensions. |
| Implementation steps | 1. Define row-major semantics and bounds behavior. <br> 2. Add runtime and codegen support. <br> 3. Add tests for rectangular, empty, and out-of-range matrices. <br> 4. Do not add a second matrix type. |
| Expected benefit | Lets port 03 and future matrix code use the existing `Mat` kernels instead of a parallel flat representation. |
| Effort | M |
| Acceptance criteria | Get/set work within bounds; out-of-bounds returns the documented error; no behavior change to existing `Mat` methods. |
| Validation | Focused fixtures, ASan, port 03 reconstruction spike. |
| Parallel | ARR-01, PERF-02, CPLX-01 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/mat_get_set.valvet` and `.stdout`; modify `crates/valvet_core/src/sema.rs`, `crates/valvet_core/src/codegen.rs`. Full edit, sources, completion, and risk: the `MAT-01` row in the `Exact File Changes Required` table below. |

## LINALG-01: Complex 2x2 solve wrapper

| Field | Value |
| --- | --- |
| Task ID | LINALG-01 |
| Title | Add a narrow complex 2x2 solve to `std.linalg` |
| Category | New Library |
| Priority | P1 |
| Dependencies | CPLX-01, MAT-01 (optional) |
| Affected ports | 09; future FIRK |
| Evidence | XRD G6; PORT-09 `LU1 = lu(...)` and `\`; real-arithmetic 2x2 expansion |
| Proposed API | `solve2x2_complex(a: &CMat2, b: &CVec2): CVec2` or an equivalent narrow function. |
| Implementation steps | 1. Define a 2x2 complex struct or reuse `Complex` fields. <br> 2. Implement the same real-arithmetic expansion as the oracle. <br> 3. Do not call LAPACK. <br> 4. Add tests for the Radau3 matrix and a singular matrix. |
| Expected benefit | Removes the local solve and prepares for future implicit FIRK without a general BLAS. |
| Effort | S |
| Acceptance criteria | Matches port 09 bit-exactly; singular behavior documented; no second complex type. |
| Validation | Port 09 332/332, focused linear algebra fixture, ASan. |
| Parallel | NUM-03, ARR-01, TEST-01 |
| Status | Complete. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `stdlib/linalg.valvet`, `valvet-tests/compile-pass/linalg_complex2.valvet` and `.stdout`. Port 09 source is external. Full edit, sources, completion, and risk: the `LINALG-01` row in the `Exact File Changes Required` table below. |

## PERF-02: Inline-friendly `Vec` kernels

| Field | Value |
| --- | --- |
| Task ID | PERF-02 |
| Title | Make simple `Vec` kernels inline-friendly |
| Category | Compiler |
| Priority | P1 |
| Dependencies | COL-01, COL-02 |
| Affected ports | 04, 05, 06, 07, 08, 09, 10 |
| Evidence | AUD 5/6; `codegen.rs:2502-2530`; out-of-line static push/get; 2-element kernels |
| Proposed API | No user API change. Codegen may mark small push/get/len kernels `static inline` or emit direct field access where safe. |
| Implementation steps | 1. Add an inline policy for small fixed kernels. <br> 2. Avoid forcing large loop bodies inline. <br> 3. Measure code size and hot-loop time. <br> 4. Keep the C output valid at `-O0` and `-O2`. |
| Expected benefit | Reduces call overhead in 2-element kernels and enables capacity-check elimination. |
| Effort | M |
| Acceptance criteria | No output change; no default binary size regression over 5%; hot-loop time not worse; generated C readable. |
| Validation | `objdump`, `nm -S`, warm loops, binary size, ASan. |
| Parallel | PERF-03, PERF-06, ARR-01 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Modify `crates/valvet_core/src/codegen.rs` (`emit_adts` prototype block, `emit_vec_fns`, `emit_vec_method`); add assertions to `crates/valvet_core/tests/codegen_fp.rs`. Full edit, sources, completion, and risk: the `PERF-02` row in the `Exact File Changes Required` table below. |

## PERF-03: Bounds-check elision or `@inbounds`

| Field | Value |
| --- | --- |
| Task ID | PERF-03 |
| Title | Add proven-index bounds elision or explicit unchecked blocks |
| Category | Compiler |
| Priority | P1 |
| Dependencies | ARR-01; proof design is part of this task |
| Affected ports | All numeric ports; evidence from 07, 08, 09, 10 |
| Evidence | AUD 6; 1,051 bounds checks in port 07 C; constant-index checks; bounds microbenchmark inconclusive |
| Proposed API | Constant-index elision; optional `@inbounds` block or equivalent. |
| Implementation steps | 1. Write a conservative proof for literal indices and fixed-array lengths. <br> 2. Add an explicit unchecked mechanism only if the proof cannot cover the hot loop. <br> 3. Keep ASan behavior for unchecked accesses and document the safety contract. <br> 4. Add negative tests for out-of-range access. |
| Expected benefit | Removes predictable branches in small fixed-size kernels. Do not claim a speedup before measurement. |
| Effort | L |
| Acceptance criteria | Constant indices in fixed arrays emit no bounds branch; unchecked blocks are explicit and documented; checked paths retain ASan coverage. |
| Validation | Bounds-check count, generated C, ASan, warm loops, all oracles. |
| Parallel | PERF-02, PERF-04, ARR-02 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/inbounds_array.valvet` and `.stdout`, `valvet-tests/compile-fail/inbounds_outside_fn.valvet` and `.error`; modify `crates/valvet_core/src/ast.rs`, `crates/valvet_core/src/parser.rs`, `crates/valvet_core/src/sema.rs`, `crates/valvet_core/src/ownership.rs`, `crates/valvet_core/src/codegen.rs`. Full edit, sources, completion, and risk: the `PERF-03` row in the `Exact File Changes Required` table below. |

## PERF-06: Literal print fast path and peephole

| Field | Value |
| --- | --- |
| Task ID | PERF-06 |
| Title | Avoid allocations for literal print and add output-call peepholes |
| Category | Compiler |
| Priority | P1 |
| Dependencies | IO-01 |
| Affected ports | 04, 05, 06, 07, 08, 09, 10 |
| Evidence | AUD 3; per-label `"="` allocation; 87,033 mallocs in port 07; 81% output share |
| Proposed API | `write("literal")` and `println("literal")` lower to a direct write with the correct newline behavior. The fused label reporter belongs to HARNESS-01. |
| Implementation steps | 1. Add a literal-string fast path. <br> 2. Add a peephole for consecutive write/println calls if output is identical. <br> 3. Keep formatting exact. <br> 4. Measure allocations and wall time. |
| Expected benefit | Removes most label-construction allocations and reduces output share. |
| Effort | M |
| Acceptance criteria | Port 07 output remains 31,122 lines; allocations drop; output byte-identical; no W1002. |
| Validation | Allocation counter, output diff, wall time, ASan. |
| Parallel | PERF-02, PERF-03 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/print_literal_alloc.valvet` and `.stdout`; modify `crates/valvet_core/src/codegen.rs` (`emit_block_body_only`, print branch). Full edit, sources, completion, and risk: the `PERF-06` row in the `Exact File Changes Required` table below. |

## DX-03: Borrow and temporary lifetime ergonomics

| Field | Value |
| --- | --- |
| Task ID | DX-03 |
| Title | Improve temporary borrow and self-assignment ergonomics |
| Category | DX |
| Priority | P1 |
| Dependencies | None; soundness design is part of this task |
| Affected ports | 07, 08, 09; all ports with inline stage construction |
| Evidence | GAPS port 09 item 11 (E0303), port 09 item 10 (E0302), port 07 item 17, port 08 item 9 |
| Proposed API | Temporary lifetime extension for borrowed call arguments; clearer self-assignment pattern; no unsound aliasing. |
| Implementation steps | 1. Write the lifetime and drop rules. <br> 2. Implement temporary extension or a local-binding diagnostic. <br> 3. Add positive and negative fixtures. <br> 4. Review interactions with moves and `&mut`. |
| Expected benefit | Removes repetitive hoists at inline stage construction sites. |
| Effort | L |
| Acceptance criteria | `&f()` is accepted where the temporary lives through the call; unsound cases still rejected; existing borrow fixtures pass. |
| Validation | Borrow-checker fixtures, ASan, full suite, port 09 migration spike. |
| Parallel | DX-04, ARR-01, PERF-03 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-pass/borrow_temp_let.valvet` and `.stdout`, `valvet-tests/compile-fail/borrow_temp_escape.valvet` and `.error`; move `valvet-tests/compile-fail/borrow_temp.valvet` and `.error` to `valvet-tests/compile-pass/borrow_temp.valvet` and `.stdout`; modify `crates/valvet_core/src/ownership.rs`, `crates/valvet_core/src/sema.rs`, `crates/valvet_core/src/codegen.rs`. Full edit, sources, completion, and risk: the `DX-03` row in the `Exact File Changes Required` table below. |

## DX-04: Diagnostic quality pass

| Field | Value |
| --- | --- |
| Task ID | DX-04 |
| Title | Improve borrow, literal, and import diagnostics |
| Category | DX |
| Priority | P1 |
| Dependencies | None |
| Affected ports | All |
| Evidence | E0301, E0302, E0303, E0201, E0602 observations in GAPS ports 06-09 |
| Proposed API | No API change. Better messages, help text, and span coverage. |
| Implementation steps | 1. Audit the five recurring diagnostics. <br> 2. Add a concrete rewrite suggestion where possible. <br> 3. Add tests for the message text and spans. <br> 4. Keep codes stable. |
| Expected benefit | Reduces porting time and makes language rules discoverable. |
| Effort | M |
| Acceptance criteria | Each audited code has a help line and a fixture; no diagnostic regresses. |
| Validation | Diagnostic fixtures, manual review of compiler output. |
| Parallel | DX-03, DOC-01, DOC-02 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `valvet-tests/compile-fail/diag_arity.valvet` and `.error`, `valvet-tests/compile-fail/diag_borrow_site.valvet` and `.error`, `valvet-tests/compile-fail/diag_redundant_cast.valvet` and `.error`; modify `crates/valvet_core/src/diag.rs`, `crates/valvet_core/src/pipeline.rs`, `crates/valvet_core/src/sema.rs`, `crates/valvet_core/tests/front_end.rs`. Full edit, sources, completion, and risk: the `DX-04` row in the `Exact File Changes Required` table below. |

## TEST-01: Benchmark and codegen snapshot harness

| Field | Value |
| --- | --- |
| Task ID | TEST-01 |
| Title | Add a benchmark and generated-code snapshot harness |
| Category | Tooling |
| Priority | P1 |
| Dependencies | VER-01 |
| Affected ports | 07, 08, 09, 10; later all |
| Evidence | AUD 11; manual scripts; no repository baseline |
| Proposed API | `valvet bench <file> --json`; generated C and assembly snapshots; threshold checks. |
| Implementation steps | 1. Define a JSON schema. <br> 2. Wrap warm-loop, allocation, and size measurements. <br> 3. Add snapshot files for key kernels. <br> 4. Add a compare command with tolerance. <br> 5. Do not make snapshots host-dependent where avoidable. |
| Expected benefit | Makes performance and codegen regressions visible in CI. |
| Effort | M |
| Acceptance criteria | One command reports kernel time, allocations, binary size, oracle status, and codegen diff; baseline checked in. |
| Validation | Run against the audit results and confirm the reported values are in range. |
| Parallel | NUM-03, CPLX-01, ARR-01, LINALG-01 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `crates/valvet_cli/src/bench.rs`, `scripts/alloc_count.c`, `scripts/compare_bench.py`, `benchmarks/kernels/axpy.valvet` and `.stdout`; modify `crates/valvet_cli/src/main.rs`. Full edit, sources, completion, and risk: the `TEST-01` row in the `Exact File Changes Required` table below. |

---

# P2 tasks

## ODE-01: `ode` package primitives

| Field | Value |
| --- | --- |
| Task ID | ODE-01 |
| Title | Add narrow ODE protocol primitives as a package |
| Category | New Library |
| Priority | P2 |
| Dependencies | NUM-02, FND-03, COL-02 |
| Affected ports | 04, 05, 06, 07, 08, 09, 10 |
| Evidence | XRD D7/G8; GAPS ports 04-09; constant-cache `nf` asymmetry in ports 07, 08, 09 |
| Proposed API | `OdeStatus`, `increment_nf`, `rotate_fsal`, `convergence_ratio`, norm/residual re-exports. |
| Implementation steps | 1. Create an external package, not `std.ode`. <br> 2. Add counter and FSAL tests. <br> 3. Keep ConstantCache asymmetry explicit. <br> 4. Migrate two ports, not all at once. |
| Expected benefit | Removes 150 to 250 estimated structural lines; makes counters testable. Structural estimate only; it overlaps with NUM-02 norm adapters. |
| Effort | M |
| Acceptance criteria | Two ports pass bit-exactly; no generic solver; asymmetry documented and tested. |
| Validation | Port 07/08/09 oracles, package dependency test, ASan. |
| Parallel | HARNESS-01, NUM-04, DOC-01 |
| Status | Not started; port sources and oracles external. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `packages/ode/valvet.toml`, `packages/ode/ode.valvet`, `packages/ode/README.md`, `valvet-tests/multi-file/ode-package/valvet.toml`, `valvet-tests/multi-file/ode-package/main.valvet`, `valvet-tests/multi-file/ode-package/main.valvet.stdout`. Full edit, sources, completion, and risk: the `ODE-01` row in the `Exact File Changes Required` table below. |

## HARNESS-01: `harness` package label emitter

| Field | Value |
| --- | --- |
| Task ID | HARNESS-01 |
| Title | Add an optional label emitter package |
| Category | New Library |
| Priority | P2 |
| Dependencies | IO-01, PERF-06 |
| Affected ports | 04, 05, 06, 07, 08, 09, 10 |
| Evidence | XRD D8; AUD 3; manual `rep`, `rep8`, `rep9`, `rep10` |
| Proposed API | `emit_label(label: &String, value: Float64)` and typed variants as needed. |
| Implementation steps | 1. Wait for IO-01 and PERF-06. <br> 2. Implement one reporter with the exact label, `=`, and value lines. <br> 3. Migrate one port. <br> 4. Keep RHS fixtures out of the package. |
| Expected benefit | Removes 100 to 200 test-only lines while preserving warning-free output. W1002 removal happens in IO-01, not here. |
| Effort | S |
| Acceptance criteria | Output byte-identical; no W1002; no production solver dependency. |
| Validation | Port output diff, label count, ASan. |
| Parallel | ODE-01, NUM-04 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `packages/harness/valvet.toml`, `packages/harness/harness.valvet`, `packages/harness/README.md`, `valvet-tests/multi-file/harness-package/valvet.toml`, `valvet-tests/multi-file/harness-package/main.valvet`, `valvet-tests/multi-file/harness-package/main.valvet.stdout`. Full edit, sources, completion, and risk: the `HARNESS-01` row in the `Exact File Changes Required` table below. |

## STOCH-01: `stochastic` package weak SRK helpers

| Field | Value |
| --- | --- |
| Task ID | STOCH-01 |
| Title | Add narrow weak SDE helpers; widen only with a second consumer |
| Category | New Library |
| Priority | P2 |
| Dependencies | ODE-01, NUM-01 |
| Affected ports | 10; future weak SDE ports |
| Evidence | XRD G9; PORT-10 `wv_three10`; `srk_dri1_cc.jl:12`; one current consumer |
| Proposed API | `Wiener`, `three_point_increment`; add `two_point_increment` and `is_diagonal_noise` only from a second concrete consumer. |
| Implementation steps | 1. Confirm the port 10 consumer. If absent, defer. <br> 2. Keep scalar and vector branches separate. <br> 3. Preserve the nested predicate and negative quantile. <br> 4. Keep deterministic oracle noise. <br> 5. Do not add the wider helpers without a second consumer. |
| Expected benefit | 20 lines now; future reuse. The v1 surface is limited to the two helpers that port 10 proves. |
| Effort | M |
| Acceptance criteria | Port 10 328/328; branch semantics documented; no RNG in deterministic tests. |
| Validation | Port 10 oracle, ASan, package dependency test. |
| Parallel | NUM-04, DOC-01 |
| Status | Not started; port sources and oracles external. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `packages/stochastic/valvet.toml`, `packages/stochastic/stochastic.valvet`, `packages/stochastic/README.md`, `valvet-tests/multi-file/stochastic-package/valvet.toml`, `valvet-tests/multi-file/stochastic-package/main.valvet`, `valvet-tests/multi-file/stochastic-package/main.valvet.stdout`; defer the body until the port 10 helper is recovered. Full edit, sources, completion, and risk: the `STOCH-01` row in the `Exact File Changes Required` table below. |

## NUM-04: Trixi log-mean helpers

| Field | Value |
| --- | --- |
| Task ID | NUM-04 |
| Title | Add `log_mean` and `inv_log_mean` |
| Category | Library Expansion |
| Priority | P2 |
| Dependencies | FND-04 |
| Affected ports | 01, 02 |
| Evidence | XRD G10; 20 `ln_mean` and 8 `inv_ln_mean` calls; `trixi_math.jl:198-210,238-250` |
| Proposed API | `log_mean(x, y)`, `inv_log_mean(x, y)` with the exact polynomial branch near `f2 < 1e-4`. |
| Implementation steps | 1. Port the exact polynomial branch. <br> 2. Use `evalpoly` with the same order. <br> 3. Add tests near the branch boundary. <br> 4. Migrate only if ports 01 and 02 are restored or a new Trixi port exists. |
| Expected benefit | Removes 40 to 80 estimated helper lines and makes 28 Trixi call sites use one shared implementation. |
| Effort | S |
| Acceptance criteria | Bit-exact against the Trixi oracle or the report's reconstructed values; branch boundary tested. |
| Validation | Port 01/02 oracle or reconstructed fixture. |
| Parallel | ODE-01, HARNESS-01, DOC-01 |
| Status | Not started; exact formulas external. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Modify `stdlib/numeric.valvet`; create `valvet-tests/compile-pass/log_mean.valvet` and `.stdout`; requires Trixi source recovery. Full edit, sources, completion, and risk: the `NUM-04` row in the `Exact File Changes Required` table below. |

## PERF-04: Large aggregate by pointer or move elision

| Field | Value |
| --- | --- |
| Task ID | PERF-04 |
| Title | Remove large by-value state copies |
| Category | Compiler |
| Priority | P2 |
| Dependencies | PERF-02 |
| Affected ports | 07, 08, 09 |
| Evidence | AUD 9; 536-byte `VState07` copy per step; stack frames 4,392/3,112/4,584 bytes |
| Proposed API | No source API change required; improve ABI or document a `&mut State` port rewrite. |
| Implementation steps | 1. Measure the copy on a long run. <br> 2. Decide between compiler move elision and a port rewrite. <br> 3. Implement the lower-risk option. <br> 4. Verify aliasing and drop behavior. |
| Expected benefit | Could improve long simulations; not the first fix for output-bound batteries. |
| Effort | L |
| Acceptance criteria | Port 07 no longer copies 536 bytes per step, or a measured decision records why not; output identical. |
| Validation | Disassembly, warm loops, long-run benchmark, ASan. |
| Parallel | PERF-05, PERF-03 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `docs/implementation/PORT-PERF-04.md`, `valvet-tests/compile-pass/large_state_borrow.valvet` and `.stdout`; change port 07 source outside this repository. Full edit, sources, completion, and risk: the `PERF-04` row in the `Exact File Changes Required` table below. |

## PERF-05: Optional `-O3` profile

| Field | Value |
| --- | --- |
| Task ID | PERF-05 |
| Title | Add an optional math-heavy `-O3` profile |
| Category | Compiler |
| Priority | P2 |
| Dependencies | TEST-01 |
| Affected ports | 04-10 |
| Evidence | AUD 8; numeric loops improve 2.6 to 25 percent; binary size grows 11 to 40 percent; end-to-end up to about 8 percent |
| Proposed API | An opt-in profile, not the default. |
| Implementation steps | 1. Add a manifest or CLI profile. <br> 2. Document size and reproducibility trade-offs. <br> 3. Run all oracles. <br> 4. Keep `-O2` default. |
| Expected benefit | Gives math-heavy users a measured option without changing default reproducibility. |
| Effort | S |
| Acceptance criteria | Profile works; default unchanged; size/time data recorded; outputs byte-identical. |
| Validation | Flag matrix benchmark, oracles, binary size. |
| Parallel | PERF-04, DOC-01 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Modify `crates/valvet_core/src/codegen.rs` (`cc_build_profile`, new `cc_flags`), `crates/valvet_cli/src/main.rs` (help and profile validation), `docs/implementation/IMPLEMENTATION_STATUS.md`; add flag assertions to `crates/valvet_core/tests/codegen_fp.rs`. Full edit, sources, completion, and risk: the `PERF-05` row in the `Exact File Changes Required` table below. |

## DOC-01: Standard-library API reference

| Field | Value |
| --- | --- |
| Task ID | DOC-01 |
| Title | Generate a reference for core standard-library APIs |
| Category | Tooling |
| Priority | P2 |
| Dependencies | FND-01, FND-04, NUM-02, NUM-03, CPLX-01, LINALG-01 |
| Affected ports | All |
| Evidence | Existing stdlib is source-discoverable only; public APIs lack allocation and exactness notes |
| Proposed API | Markdown reference for `std.math`, `std.numeric`, `std.complex`, `std.linalg`. |
| Implementation steps | 1. Extract `pub` declarations. <br> 2. Add semantics, allocation, and exactness notes. <br> 3. Add examples. <br> 4. Check the reference against source in CI. |
| Expected benefit | Makes the new foundations discoverable and reduces reimplementation. |
| Effort | M |
| Acceptance criteria | Every public P0/P1 item in the four modules is documented; P2 additions are documented with their own task; examples compile; no stale signatures. |
| Validation | Doc check script, example builds. |
| Parallel | None; the two documentation follow-ups depend on this task. |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `docs/stdlib/README.md`, `docs/stdlib/math.md`, `docs/stdlib/numeric.md`, `docs/stdlib/complex.md`, `docs/stdlib/linalg.md`, `scripts/check_stdlib_docs.py`, `examples/numeric_core.valvet` and `examples/numeric_core.valvet.stdout`. Full edit, sources, completion, and risk: the `DOC-01` row in the `Exact File Changes Required` table below. |

## DOC-02: Porting guide and template

| Field | Value |
| --- | --- |
| Task ID | DOC-02 |
| Title | Add a porting guide and project template |
| Category | Tooling |
| Priority | P2 |
| Dependencies | DOC-01, TEST-01 |
| Affected ports | All future ports |
| Evidence | Ten ports used manual scripts and repeated setup |
| Proposed API | `docs/porting/` guide and a `valvet new --template port` or repository scaffold. |
| Implementation steps | 1. Document the oracle-first workflow. <br> 2. Include the label protocol, a comparison script, ASan command, and allocation counter; link HARNESS-01 if it exists. <br> 3. Add a small example port. <br> 4. Link the Julia-to-Valvet mapping notes. |
| Expected benefit | Reduces setup time and prevents helper duplication. |
| Effort | M |
| Acceptance criteria | A new port can follow the template to a passing battery without inventing a workflow. |
| Validation | Dry run on a small new port or a reconstructed fragment. |
| Parallel | PKG-01, LANG-01 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `docs/porting/README.md`, `docs/porting/ORACLE.md`, `docs/porting/JULIA_TO_VALVET.md`, `docs/porting/template/valvet.toml`, `docs/porting/template/main.valvet`, `docs/porting/template/oracle.py`, `docs/porting/template/README.md`. Full edit, sources, completion, and risk: the `DOC-02` row in the `Exact File Changes Required` table below. |

## PKG-01: Package discovery and conventions

| Field | Value |
| --- | --- |
| Task ID | PKG-01 |
| Title | Document package naming, discovery, and dependency conventions |
| Category | Tooling |
| Priority | P2 |
| Dependencies | DOC-01 |
| Affected ports | Ecosystem |
| Evidence | `crates/valvet_core/src/manifest.rs`; `resolver.rs` path/git deps; no registry |
| Proposed API | Documentation and `valvet new` conventions, not a registry. |
| Implementation steps | 1. Document manifest fields, path/git deps, lockfile, vendoring, and the current limits of offline/hermetic builds. <br> 2. Define module naming. <br> 3. Add a local package example. <br> 4. Record when a registry would be justified. |
| Expected benefit | Makes the existing package system usable without building a registry prematurely. |
| Effort | S |
| Acceptance criteria | Docs match resolver behavior; example builds; no registry code added. |
| Validation | Example package build, resolver tests. |
| Parallel | DOC-02, LANG-01 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `docs/packages/README.md`, `docs/packages/MANIFEST.md`, `valvet-tests/multi-file/pkg-example/valvet.toml`, `valvet-tests/multi-file/pkg-example/main.valvet`, `valvet-tests/multi-file/pkg-example/main.valvet.stdout`, and `valvet-tests/multi-file/pkg-example/libs/math2/`. Full edit, sources, completion, and risk: the `PKG-01` row in the `Exact File Changes Required` table below. |

## LANG-01: Generics and closures research

| Field | Value |
| --- | --- |
| Task ID | LANG-01 |
| Title | Research generics, monomorphization, and closures |
| Category | Compiler |
| Priority | P2 |
| Dependencies | None |
| Affected ports | All; especially 01, 02, 06 |
| Evidence | `docs/implementation/PLAN-GAP-JULIA.md` J6; XRD G3/G7; `std.numeric` is concrete because user generics do not exist |
| Proposed API | Research memo, no implementation. |
| Implementation steps | 1. Document the current compiler instantiation model for `Vec`/`Mat`. <br> 2. Compare monomorphization options and their effect on compile time and binary size. <br> 3. Define what closures would unlock and what they would cost. <br> 4. Recommend a next step only if a concrete blocker appears. |
| Expected benefit | Prevents premature generic abstractions and gives a factual basis for future language work. |
| Effort | M |
| Acceptance criteria | Memo with options, costs, and a recommendation; no compiler changes. |
| Validation | Review of compiler internals and port evidence. |
| Parallel | DOC-02, PKG-01 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Create `docs/implementation/GENERICS_CLOSURES_RESEARCH.md`; no compiler or library change. Full edit, sources, completion, and risk: the `LANG-01` row in the `Exact File Changes Required` table below. |

## TOOL-01: CI benchmark and codegen gates

| Field | Value |
| --- | --- |
| Task ID | TOOL-01 |
| Title | Add CI benchmark and codegen regression gates |
| Category | Tooling |
| Priority | P2 |
| Dependencies | TEST-01 |
| Affected ports | All |
| Evidence | `.github/workflows/ci.yml` currently runs fmt, clippy, build, tests, release fixtures; no performance or codegen gate |
| Proposed API | CI job with thresholds and artifacts. |
| Implementation steps | 1. Run the benchmark harness on a stable runner. <br> 2. Upload JSON and generated C. <br> 3. Fail on oracle or allocation regressions. <br> 4. Keep timing thresholds advisory unless the runner is stable. |
| Expected benefit | Prevents silent regressions and makes performance evidence durable. |
| Effort | M |
| Acceptance criteria | CI reports benchmark and codegen results; deterministic gates fail on real regressions; no flaky timing gate. |
| Validation | Run on a pull request and on main. |
| Parallel | LANG-01, PKG-01 |
| Status | Not started. See [09_completion_status.md](09_completion_status.md) for files, tests, issues, and evidence. |
| Exact File Changes Required | Modify `.github/workflows/ci.yml`; create `scripts/compare_bench.py`, `benchmarks/baseline.json`, `benchmarks/kernels/axpy.valvet` and `.stdout`. Full edit, sources, completion, and risk: the `TOOL-01` row in the `Exact File Changes Required` table below. |

---

## Exact File Changes Required

This table is the implementation inventory for every task above. Paths are
relative to the repository root and are exact for the current tree. A path
marked external is not in this checkout. Line numbers are omitted because
they drift. The phase tables in `05_implementation_roadmap.md` carry the
compact file list; this table carries the edit, sources, completion check,
and risk. A task is not implementation-ready until the files in its row
exist or the row explicitly records why an artifact is missing.

Status words used below: `candidate` means the code exists but still needs
proof; `blocked` means a required artifact is not in this checkout; `external`
means the file is outside this repository. A row without one of those words is
a planned change whose paths and symbols are confirmed against the current
tree. All rows remain plans, not measurements.

| Task | Exact files, edits, sources, completion, and risk |
| --- | --- |
| FND-01 | Create `valvet-tests/compile-pass/math_scalar.valvet` and `.stdout`. Modify `stdlib/math.valvet` only. Add `pub fun abs(x: Float64): Float64`, `min(a: Float64, b: Float64): Float64`, `max`, `clamp`, `floor`, `ceil`, `trunc`, `round`, `eps`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh`, and `pow_int(x: Float64, n: Int64): Float64`. Keep the existing `sqrt` and double-double helpers unchanged. `abs` must return `0.0 - x` for negative values so `abs(-0.0)` is `+0.0` and NaN passes through. `min` and `max` must propagate a NaN operand and use `1.0 / x` to distinguish signed zeros; do not use `if a < b` alone. `floor`, `ceil`, and `trunc` use the exact integral path for `abs(x) >= 2^52` and an `Int64` cast below that, preserving negative zero. `round` is ties-to-even via `floor` plus the fractional part. `eps(x)` uses a binade helper for normals, returns the minimum subnormal for zero, subnormals, and the smallest normal, and returns the input for non-finite values. Hyperbolics are built from `exp`. `pow_int` uses exponentiation by squaring and a reciprocal for negative exponents, with `pow_int(x, 0) = 1.0`. Tests cover `+0.0`, `-0.0`, NaN, infinities, subnormals, `2^52`, and negative half cases. Sources: Julia math functions (https://docs.julialang.org/en/v1/base/math/) and Rust `round_ties_even` (https://doc.rust-lang.org/std/primitive.f64.html#method.round_ties_even). Done when the fixture, `cargo test`, and the existing math fixtures pass with unchanged output. Risk: API additions only; `eps` at subnormals is the one branch that needs explicit tests. |
| FND-02 | Create `scripts/sqrt_sweep.py` and `valvet-tests/compile-pass/math_sqrt_tiny.valvet` with `.stdout`. The working tree already contains the correctly rounded `wv_sqrt_cr` candidate in `stdlib/math.valvet`; treat that as the candidate under test, not as a new design. `sqrt_sweep.py` generates positive binary64 bit patterns from zero through the smallest normals, emits round-trip decimal literals into a temporary Valvet source, runs the compiler and executable, and compares the result bit pattern with Python `math.sqrt`. Use at least 1,000,000 inputs and a fixed seed; compile in batches of 10,000 values to keep C compile time bounded, and write the mismatch list and summary as JSON. If any input fails, adjust only `wv_sqrt_cr`, `wv_sqrt_mant_even`, or the final correction step, never the public signature. Add the first failing values as a permanent fixture. Sources: IEEE 754 square root (https://en.wikipedia.org/wiki/IEEE_754) and LLVM `llvm.sqrt` semantics (https://llvm.org/docs/LangRef.html#llvm-sqrt-intrinsic). Done when the sweep has 0 mismatches and all existing `sqrt` fixtures still match. The sweep includes `+0.0`, `-0.0`, NaN, and infinities; the current wrapper returns `+0.0` for either zero, so keep that contract unless the fixtures are deliberately regenerated. Risk: the candidate is uncommitted; if that batch is discarded, reapply the same helper before running the sweep. Do not claim a hardware speedup in this task. |
| FND-03 | Create `crates/valvet_core/tests/codegen_fp.rs` and `valvet-tests/compile-pass/sqrt_builtin.valvet` with `.stdout`. Modify `crates/valvet_core/src/sema.rs` in `check_call` to resolve a bare `sqrt` call with exactly one `Float64` argument to `Float64` before user-function lookup. Modify `crates/valvet_core/src/codegen.rs` in the `ExprKind::Call` builtin branch next to `sin` and `cos` to emit `vsqrt_builtin((double)(arg))`. Add this helper to `PRELUDE`: return the input for NaN and `+inf`; return `0.0 / 0.0` for negative finite and `-inf`; return `0.0` for either zero so the builtin matches the current `std.math.sqrt` wrapper; otherwise call `sqrt`. If FND-02 changes the wrapper's zero contract, change both paths in the same task and update the fixtures. In `cc_build_profile`, add `-fno-math-errno` and `-ffp-contract=off`; do not add `-ffast-math`, `-march=native`, or implicit FMA. Extract the flag list into `pub fn cc_flags(profile: &str, lto: bool) -> Vec<String>` so the test can assert the flags without invoking a compiler. PERF-01 and PERF-05 extend this same helper; do not add a second flag function. The generated C test asserts the helper is present and the normal call site uses it. Tiny normals use hardware `sqrt` only after FND-02 proves bit identity; if the sweep fails, route tiny normals to the imported `v_std_math___sqrt_a1` symbol and make that symbol a required part of the fixture. Sources: GCC optimize options (https://gcc.gnu.org/onlinedocs/gcc/Optimize-Options.html). Done when edge fixtures, the generated-C assertion, and `cargo test` pass. Risk: `-fno-math-errno` changes errno behavior only; no generated code reads it. |
| COL-01 | Create `valvet-tests/compile-pass/vec_repeat.valvet` and `.stdout`, plus `valvet-tests/compile-pass/vec_literal_owned.valvet` and `.stdout`. Modify `crates/valvet_core/src/ast.rs` to add `ExprKind::VecRepeat { value: Box<Expr>, len: usize }`. In `crates/valvet_core/src/parser.rs`, extend the `vec![...]` branch in `parse_primary`: after the first element, accept `;` followed by a non-negative integer literal and `]`; reject a negative or non-literal count with E0005, and allow `n == 0`. In `crates/valvet_core/src/sema.rs`, check `VecRepeat` against the expected `Vec<T>` element type, require `T` to be copy in v1, and record the `Vec<T>` instantiation. In `crates/valvet_core/src/codegen.rs`, replace the repeated-push body for the literal and repeat forms with one exact allocation: allocate `n * sizeof(T)` bytes, set `len = n` and `cap = n`, then assign each element directly. For owned literal elements, assignment transfers ownership; for the repeat form, copy each value. For `n == 0`, return an empty vector with `data = NULL`, `len = 0`, and `cap = 0` without calling `malloc`. For `n > 0`, tests assert one `malloc`, no `vvec_*_push`, correct drop for owned elements under ASan, and byte-identical output. Sources: Rust `vec!` macro (https://doc.rust-lang.org/std/macro.vec.html) and `Vec::with_capacity` (https://doc.rust-lang.org/std/vec/struct.Vec.html#method.with_capacity). Done when allocation counters show one allocation and owned fixtures are LSan clean. Risk: `vec![x; n]` remains copy-only in v1; this is an explicit E0201, not a silent clone. |
| COL-02 | Create `valvet-tests/compile-pass/vec_capacity.valvet` and `.stdout`, `valvet-tests/compile-pass/vec_owned_ops.valvet` and `.stdout`, and `valvet-tests/compile-fail/vec_copy_from_short.valvet` and `.error`. Modify `crates/valvet_core/src/ast.rs` to add `ExprKind::AssocCall { base: String, type_args: Vec<Ty>, method: String, args: Vec<Expr> }`. In `crates/valvet_core/src/parser.rs`, in the `Tok::Ident` path of `parse_primary`, parse `Vec<T>::with_capacity(n)` and produce that node; do not add a temporary builtin. In `crates/valvet_core/src/sema.rs`, add the `AssocCall` branch in `check_expr`, allow only `Vec` with one type argument and `with_capacity(Int64)`, and record the instantiation. Extend `check_vec_method` with `capacity() -> Int64`, `reserve(Int64) -> Unit`, `resize(Int64, T) -> Unit`, `fill(T) -> Unit`, and `copy_from(&Vec<T>) -> Unit`; require `T` to be copy for `resize` and `fill` in v1, and allow owned elements for `copy_from` by cloning each source element and dropping replaced destinations. In `crates/valvet_core/src/codegen.rs`, add `vvec_*_with_capacity`, `_capacity`, `_reserve`, `_resize`, `_fill`, and `_copy_from` to the prototypes and definitions. `_with_capacity` allocates exactly `n` and aborts on a negative count; `_reserve` grows to `max(cap * 2, len + n)` with overflow checks; `_resize` drops the tail before shrinking and clones the value into new slots; `_copy_from` refuses a shorter destination via the existing `index out of bounds` abort path and does not resize. If associated-call parsing slips, land the five kernels and methods first; `with_capacity` is the only blocked constructor and must not be replaced by a builtin. Sources: Rust `Vec` capacity and resize docs (https://doc.rust-lang.org/std/vec/struct.Vec.html). Done when capacity values, allocation counts, owned drops, and the short-destination compile-fail case pass. Risk: `resize` and `fill` are copy-only in v1; document that restriction in the task docs. |
| NUM-01 | Create `stdlib/numeric.valvet` with `module std.numeric;`, `import std::math::sqrt;`, and the exact-order P0 surface: `zeros`, `filled`, `clone`, `copy_from`, `fill`, `scale`, `scale_into`, `axpy`, `axpy_into`, `axpy3`, `axpy3_into`, `axpy4`, `axpy4_into`, `dot`, and `norm2`. The norm and residual surface (`rms`, `rms_parts`, `scaled_residual`, and `scaled_residual2`) is added by NUM-02, not by this task. Signatures are the ones in `01_ecosystem_architecture.md` section 6. Implement loops with increasing indices. `dot` uses `acc = 0.0; acc = acc + x[i] * y[i]`; do not call `Vec.dot`, because that kernel returns `Result` and does not expose the sum order. `axpy3` is `(a * x[i] + b * y[i]) + z[i]` with left association; `axpy4` is `((a * x[i] + b * y[i]) + c * z[i]) + w[i]`. `scale_into`, `axpy_into`, and friends require `dst.len() >= source.len()` and write only the first source length. `zeros` uses `Vec<Float64>::with_capacity(n)` then `resize(n, 0.0)`; `filled` uses the same constructor and `resize`. `norm2` is `sqrt(dot(v, v))`. Create `valvet-tests/compile-pass/numeric_core.valvet` with lengths 0, 1, 2, and 5 and exact expected decimals for the core only; NUM-02 adds the residual fixture. Sources: Julia `dot` (https://docs.julialang.org/en/v1/stdlib/LinearAlgebra/) and IEEE 754 addition and multiplication. Done when the fixture matches bit for bit and generated C has no hidden `Vec.add` or `Vec.mul` calls. Risk: `std.numeric` is concrete for `Vec<Float64>` in v1; do not introduce generic parameters. |
| NUM-02 | Modify `stdlib/numeric.valvet`. Define `rms(x)` as `sqrt(sum(x[i] * x[i]) / n)`, with `0.0` for `n == 0`. Define `rms_parts(a, b)` with two explicit loops that accumulate `v[i] * v[i]` for all of `a` then all of `b`, then `sqrt((sum_a + sum_b) / (a.len() + b.len()))`; unequal lengths are allowed and the denominator is the total length. Do not depend on `sum_sq` here; NUM-03 adds the centered `sum_sq(v, c)` form later. Define `scaled_residual(x, u0, u1, alpha, rho)` elementwise as `x[i] / (alpha + max(abs(u0[i]), abs(u1[i])) * rho)`, preserving the left-to-right order of the denominator. Define `scaled_residual2(x, u0a, u1a, u0b, u1b, alpha, rho)` with the first `u0a.len()` elements using the a pair and the remaining elements using the b pair; require `u0a.len() == u1a.len()`, `u0b.len() == u1b.len()`, and `x.len() == u0a.len() + u0b.len()`. Use `std.math.abs` and `std.math.max`; do not use `fmin` or `fmax`. Create `valvet-tests/compile-pass/numeric_residuals.valvet` with zero length, scalar, two-part, unequal `rms_parts`, and sign cases. Sources: SciML residual formulas (https://docs.sciml.ai/DiffEqDocs/stable/) and the port 08 formulas. Done when port 08 and port 09 oracles stay at 280/280 and 332/332 if artifacts are supplied. Risk: the formulas are only as exact as the port source; do not reorder the sum or factor the denominator. |
| IO-01 | Create `valvet-tests/compile-pass/write_no_newline.valvet` and `.stdout`. In `crates/valvet_core/src/sema.rs`, add `write` to the same call-checking branch as `println`, with exactly one printable argument, but do not emit W1002. In `crates/valvet_core/src/codegen.rs`, add a `write` branch that uses the existing `vfmt_*` helpers and no trailing newline; keep `println` on `vprint_*` and `print` as the deprecated alias of `println`. Do not use `write` for the three-line oracle protocol. The fixture writes a string, `Int64`, `Bool`, and `Float64` without separators and then calls `println("")`; the expected output is exactly one concatenated line. Add a generated-C assertion in `crates/valvet_core/tests/front_end.rs` or `crates/valvet_core/tests/codegen_fp.rs` that `write` does not call `vprint_*`. Sources: C `fwrite` (https://en.cppreference.com/w/c/io/fwrite) and Rust `print!` (https://doc.rust-lang.org/std/macro.print.html). Done when output is byte-identical to the concatenation, no W1002 is emitted for `write`, and existing `println` fixtures are unchanged. Risk: `print` remains a warning source; do not remove it in this task. |
| PERF-01 | Create `crates/valvet_core/tests/codegen_fp.rs` if FND-03 has not already created it. Modify `crates/valvet_core/src/codegen.rs`: add `-ffunction-sections` and `-fdata-sections` for the `dev` profile in `cc_build_profile`, and add `-Wl,--gc-sections` to the link arguments for `dev` only. Extract `pub fn cc_flags(profile: &str, lto: bool) -> Vec<String>` from `cc_build_profile` and have the build path use it. Do not add these flags to the default release profile. The test asserts the dev list contains the three flags and the release list does not. Rebuild BDF, FIRK, and weak binaries and record `size` output; the existing phase exit target is a 40 percent text-size drop, but do not repeat a number that was not re-measured. Sources: GCC section flags (https://gcc.gnu.org/onlinedocs/gcc/Optimize-Options.html) and GNU ld `--gc-sections` (https://sourceware.org/binutils/docs/ld/Options.html). Done when output is byte-identical, the flags are asserted, and the size delta is written into `03_performance_and_machine_code.md` as measured or estimated with the command used. Risk: debug info can keep some sections; measure with and without `-g`. |
| DX-01 | Create `valvet-tests/compile-pass/literal_context.valvet` and `.stdout`, plus `valvet-tests/compile-fail/literal_narrowing.valvet` and `.error`. Modify `crates/valvet_core/src/sema.rs` in `check_expr`: for `ExprKind::Int`, consult `self.expected` first. If the expected type is `Int64`, choose `Int64`; if `Int32` and the value fits, choose `Int32`; if `Float64` and the value is exactly representable, choose `Float64`; otherwise keep the current range default. In `ExprKind::Assign`, resolve the place before checking the value for the plain `Set` case, set `expected` to the place type while checking the value, and restore it afterward. This is the fix for `st.s.max_order = 6`; do not add a source cast. Do not change overload resolution globally or make every literal `Int64`. Add `crates/valvet_core/tests/front_end.rs` assertions that the assignment node and argument node get the expected type, and that a literal wider than `Int32` is rejected when the expected type is `Int32`. Sources: Rust literal expressions (https://doc.rust-lang.org/reference/expressions/literal-expr.html) and Julia integer promotion (https://docs.julialang.org/en/v1/manual/conversion-and-promotion/). Done when the fixture compiles with no source casts and the negative fixture fails for the intended reason. Risk: contextual inference can change overload winners; record any changed fixture output explicitly. |
| DX-02 | Modify `crates/valvet_core/src/parser.rs` in `parse_import` and `parse_import_braces`. Remove the E0602 dotted-path rejection. Accept `Tok::Dot` exactly where `Tok::ColonColon` is accepted, including before a brace list, so `import std.math::sqrt;`, `import std::math::sqrt;`, and `import std.math.sqrt;` normalize to the same `ImportDecl.path` and item set. A trailing dot or an empty segment stays E0005. No resolver change is needed because `resolver.rs` already joins the segment vector. Update `valvet-tests/multi-file/dotted/main.valvet` to use `import math.double;`, add `valvet-tests/multi-file/dotted/math.valvet` with `module math;` and `pub fun double(n: Int64): Int64`, delete `main.valvet.error`, and add `main.valvet.stdout`. Add `valvet-tests/multi-file/dotted-std/main.valvet` that imports `std.math::sqrt` and a duplicate import under an alias, and assert one load. Add a parser-only test in `crates/valvet_core/tests/front_end.rs` that compares the three spellings. Sources: Rust paths (https://doc.rust-lang.org/reference/paths.html). Done when the multi-file fixture builds, both spellings resolve to one module unit, and duplicate imports do not double-load. Risk: accepting more spellings can hide a typo; keep the canonical diagnostic text showing `::`. |
| VER-01 | Create `scripts/compare_oracles.py`, `scripts/oracle_schema.json`, `scripts/README.md`, and `scripts/tests/test_compare_oracles.py`. Use only the Python standard library. The CLI accepts `--actual`, `--expected`, `--port`, `--json`, `--one-ulp`, and `--oracles-root`; `--all` reads a manifest mapping port names to expected and actual paths. Parse the three-line label protocol: label line, `=`, value line. Compare numeric values by binary64 bit pattern; count exact, one-ulp, missing, extra, and malformed records. Emit a JSON object with `schema`, `port`, `labels`, `exact`, `one_ulp`, `missing`, `extra`, `malformed`, and `status`. Exit 0 only when there are no missing, extra, or malformed records and every difference is allowed by `--one-ulp`; exit 1 otherwise. Do not hardcode `/tmp/opencode/ports` or any retained path. The README states that port artifacts are external; a clean checkout cannot verify them until an oracle root is supplied. Sources: Python `argparse` (https://docs.python.org/3/library/argparse.html) and JSON (https://docs.python.org/3/library/json.html). Done when the unit tests cover exact, one-ulp, missing, extra, and malformed records, and the command fails cleanly when the root is absent. Risk: the actual port outputs are not in the repository, so this task can ship the checker but cannot close the port comparison until artifacts are supplied. |
| FND-04 | Modify `stdlib/math.valvet`. Add `pub fun sincos(x: Float64): (Float64, Float64)` that calls the existing `sin` and `cos` builtins once each, `pub fun hypot(x: Float64, y: Float64): Float64` with the scaled formula `a = max(abs(x), abs(y)); b = min(abs(x), abs(y)); if a == 0.0 { 0.0 } else { a * sqrt(1.0 + (b / a) * (b / a)) }`, `pub fun ldexp(x: Float64, n: Int64): Float64` by exact powers of two, and `pub fun evalpoly(x: Float64, c: &Vec<Float64>): Float64` by Horner from the last coefficient to index 0. `evalpoly` returns `0.0` for an empty vector and must not reverse the vector. `sincos` depends on ARR-02 tuple returns; do not add a temporary struct. Create `valvet-tests/compile-pass/math_special.valvet` with `.stdout` and `hypot(3.0, 4.0) == 5.0`, `hypot` overflow and underflow cases, `ldexp` positive and negative exponents, `evalpoly` order, and `sincos(0.0)`. Sources: Julia math (https://docs.julialang.org/en/v1/base/math/), C `hypot` (https://en.cppreference.com/w/c/numeric/math/hypot), and C `ldexp` (https://en.cppreference.com/w/c/numeric/math/ldexp). Done when the fixture passes and `sincos` uses no heap allocation. Risk: `sincos` is blocked on ARR-02; keep it out of the P0 build until tuples land. |
| CPLX-01 | No `stdlib/complex.valvet` change is confirmed: the file already exports `cnew`, `cadd`, `csub`, `cmul`, `cdiv`, and `cpow_int` at lines 14 to 86. Add `valvet-tests/compile-pass/complex_pow.valvet` and `.stdout` with positive, zero, and negative exponents and a non-unit imaginary part, and use it to lock the existing behavior. Port 09's manual `C09` and `LU09` live outside this checkout; the port-side edit is an external file edit, not a repository path. If the port source is not available, the repository change is only the fixture and the port migration is unverified. Do not add a second `cpow_int`. Sources: C99 complex arithmetic (https://en.cppreference.com/w/c/numeric/complex) and the existing `std.complex` implementation. Done when the fixture passes and `cpow_int(a, 1)` returns the same bits as `a`. Risk: negative exponents can change rounding; test them explicitly and do not add a generic matrix type. |
| NUM-03 | Modify `stdlib/model.valvet`, `stdlib/plot.valvet`, `stdlib/infer.valvet`, and `stdlib/numeric.valvet`. Remove `vec_add` (lines 27 to 35), `vec_sub` (lines 526 to 534), `vec_mean` (lines 536 to 547), and `sum_sq` (lines 549 to 557) from `stdlib/model.valvet`; import `std::numeric::add`, `std::numeric::sub`, `std::numeric::mean`, and `std::numeric::sum_sq`, and update every call site in `design_matrix`, `factor_coding`, and `predict`. `std::numeric::sum_sq` keeps the `(v, c)` centered form used by `model.valvet`. Remove `fmin` (lines 342 to 352), `fmax` (lines 354 to 364), `fmin2` (lines 366 to 371), `fmean` (lines 385 to 393), and `fsd` (lines 396 to 409) from `stdlib/plot.valvet`; import `std::numeric::min_vec`, `max_vec`, `mean`, and `sd`, and replace `fmin2` with `std::math::min`. Remove `mean_i` (lines 192 to 198) from `stdlib/infer.valvet` and import `std::numeric::mean`. `min_vec` and `max_vec` preserve the current `fmin` and `fmax` non-empty precondition; do not add a total empty-vector result in this task. `fmin2` currently returns the second operand when the first is NaN and ignores signed-zero order, so replace it with `std.math.min` only after a plot fixture proves identical output; otherwise keep `fmin2` and record the difference. Add `add`, `sub`, `sum`, `mean`, `min_vec`, `max_vec`, `sum_sq`, and `sd` to `stdlib/numeric.valvet`; `sum` and `sum_sq` return `0.0` for empty input, `mean` of an empty vector returns NaN, and `sd` uses denominator `n - 1` and returns `0.0` for `n < 2`. Preserve every existing output fixture. Add `numeric_reductions.valvet` and `.stdout` for empty, one-element, and signed-zero cases. Sources: Julia `Statistics.mean` and `std` (https://docs.julialang.org/en/v1/stdlib/Statistics/) and Rust iterator reductions (https://doc.rust-lang.org/std/iter/trait.Iterator.html). Done when `model_*`, `plot_*`, and `infer_*` stdout files are byte-identical and no private duplicate helper remains. Risk: `add` and `sub` require equal lengths while the removed `model.valvet` loops truncated at the shorter length; all current call sites are equal length, but a mismatch now aborts instead of silently truncating. Also, `sd` sample versus population is a semantic choice; the task pins sample and must not silently change. |
| ARR-01 | Create `valvet-tests/compile-pass/fixed_array_basic.valvet` and `.stdout`, `valvet-tests/compile-pass/fixed_array_owned.valvet` and `.stdout`, and `valvet-tests/compile-fail/fixed_array_len_mismatch.valvet` and `.error`. Modify `crates/valvet_core/src/ast.rs` to add `Ty::Array { elem: Box<Ty>, len: usize }`, `ExprKind::ArrayLit { elems: Vec<Expr> }`, and `ExprKind::ArrayRepeat { value: Box<Expr>, len: usize }`. In `crates/valvet_core/src/parser.rs`, parse `[T; N]` in `parse_type`, `[a, b, c]` and `[x; N]` in `parse_primary`, and keep index syntax in `parse_postfix`. Reject non-literal lengths and zero-length fixed arrays in v1. In `crates/valvet_core/src/sema.rs`, extend `resolve_ty`, `ty_key`, `is_copy_ty`, `check_expr`, `check_place`, and the index branches; require matching element types and exact length, and report E0201 with expected and found lengths. In `crates/valvet_core/src/ownership.rs`, walk array elements and enforce move, borrow, and drop rules. In `crates/valvet_core/src/codegen.rs`, extend `cty`, `tycode`, `emit_poison`, `drop_call`, `clone_call`, and `type_needs_drop`, and generate `typedef struct { T v[N]; } VArr_T_N;` so arrays can be returned by value. Emit element access as `.v[i]`, and emit array drop and clone helpers only when needed. Update `docs/implementation/ARCHITECTURE.md` with the layout. Sources: Rust arrays (https://doc.rust-lang.org/std/primitive.array.html) and C struct return rules (https://en.cppreference.com/w/c/language/struct). Done when basic, owned, and length-mismatch fixtures pass, generated C has no VLA, and ASan is clean. Risk: C arrays cannot be returned by value, so the struct wrapper is mandatory; do not use a pointer plus length. |
| ARR-02 | Create `valvet-tests/compile-pass/tuple_return.valvet` and `.stdout`, `valvet-tests/compile-pass/tuple_owned.valvet` and `.stdout`, and `valvet-tests/compile-fail/tuple_arity.valvet` and `.error`. Modify `crates/valvet_core/src/ast.rs` to add `Ty::Tuple(Vec<Ty>)`, `ExprKind::Tuple(Vec<Expr>)`, `Pat::Tuple(Vec<Pat>)`, and a `Stmt::ValPat` form or an optional pattern on `Stmt::Val`. In `crates/valvet_core/src/parser.rs`, parse `(T1, T2)` in `parse_type`, `(a, b)` in `parse_primary` when a comma follows the first expression, tuple patterns in `parse_pat`, and `t.0` or `t.1` in the postfix field branch by accepting an integer token after `.`; a one-element `(x)` stays grouping. In `crates/valvet_core/src/sema.rs`, extend `resolve_ty`, `ty_key`, `is_copy_ty`, `check_expr`, `check_pat`, `bind_pat`, and `check_place`; tuple fields are positional, source syntax is `.0` and `.1`, and named fields are rejected. In `crates/valvet_core/src/ownership.rs`, add tuple moves, partial moves, and drop order. In `crates/valvet_core/src/codegen.rs`, generate `typedef struct { T0 v__0; T1 v__1; } VTup_...;`, construct tuple literals directly, lower `.0` to `.v__0`, destructure by field assignment, and emit clone and drop helpers for owned elements. Create `tuple_return.valvet` with `pair(1.0, 2.0)`, nested tuples, and `val (a, b) = pair(...)`; create `tuple_owned.valvet` with `String` fields and `tuple_arity.valvet.error` for the wrong arity. Sources: Rust tuples (https://doc.rust-lang.org/std/primitive.tuple.html) and C struct initialization (https://en.cppreference.com/w/c/language/struct_initialization). Done when the tuple fixtures pass, owned elements drop exactly once, and `sincos` can be declared. Risk: generalizing `Stmt::Val` touches every destructuring site; prefer a separate `Stmt::ValPat` to limit the diff. |
| MAT-01 | Create `valvet-tests/compile-pass/mat_get_set.valvet` and `.stdout`. Modify `crates/valvet_core/src/sema.rs` in the builtin `ShapeError` registration to add `index: Int64`, `axis: Int64`, `rows: Int64`, and `cols: Int64` while keeping `expected` and `found`. Extend `check_mat_method` with `get(i: Int64, j: Int64): Option<Float64>` and `set(i: Int64, j: Int64, value: Float64): Result<Unit, ShapeError>`; accept `Int32` indexes by widening and record `Option<Float64>` and the `Result` instantiation. Modify `crates/valvet_core/src/codegen.rs` in `emit_mat_method` and the Mat helper block to add `vmat_get` and `vmat_set`. `vmat_get` returns `None` for an out-of-range index, otherwise `Some(data[i * cols + j])`. `vmat_set` returns `Err` with `index`, `axis`, `rows`, and `cols` populated, otherwise writes the element and returns `Ok(Unit)`. Existing `ShapeError` construction sites must use designated initializers or the new zero fields. Sources: `mdspan` indexing (https://en.cppreference.com/w/cpp/container/mdspan) and the existing `Mat` kernels. Done when valid and invalid get and set cases pass, existing `transpose`, `det`, `lu_solve`, and `inverse` outputs are unchanged, and ASan is clean. Risk: adding fields to a builtin struct changes its C ABI and any fixture that constructs `ShapeError` by position; flag that as a breaking internal change and update all literals in the same commit. |
| LINALG-01 | Create `stdlib/linalg.valvet` with `module std.linalg;`, imports from `std::complex`, and two local structs: `pub struct CMat2 { a: Complex, b: Complex, c: Complex, d: Complex }` and `pub struct CVec2 { x: Complex, y: Complex }`. Add `pub fun solve2x2_complex(m: &CMat2, rhs: &CVec2): CVec2`. Compute `det = csub(cmul(m.a, m.d), cmul(m.b, m.c))`, `x = cdiv(csub(cmul(m.d, rhs.x), cmul(m.b, rhs.y)), det)`, and `y = cdiv(csub(cmul(m.a, rhs.y), cmul(m.c, rhs.x)), det)`. If `det` is exactly zero, return NaN components instead of a `Result`; document that choice. Do not add a general matrix type or an LU factorization. Create `linalg_complex2.valvet` with a nonsingular real and complex system and a singular case. Port 09's manual solver is external; the repository migration is the `std.linalg` wrapper and the fixture. Sources: Julia linear algebra (https://docs.julialang.org/en/v1/stdlib/LinearAlgebra/) and LAPACK 2x2 solve conventions (https://netlib.org/lapack/lug/node50.html). Done when the nonsingular fixture matches bit-exactly, the singular case is documented, and the package has no dependency on `std.numeric`. Risk: the exact port 09 formula may differ in operation order; compare and adjust only the operation order, not the algebra. |
| PERF-02 | Modify `crates/valvet_core/src/codegen.rs` in the `emit_adts` Vec prototype block and in `emit_vec_fns`. Mark `_new`, `_len`, `_get`, and `_pop` as `static inline`; leave `_push` as a normal function unless a measured test shows it is safe and smaller. In `emit_vec_method`, emit direct member access for `len` and `capacity` instead of a call, and keep the bounds check in `_get` so ASan behavior is unchanged. Do not add `always_inline` to large helpers and do not inline `_clone` or `_drop`. Extend `crates/valvet_core/tests/codegen_fp.rs` to assert that the small helper definitions contain `static inline`, that `len` lowers to `(.len)`, and that `_push` is not force-inlined. Rebuild the default release profile and compare binary size; the guardrail is no more than 5 percent growth without an explicit decision. Sources: GCC inline semantics (https://gcc.gnu.org/onlinedocs/gcc/Inline.html) and LLVM alwaysinline (https://llvm.org/docs/LangRef.html#function-attributes). Done when output is byte-identical, the generated-C assertions pass, and the size delta is recorded. Risk: `static inline` at `-O0` may not inline; that is acceptable because the task targets release hot loops. |
| PERF-03 | Create `valvet-tests/compile-pass/inbounds_array.valvet` and `.stdout`, and `valvet-tests/compile-fail/inbounds_outside_fn.valvet` and `.error`. Modify `crates/valvet_core/src/ast.rs` to add `Stmt::Inbounds { body: Block, span: Span }`. In `crates/valvet_core/src/parser.rs`, accept `@inbounds { ... }` only at statement position inside a function body and reject it at top level with E0005. In `crates/valvet_core/src/sema.rs`, check the block in a new scope; no value is produced. In `crates/valvet_core/src/ownership.rs`, walk the block as a normal block. In `crates/valvet_core/src/codegen.rs`, add an `unchecked: bool` field to the generator, set it while emitting the block, restore it afterward, and skip the `index out of bounds` guard in the index read and assignment paths. Add `fn const_index_in_bounds(&self, idx: &Expr, ty: &Ty) -> bool` and elide the check for a fixed array with a literal index that is less than the array length even without `@inbounds`. Do not elide checks for `Vec`, for non-literal indexes, or for slices. Add a generated-C assertion that the fixed-array fixture contains no `index out of bounds` string and that the same code without `@inbounds` does. Sources: Rust `get_unchecked` contract (https://doc.rust-lang.org/std/primitive.slice.html#method.get_unchecked) and LLVM bounds-check elimination notes (https://llvm.org/docs/Vectorizers.html). Done when the positive fixture runs, the negative fixture is rejected, and ASan still catches an injected out-of-bounds access. Risk: `@inbounds` is unsafe; keep the block small and do not add it to `std.numeric` until a proof or a measured need exists. PERF-02 also edits index and Vec emission, so land the two tasks in separate gated batches. |
| PERF-06 | Create `valvet-tests/compile-pass/print_literal_alloc.valvet` and `.stdout`. Modify `crates/valvet_core/src/codegen.rs` in the `println` and `print` branch and in `emit_block_body_only`. For a string literal argument, emit `fputs` or `fwrite` directly with `c_escape` and no `vstr_from_c` allocation; for `println`, emit one newline after the literal. For adjacent literal `write` or `println` calls, combine them in `emit_block_body_only` into one `fputs` or `printf` when both are compile-time string literals and no formatting side effect exists. Do not combine non-literal calls and do not change `write` ordering relative to `println`. Add a test that the generated C has no `vstr_from_c` for the literal fixture and that output is byte-identical. Use the TEST-01 allocation shim to record the label allocation count; the current port 07 printer consumes about 81 percent of wall time with 87,033 label mallocs (measured in the port 07 report). Sources: C `fwrite` (https://en.cppreference.com/w/c/io/fwrite) and Rust `print!` (https://doc.rust-lang.org/std/macro.print.html). Done when port 07 still emits 31,122 lines, label allocations fall, and output is byte-identical. Risk: peephole merging can reorder output if it crosses a non-literal call; restrict it to immediately adjacent literal statements. |
| DX-03 | Create `valvet-tests/compile-pass/borrow_temp_let.valvet` and `.stdout`, plus `valvet-tests/compile-fail/borrow_temp_escape.valvet` and `.error`. Move `valvet-tests/compile-fail/borrow_temp.valvet` and `.error` to `valvet-tests/compile-pass/borrow_temp.valvet` and `.stdout`, because the scalar let-bound temporary becomes valid under the new rule. In `crates/valvet_core/src/ownership.rs`, extend `Loan` with an explicit temporary owner and end statement. Change `walk_stmt` so a temporary borrowed by `&expr` in a `val` initializer is live to the end of the enclosing block, while temporaries in a normal call remain live only through the statement. Keep the existing shared and mutable loan conflict rules. In `crates/valvet_core/src/sema.rs`, in the `ExprKind::AddrOf` branch, detect a non-place operand, record the temporary lifetime, and report E0303 on use after the block. In `crates/valvet_core/src/codegen.rs`, materialize the temporary with a block-scoped drop instead of the current statement-scoped temp list. Add positive tests for `val r = &vec![1.0, 2.0]` and for a tuple-returning call bound first to a local and then borrowed; add a negative test that returns the reference. Sources: Rust temporary lifetime extension (https://doc.rust-lang.org/reference/destructors.html#temporary-lifetime-extension) and NLL (https://rust-lang.github.io/rfcs/2094-nll.html). Done when the positive fixture compiles, the escape fixture fails with E0303, and ASan reports no use-after-free. Risk: extending every temporary is too broad; extend only `let`-bound borrows and document the rest as statement-scoped. |
| DX-04 | Create `valvet-tests/compile-fail/diag_arity.valvet` and `.error`, `valvet-tests/compile-fail/diag_borrow_site.valvet` and `.error`, and `valvet-tests/compile-fail/diag_redundant_cast.valvet` and `.error`. Modify `crates/valvet_core/src/diag.rs` `Diagnostic::render` to render `help` and the existing `labels` notes in a consistent order and to drop a duplicate label span and text. Modify `crates/valvet_core/src/pipeline.rs` `render_diags` only for deterministic separators. Modify `crates/valvet_core/src/sema.rs` at the E0201 arity sites, the E0302 borrow-site errors, and the W1003 redundant cast warning to use the same structured note and suggestion fields. Do not change error codes. Add `crates/valvet_core/tests/front_end.rs` assertions for the exact code, primary span line, and help text of three representative diagnostics. Sources: rustc diagnostic guidance (https://rustc-dev-guide.rust-lang.org/diagnostics.html) and the existing `TEST_STRATEGY.md`. Done when the fixtures match the rendered output byte for byte, all existing `.error` sidecars still match, and no code or API behavior changes. Risk: broad wording churn can break many golden files; limit to the three listed sites and update their sidecars in the same batch. |
| TEST-01 | Create `crates/valvet_cli/src/bench.rs`, `scripts/alloc_count.c`, `scripts/compare_bench.py`, `benchmarks/kernels/axpy.valvet`, and `.stdout`. In `crates/valvet_cli/src/main.rs`, add `mod bench;` and a `bench` subcommand that calls `bench::run`; support `--iterations`, `--json`, `--stdout`, `--write-baseline`, and `--baseline`, and reuse `build_exe` and `opts_for` rather than duplicating build logic. The JSON schema is `valvet-bench-v1` with `source`, `profile`, `iterations`, `wall_ms`, `median_ms`, `binary_bytes`, `allocations`, `stdout_sha256`, `codegen_sha256`, and `oracle_status`. Run at least 5 warm iterations and discard the first. `alloc_count.c` is an `LD_PRELOAD` shim built with `cc -shared -fPIC -ldl -o liballoc_count.so scripts/alloc_count.c`; it counts `malloc`, `calloc`, `realloc`, and `free` and prints `VALVET_ALLOC_COUNT=<n>` at exit; if the shim is unavailable, write `allocations: null` and fail the strict gate rather than inventing a number. `compare_bench.py` compares two JSON files, fails on oracle or codegen hash mismatch and on allocation growth, and treats timing change as advisory. Create `benchmarks/baseline.json` with `--write-baseline` on the audit host; do not hand-write timing numbers. Sources: GitHub Actions artifacts (https://docs.github.com/en/actions/using-workflows/storing-workflow-data-as-artifacts) and Python JSON (https://docs.python.org/3/library/json.html). Done when the command emits valid JSON, the shim count is stable across three runs, and the comparator fails an injected oracle mismatch. Risk: timing is host-dependent; keep it advisory and gate deterministic fields. |
| ODE-01 | Create `packages/ode/valvet.toml` with `[package] name = "ode"` and `version = "0.1.0"`, `packages/ode/ode.valvet` with `module ode;`, `packages/ode/README.md`, `valvet-tests/multi-file/ode-package/valvet.toml`, `valvet-tests/multi-file/ode-package/main.valvet`, and `valvet-tests/multi-file/ode-package/main.valvet.stdout`. Export exactly `pub struct OdeStatus { nf: Int64, nsolve: Int64, nnonliniter: Int64 }`, `pub fun increment_nf(s: &mut OdeStatus, n: Int64)`, `pub fun rotate_fsal(fsa: &mut Vec<Float64>, fsb: &mut Vec<Float64>)`, and `pub fun convergence_ratio(ndw: Float64, ndwprev: Float64): Float64`. `increment_nf` adds `n` to `nf` and is always an explicit call. `rotate_fsal` swaps the two vectors elementwise and requires equal lengths. `convergence_ratio` returns `ndw / ndwprev` with no clamp; the caller decides whether the value is acceptable. Do not hide the ConstantCache versus mutable-cache `nf` asymmetry. Do not add a `pub import` re-export: `pub import` is E0800 in 0.1.0, so the package cannot re-export `std.numeric` yet. Sources: SciML `OrdinaryDiffEq` (https://docs.sciml.ai/OrdinaryDiffEq/stable/) and the port 07/08/09 counter notes. Done when two ports or a two-function fixture use the primitives without changing labels. Risk: package naming and module resolution are still conventions, not a registry. |
| HARNESS-01 | Create `packages/harness/valvet.toml`, `packages/harness/harness.valvet` with `module harness;`, `packages/harness/README.md`, `valvet-tests/multi-file/harness-package/valvet.toml`, `valvet-tests/multi-file/harness-package/main.valvet`, and `valvet-tests/multi-file/harness-package/main.valvet.stdout`. Export only `pub fun emit_label(label: &String, value: Float64)` for v1. The body is exactly `println(*label); println("="); println(value)` with no extra spaces and no trailing blank line. Do not use `write` for the three-line protocol. Migrate one port reporter and compare the output bytes. The package imports only the language prelude; it must not import `std.numeric` or a solver. Sources: Rust `print!` and `println!` (https://doc.rust-lang.org/std/macro.println.html) and the existing oracle protocol in `02_library_opportunities.md`. Done when the fixture emits exactly three lines, no W1002 appears, and a port output remains line-for-line identical. Risk: typed variants are deferred; add them only when a second value type appears. |
| STOCH-01 | Create `packages/stochastic/valvet.toml`, `packages/stochastic/stochastic.valvet` with `module stochastic;`, `packages/stochastic/README.md`, `valvet-tests/multi-file/stochastic-package/valvet.toml`, `valvet-tests/multi-file/stochastic-package/main.valvet`, and `valvet-tests/multi-file/stochastic-package/main.valvet.stdout`. The v1 surface is exactly `pub struct Wiener { dW: Vec<Float64>, dZ: Vec<Float64>, sqdt: Float64 }` and `pub fun three_point_increment(sq3dt: Float64, quantile: Float64, dW_scaled: Float64): Float64`. The body must be copied from port 10's `wv_three10`, preserving the nested predicate `abs(dW_scaled) > 0.0 - quantile`, then `dW_scaled < quantile`, and preserving negative quantile behavior. This checkout does not contain port 10 source, so do not guess the formula. If the helper cannot be recovered, create only the package manifest and README as a deferred placeholder, keep the task blocked, and do not add a second SRK formula. Do not replace deterministic noise with an RNG in tests. Sources: SciML `StochasticDiffEq` (https://docs.sciml.ai/StochasticDiffEq/stable/) and the port 10 report. Done when a reconstructed deterministic fixture matches the port value bit for bit. Risk: blocked on the missing port 10 artifact; an unverified formula is worse than a deferred task. |
| NUM-04 | Modify `stdlib/numeric.valvet` with `pub fun log_mean(x: Float64, y: Float64): Float64` and `pub fun inv_log_mean(x: Float64, y: Float64): Float64`. Use `evalpoly` for the `f2 < 1e-4` branch and the closed form otherwise; preserve the branch predicate and polynomial coefficient order from Trixi. Create `valvet-tests/compile-pass/log_mean.valvet` and `.stdout` with values on both sides of `f2 = 1e-4`, equal inputs, and values near one. The exact coefficients are not in this checkout. Sources: Trixi `src/auxiliary/math.jl` (https://github.com/trixi-framework/Trixi.jl/blob/main/src/auxiliary/math.jl); the exact commit was not recorded, so mark the coefficients unverified until the source is pinned. Do not add `log_mean` to P0 or P1. Done only when ports 01 and 02 or a reconstructed fixture match the exact polynomial branch. Risk: a guessed coefficient or a moved branch changes low-order bits; keep the task blocked until the source is available. |
| PERF-04 | Recommended path: no compiler ABI change. Create `docs/implementation/PORT-PERF-04.md` and `valvet-tests/compile-pass/large_state_borrow.valvet` with `.stdout`. Change port 07's external `step!` signature to take a mutable borrow of its state type, written with the exact type name from the port source, instead of copying the reported 536-byte state; update each call site to pass the existing `var` state. The compiler already supports mutable borrows, so this is a port rewrite, not a new language feature. The document records the measured copy count, the rewritten signature, and the long-run disassembly check. If a later measurement shows the port rewrite is insufficient, the compiler alternative touches `param_abi`, `emit_proto`, `emit_fun`, and `emit_user_call` in `crates/valvet_core/src/codegen.rs`; do not start that work without a new measurement. Sources: C structure passing (https://en.cppreference.com/w/c/language/struct) and the port 07 report. Done when the long-run binary has no 536-byte state copy at the step call and output is byte-identical. Risk: the port source is outside this checkout; the repository change is the decision record and the borrow fixture. |
| PERF-05 | Modify `crates/valvet_core/src/codegen.rs` so `cc_build_profile` maps `dev` to `-O0 -g`, `release` to `-O2`, and `math` to `-O3`; extract the same `cc_flags` helper. Keep `release` as the default and keep LTO opt-in from the manifest. Modify `crates/valvet_cli/src/main.rs` help text and add validation so an unknown `--profile` value exits 2 with a clear message. Add `crates/valvet_core/tests/codegen_fp.rs` assertions that `math` contains `-O3`, `release` contains `-O2` and not `-O3`, and `dev` contains `-O0`. Rebuild the numeric kernels and record size and timing deltas; the audit measured 2.6 to 25 percent kernel gains, 11 to 40 percent size growth, and up to about 8 percent end-to-end change, so do not generalize those numbers. Sources: GCC `-O3` (https://gcc.gnu.org/onlinedocs/gcc/Optimize-Options.html). Done when the profile is documented, output is byte-identical, and the default remains `-O2`. Risk: `-O3` is not bit-exact by itself; it is safe only with `-ffp-contract=off` and no fast-math. PERF-01 and FND-03 already introduce `cc_flags`; extend that helper and do not create a competing copy. |
| DOC-01 | Create `docs/stdlib/README.md`, `docs/stdlib/math.md`, `docs/stdlib/numeric.md`, `docs/stdlib/complex.md`, `docs/stdlib/linalg.md`, `scripts/check_stdlib_docs.py`, and `examples/numeric_core.valvet` with `.stdout`. Each page documents the signature, semantics, allocation behavior, exactness, and error cases for every `pub` item added by FND-01, FND-04, NUM-01, NUM-02, NUM-03, CPLX-01, and LINALG-01. `check_stdlib_docs.py` parses `pub fun` and `pub struct` declarations from `stdlib/*.valvet`, parses the documented headings and signatures, and exits 1 on a missing or extra item. Do not document private helpers. The example uses only the documented API and is picked up by the existing examples test. Sources: rustdoc conventions (https://doc.rust-lang.org/rustdoc/what-is-rustdoc.html) and Julia docstring conventions (https://docs.julialang.org/en/v1/manual/documentation/). Done when the checker reports zero missing or extra items and the example stdout matches. Risk: docs can drift; run the checker in CI after TOOL-01. |
| DOC-02 | Create `docs/porting/README.md`, `docs/porting/ORACLE.md`, `docs/porting/JULIA_TO_VALVET.md`, and `docs/porting/template/` with `valvet.toml`, `main.valvet`, `oracle.py`, and `README.md`. The guide covers project layout, path dependencies, the three-line label protocol, exact-order loops, the `sqrt` and `abs` imports, and the rule that ports must not copy `std.numeric` kernels. The template's `oracle.py` calls `scripts/compare_oracles.py` and does not embed port data. The Julia guide records the concrete translations already used in ports 07 to 10: `@..` to explicit loops or array assignment, `similar` to `zeros`, and closures to named functions. Do not claim generics or closures are supported. Sources: existing port reports and the Julia manual (https://docs.julialang.org/en/v1/manual/noteworthy-differences/). Done when a dry run on a reconstructed three-label fragment builds and compares. Risk: the template can outrun the compiler; every example must be compiled in the same change. |
| PKG-01 | Create `docs/packages/README.md`, `docs/packages/MANIFEST.md`, and `valvet-tests/multi-file/pkg-example/` with `valvet.toml`, `main.valvet`, `main.valvet.stdout`, and `libs/math2/valvet.toml` plus `libs/math2/math2.valvet`. Document the current manifest subset: `[package]` name and version, `[dependencies]` path and git forms, lockfiles, vendoring, and `[features]`. State exactly what is supported today: path dependencies and pinned git revisions work; `--offline` blocks network and uses caches or fails; `--hermetic` blocks subprocess and environment reads but is not a full OS sandbox; there is no registry. Do not claim a package index or semver resolver beyond the current manifest. Use the existing `dep-path` fixture as the second example. Sources: Cargo manifest format (https://doc.rust-lang.org/cargo/reference/manifest.html) and the current `crates/valvet_core/src/manifest.rs`. Done when the example builds, runs, and matches stdout, and the docs match the manifest parser. Risk: documenting an aspirational registry would be false; keep the limitations explicit. |
| LANG-01 | Create `docs/implementation/GENERICS_CLOSURES_RESEARCH.md`. Cover monomorphization, dictionary passing, and type erasure for generics; capture-by-reference and capture-by-value for closures; the interaction with the existing ownership and drop analysis; code-size and compile-time costs; and the concrete blockers seen in ports 01 to 10. Recommend a decision, not an implementation. Reuse the evidence already in `08_open_questions_and_research.md` and do not invent demand that is not in the reports. Do not modify the compiler or stdlib. Sources: Rust monomorphization (https://rustc-dev-guide.rust-lang.org/backend/monomorph.html), Swift generics (https://docs.swift.org/swift-book/documentation/the-swift-programming-language/generics/), and the port reports. Done when the memo has a recommendation, alternatives, costs, and a trigger condition for implementation. Risk: research can expand without a consumer; keep the scope to one document. |
| TOOL-01 | Modify `.github/workflows/ci.yml` to add a `perf-gate` job after `build-test`, with `needs: build-test`, a 30-minute timeout, `cargo build --workspace --release`, a bench run on `benchmarks/kernels/axpy.valvet`, a comparator run, and `actions/upload-artifact@v4` for JSON and generated C. Create `scripts/compare_bench.py`, `benchmarks/baseline.json`, and `benchmarks/kernels/axpy.valvet` with `.stdout`. The job fails on oracle mismatch, allocation growth, or codegen hash mismatch; timing thresholds are advisory unless the runner is dedicated. `baseline.json` is generated by `valvet bench --write-baseline` on the audit host, not hand-written. Add `scripts/check_stdlib_docs.py` to the same job only after DOC-01 lands. Sources: GitHub Actions workflow syntax (https://docs.github.com/en/actions/using-workflows/workflow-syntax-for-github-actions) and artifact upload (https://docs.github.com/en/actions/using-workflows/storing-workflow-data-as-artifacts). Done when a pull request run uploads both artifacts and an injected oracle mismatch fails the job. Risk: hosted-runner timing is noisy; never make it a hard gate without a stable runner. |
| PERF-02 follow-up | Modify `crates/valvet_core/src/codegen.rs` only after the initial inline change is measured. Add a per-kernel inline policy table used by `emit_adts` and `emit_vec_fns`; `_new`, `_len`, `_get`, and `_pop` stay inline, and `_push` becomes inline only if the default release size grows by less than 5 percent and the hot-loop benchmark improves. Extend `crates/valvet_core/tests/codegen_fp.rs` to assert the policy and add a size check command. Sources: GCC inline (https://gcc.gnu.org/onlinedocs/gcc/Inline.html). Done when the policy is data-driven and output is unchanged. Risk: inlining `_push` duplicates the realloc path and can hurt instruction locality. |
| PERF-03 follow-up | Modify the `const_index_in_bounds` proof helper in `crates/valvet_core/src/codegen.rs` and the `@inbounds` tests. Add proof for a fixed-array index that is a literal, a loop induction variable bounded by the array length, or a checked `if i < N` branch. Do not add proof for `Vec` or arbitrary arithmetic. Add `valvet-tests/compile-pass/inbounds_proof.valvet` and `.stdout` for the elided path, and run the injected out-of-bounds path under ASan rather than adding it as a passing fixture. Sources: LLVM bounds-check elimination (https://llvm.org/docs/Vectorizers.html) and Rust `get_unchecked` (https://doc.rust-lang.org/std/primitive.slice.html#method.get_unchecked). Done when the proof cases pass, unproved cases keep their checks, and ASan reports the injected fault. Risk: a wrong proof is memory corruption; keep the proof local and test every new pattern. |
| PERF-06 follow-up | Modify `emit_block_body_only` and the `println` and `print` branches in `crates/valvet_core/src/codegen.rs` after the literal peephole lands. Introduce a per-block output buffer only if the allocation and timing measurements show the write syscalls or `printf` calls dominate. Emit one `fwrite` for the concatenated literal prefix and keep every non-literal call in order. Extend `valvet-tests/compile-pass/print_literal_alloc.valvet` with mixed literal and value lines. Sources: C `fwrite` (https://en.cppreference.com/w/c/io/fwrite). Done when output is byte-identical, allocations fall, and the benchmark shows a measured gain. Risk: buffering can delay output and change interleaving with stderr; do not buffer across calls that may abort. |

### Blocked or unverified rows

- `VER-01`: the checker is implementable now, but port comparison is blocked
  until external oracle artifacts and a manifest are supplied.
- `STOCH-01`: blocked on the port 10 `wv_three10` source; do not guess the
  formula.
- `NUM-04`: blocked on the exact Trixi coefficients and branch order; the
  linked `main` source is not a pinned commit.
- `CPLX-01` and `PERF-04`: the repository side is implementable, but the port
  09 and port 07 source changes are external and cannot be verified from this
  checkout.
- `FND-02`: the candidate `sqrt` implementation is in the uncommitted working
  tree; the sweep is the proof, not a new design.

## Task dependency matrix

| Task | Depends on | Unlocks |
| --- | --- | --- |
| FND-01 | None | FND-04, NUM-01, CPLX-01 |
| FND-02 | None | FND-03 |
| FND-03 | FND-02 | NUM-02, CPLX-01, ODE-01, migrations |
| COL-01 | None | NUM-01 |
| COL-02 | None | NUM-01, ODE-01 |
| NUM-01 | FND-01, COL-01, COL-02 | NUM-02, NUM-03, ODE-01, STOCH-01 |
| NUM-02 | NUM-01, FND-03 | ODE-01, migrations |
| IO-01 | None | HARNESS-01, PERF-06 |
| PERF-01 | None | Smaller dev builds |
| DX-01 | None | Cleaner generated code |
| DX-02 | None | Import ergonomics |
| VER-01 | None | TEST-01, migrations |
| FND-04 | FND-01, ARR-02 | NUM-04 |
| CPLX-01 | FND-01, FND-03 | LINALG-01 |
| NUM-03 | NUM-01, NUM-02 | Stdlib consolidation |
| ARR-01 | None | ARR-02, PERF-03 |
| ARR-02 | ARR-01 | Tuple-based APIs |
| MAT-01 | None | LINALG-01, port 03 |
| LINALG-01 | CPLX-01; MAT-01 optional | Future implicit solvers |
| PERF-02 | COL-01, COL-02 | PERF-04 |
| PERF-03 | ARR-01 | Bounds-check reduction |
| PERF-06 | IO-01 | HARNESS-01 |
| DX-03 | None; soundness design is part of this task | Cleaner stage construction |
| DX-04 | None | Better diagnostics |
| TEST-01 | VER-01 | TOOL-01, PERF-05 |
| ODE-01 | NUM-02, FND-03, COL-02 | STOCH-01 |
| HARNESS-01 | IO-01, PERF-06 | Cleaner port tests |
| STOCH-01 | ODE-01, NUM-01 | Future weak SDE |
| NUM-04 | FND-04 | Ports 01/02 |
| PERF-04 | PERF-02 | Long-run performance |
| PERF-05 | TEST-01 | Opt-in speed profile |
| DOC-01 | FND-01, FND-04, NUM-02, NUM-03, CPLX-01, LINALG-01 | DOC-02, PKG-01 |
| DOC-02 | DOC-01, TEST-01 | Future ports |
| PKG-01 | DOC-01 | Package workflow |
| LANG-01 | None | Future generic design |
| TOOL-01 | TEST-01 | Durable CI |

## Critical path and parallel summary

Critical path:

```text
Collection and numeric path:
COL-01 + COL-02 -> NUM-01

Math path:
FND-02 -> FND-03
FND-01 -> NUM-01

Join:
NUM-01 + FND-03 -> NUM-02

Verification gate:
VER-01 (parallel, no dependency on NUM-02)

Final join:
NUM-02 + VER-01 -> port 08/10 migration -> port 09/07 numeric migration
```

`CPLX-01` can follow the first numeric migration. It is not required for the
`NUM-01`/`NUM-02` path.

Independent parallel streams:

- `IO-01 -> PERF-06 -> HARNESS-01`
- `CPLX-01 -> LINALG-01`
- `ARR-01 -> ARR-02`
- `ARR-01 -> PERF-03`
- `PERF-01`, `DX-01`, `DX-02`, `DX-04`
- `VER-01 -> TEST-01 -> TOOL-01`

Do not start `ODE-01`, `STOCH-01`, or complex linear algebra before the first
`std.numeric` migration. `ARR-01` can start after its design gate, but it must
not block the migration. Those items are real, but they are not the first
payoff.
