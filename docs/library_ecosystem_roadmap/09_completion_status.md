# 09 Completion status and evidence

Status snapshot for the roadmap. This file records only work that was actually
implemented and tested in this checkout. Port artifacts and external oracles
are not present, so port migration and oracle verification remain external.

## Summary

| Task | Status | Record |
| --- | --- | --- |
| FND-01 | Complete | [FND-01](#fnd-01) |
| FND-02 | Complete | [FND-02](#fnd-02) |
| FND-03 | Complete | [FND-03](#fnd-03) |
| COL-01 | Complete | [COL-01](#col-01) |
| COL-02 | Complete with a documented fixture deviation | [COL-02](#col-02) |
| COL-03 | Complete | [COL-03](#col-03) |
| NUM-01 | Complete | [NUM-01](#num-01) |
| NUM-02 | Complete | [NUM-02](#num-02) |
| IO-01 | Complete | [IO-01](#io-01) |
| PERF-01 | Complete for the repository fixture | [PERF-01](#perf-01) |
| DX-01 | Complete | [DX-01](#dx-01) |
| DX-02 | Complete | [DX-02](#dx-02) |
| VER-01 | Complete checker; port verification blocked | [VER-01](#ver-01) |
| CPLX-01 | Complete in-repository fixture | [CPLX-01](#cplx-01) |
| NUM-03 | Partial | [NUM-03](#num-03) |
| FND-04 | Not started | Depends on ARR-02 for tuple returns |
| ARR-01 | Not started | Large language and ABI task |
| ARR-02 | Not started | Large language and ABI task |
| ARR-03 | Complete | [ARR-03](#arr-03) |
| ARR-04 | Complete | [ARR-04](#arr-04) |
| MAT-01 | Complete | [MAT-01](#mat-01) |
| LINALG-01 | Complete | [LINALG-01](#linalg-01) |
| PERF-02 | Not started | Depends on COL-01 and COL-02 |
| PERF-03 | Not started | Large borrow and codegen task |
| PERF-06 | Not started | Depends on IO-01 |
| DX-03 | Not started | Requires a soundness review |
| DX-04 | Not started | Diagnostic redesign task |
| TEST-01 | Not started | Depends on VER-01 and PERF-05 |
| ODE-01 and later P2 tasks | Not started | Port sources and oracles are external |

## P0 tasks

### FND-01

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `stdlib/math.valvet`; `valvet-tests/compile-pass/math_scalar.valvet`; `valvet-tests/compile-pass/math_scalar.valvet.stdout` |
| Implementation summary | Added public `abs`, `min`, `max`, `clamp`, `floor`, `ceil`, `trunc`, `round`, `eps`, `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh`, and `pow_int`. NaN and signed-zero behavior is explicit. `round` uses ties-to-even. `pow_int` uses exponentiation by squaring without reassociation. |
| Sources verified | Roadmap APIs in `01_ecosystem_architecture.md` section 6.1 and `06_actionable_task_list.md` FND-01; existing `exp`, `log`, and `sqrt` implementations in `stdlib/math.valvet`; IEEE-754 behavior checked through fixtures. |
| Tests executed | `valvet run valvet-tests/compile-pass/math_scalar.valvet`; full debug and release `valvet test valvet-tests`; ASan/UBSan run with output identical to the golden file. Cases include signed zero, NaN, infinities, `2^52`, negative halves, subnormal `eps`, and empty-vector helpers. |
| Issues found | No blocking issue. `abs(-0.0)` intentionally returns `+0.0`. The fixture uses `1.0 / f(x)` to observe signed zero. |
| Completion evidence | Fixture output contains `3`, `inf`, `-inf`, `4.9406564584124654e-324`, `-2` for `round(-2.5)`, and `true` for all edge predicates. `math_scalar.valvet` passes in the 300-fixture suite. |

### FND-02

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `scripts/sqrt_sweep.py`; `valvet-tests/compile-pass/math_sqrt_tiny.valvet`; `valvet-tests/compile-pass/math_sqrt_tiny.valvet.stdout` |
| Implementation summary | The sweep samples positive finite binary64 patterns, including zero, subnormals, both normal boundaries, 1.0, and the maximum finite value. It compares printed results by bit pattern against host `math.sqrt`. The custom `wv_sqrt_cr` route was superseded for public calls by the FND-03 guarded builtin. |
| Sources verified | FND-02 row in `06_actionable_task_list.md`; `stdlib/math.valvet`; host CPython `math.sqrt` as an IEEE-754 oracle. |
| Tests executed | Earlier 1,000-input and 10,000-input sweeps reported `0` mismatches. The final 1,000,000-input run uses `target/release/valvet` and checked exactly 1,000,000 values with `0` mismatches. `math_sqrt_tiny.valvet` covers `1e-323`, `0x00000b11fd3b80b2`, the smallest normal from both sides, and the maximum finite value. |
| Issues found | The custom `wv_sqrt_cr` candidate still fails maximum-range inputs. Public `sqrt` resolves to `vsqrt_builtin`, which is exact on the sweep and preserves negative, zero, NaN, and infinity behavior. The sampler was corrected to return exactly the requested count instead of silently de-duplicating to 999,996 values. |
| Completion evidence | `evidence/sqrt_sweep_1m.json` reports `requested: 1000000`, `checked: 1000000`, `mismatches: 0`, `status: ok`. |

### FND-03

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `crates/valvet_core/src/sema.rs`; `crates/valvet_core/src/codegen.rs`; `crates/valvet_core/tests/codegen_fp.rs`; `valvet-tests/compile-pass/sqrt_builtin.valvet`; `valvet-tests/compile-pass/sqrt_builtin.valvet.stdout` |
| Implementation summary | Bare `sqrt` with one `Float64` argument resolves before user functions. Codegen emits `vsqrt_builtin`, which returns NaN for negatives, preserves NaN, infinity, and zero, and calls hardware/libm `sqrt`. `cc_flags` adds `-fno-math-errno` and `-ffp-contract=off`. |
| Sources verified | FND-03 row in `06_actionable_task_list.md`; existing `sin`/`cos` builtin path; C flags measured with the local GCC toolchain. |
| Tests executed | `cargo test -p valvet_core --test codegen_fp`; `sqrt_builtin.valvet`; full 300-fixture debug and release suites; 10,000-input sweep. |
| Issues found | The wrapper preserves the previous edge behavior, including signed zero (`sqrt(-0.0)` returns `-0.0`). Deviation: the builtin handles every finite positive range, not only the range outside tiny normals. This keeps one public path; the 1,000,000-input sweep is the evidence. `-ffp-contract=off` is mandatory for exact-order arithmetic and is now in the shared flag helper. |
| Completion evidence | Generated C calls `vsqrt_builtin`; the fixture prints exact values for `9.0`, `0.0`, `-1.0` (NaN), infinity, and a subnormal. Clippy and all tests pass. |

### COL-01

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `crates/valvet_core/src/ast.rs`; `crates/valvet_core/src/parser.rs`; `crates/valvet_core/src/sema.rs`; `crates/valvet_core/src/ownership.rs`; `crates/valvet_core/src/resolver.rs`; `crates/valvet_core/src/codegen.rs`; `valvet-tests/compile-pass/vec_repeat.valvet`; `valvet-tests/compile-pass/vec_repeat.valvet.stdout`; `valvet-tests/compile-pass/vec_literal_owned.valvet`; `valvet-tests/compile-pass/vec_literal_owned.valvet.stdout`; `crates/valvet_core/tests/codegen_fp.rs` |
| Implementation summary | `vec![a, b, c]` allocates one buffer with exact `len` and `cap`, then assigns elements directly. `vec![value; n]` uses `ExprKind::VecRepeat` and only accepts copyable values. Owned literals are moved into the buffer, source slots are nulled, and drop glue runs once. |
| Sources verified | COL-01 row in `06_actionable_task_list.md`; existing `Vec` layout and drop helpers in `crates/valvet_core/src/codegen.rs`; ownership rules in `crates/valvet_core/src/ownership.rs`. |
| Tests executed | `cargo test -p valvet_core --test codegen_fp`; `vec_repeat.valvet`; `vec_literal_owned.valvet`; ASan/UBSan with leak detection; full 300-fixture suite. Generated-C assertions require exactly one `valloc_or_abort` and no `_push(&` in `main` for non-empty literals. |
| Issues found | Empty `vec![]` intentionally allocates nothing, so the one-allocation assertion applies only to non-empty literals. |
| Completion evidence | Test `vec_literal_and_repeat_allocate_once_without_push` passes. Generated C for `vec![3; 5]` contains one `valloc_or_abort((size_t)5 * sizeof(int32_t))` and one `_vri` loop. Owned fixture prints expected values and is ASan clean. |

### COL-02

| Field | Value |
| --- | --- |
| Status | Complete with one fixture deviation |
| Files changed | `crates/valvet_core/src/ast.rs`; `crates/valvet_core/src/parser.rs`; `crates/valvet_core/src/sema.rs`; `crates/valvet_core/src/ownership.rs`; `crates/valvet_core/src/resolver.rs`; `crates/valvet_core/src/codegen.rs`; `valvet-tests/compile-pass/vec_capacity.valvet`; `valvet-tests/compile-pass/vec_capacity.valvet.stdout`; `valvet-tests/compile-pass/vec_owned_ops.valvet`; `valvet-tests/compile-pass/vec_owned_ops.valvet.stdout`; `valvet-tests/compile-fail/vec_owned_resize.valvet`; `valvet-tests/compile-fail/vec_owned_resize.valvet.error`; `crates/valvet_core/tests/codegen_fp.rs` |
| Implementation summary | Added `Vec<T>::with_capacity(n)` with an overflow-checked single allocation. Added `capacity`, `reserve`, `resize`, `fill`, and `copy_from`. `reserve` grows to `max(cap * 2, len + n)` with overflow checks. `resize` and `fill` require copyable elements. `copy_from` requires a destination at least as long as the source, clones owned elements, drops replaced destinations, and refuses a shorter destination at runtime. |
| Sources verified | COL-02 row in `06_actionable_task_list.md`; existing `Vec` layout, clone, drop, and bounds-check paths in `crates/valvet_core/src/codegen.rs`. |
| Tests executed | `vec_capacity.valvet`; `vec_owned_ops.valvet`; `vec_owned_resize.valvet` compile-fail; generated-C kernel assertions; manual short-destination run; ASan/UBSan with leak detection; full debug and release suites. |
| Issues found | The roadmap asked for `compile-fail/vec_copy_from_short.valvet` and `.error`, but a short destination is a runtime condition, not a static one. The implementation aborts with `index out of bounds` and exit code 1. The compile-fail slot is used for `vec_owned_resize.valvet`, which rejects owned `resize`. Integer counts accept `Int32` and `Int64`, a superset of the documented `Int64` signature. |
| Completion evidence | `vec_capacity` prints `0, 3, 3, 4, 6, 2.5, 2.5, 12, 7, 1, 6, 12`. `vec_owned_ops` prints `x, y, z` and is ASan clean. Short `copy_from` exits 1 with `index out of bounds`. Generated C contains the five new kernels and one `with_capacity` allocation. |

### COL-03

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `crates/valvet_core/src/ast.rs`; `crates/valvet_core/src/parser.rs`; `crates/valvet_core/src/sema.rs`; `crates/valvet_core/src/ownership.rs`; `crates/valvet_core/src/codegen.rs`; `crates/valvet_core/tests/front_end.rs`; `docs/implementation/DICT_SPEC.md`; `docs/implementation/ARCHITECTURE.md`; `README.md`; `valvet-tests/compile-pass/string_index_basic.valvet` and `.stdout`; `string_index_group.valvet` and `.stdout`; `string_index_growth.valvet` and `.stdout`; `string_index_owned.valvet` and `.stdout`; `string_index_move.valvet` and `.stdout`; `string_index_discard.valvet` and `.stdout`; `string_index_containers.valvet` and `.stdout`; `valvet-tests/compile-fail/string_index_unknown_method.valvet`, `string_index_key_type.valvet`, `string_index_not_indexable.valvet`, `string_index_use_after_move.valvet`, `string_index_print.valvet`, `string_index_constructor_arity.valvet`, and `string_index_method_arity.valvet` with `.error` sidecars |
| Implementation summary | Added `StringIndex`, an ordered string-keyed index with `Int64` positions. The API is `StringIndex::new()`, `len`, `contains`, `get -> Option<Int64>`, `get_or_insert -> Int64`, `keys -> Vec<String>`, and deep `clone`. The C layout is `typedef struct { VStr *keys; uint64_t *hashes; int64_t len; int64_t cap; int64_t *buckets; int64_t bucket_cap; } VStringIndex_;`. Keys are insertion-ordered. Hashes are cached FNV-1a. Buckets use open addressing with power-of-two capacity and a load factor of at most 0.5. `get_or_insert` borrows its key and clones only on insertion. The type is move-only; moved-from values are poisoned. Drop and clone work directly and inside structs, enums, `Option`, `Vec`, and `ArrayD`. `keys` and `clone` are the only method results that may be discarded as expression statements. The type is not a general `Map<K,V>`. Deletion, non-string keys, map literals, `idx[key]`, printing the index itself, and join multimaps remain out of scope. |
| Sources verified | `docs/implementation/DICT_SPEC.md`; the dictionary/map audit at `/tmp/opencode/valvet-dict-map-audit.md`; existing `Vec`, `ArrayD`, `Factor`, and owned-type layout, drop, clone, and poison conventions in `crates/valvet_core/src/codegen.rs`. |
| Tests executed | Nine `string_index_*` front-end tests; seven compile-pass fixtures; seven compile-fail fixtures; full debug and release fixture suites; `valvet test --doc`; `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`; ASan/UBSan/LSan on all seven compile-pass fixtures with byte-identical output. |
| Issues found | The associated-call branch originally returned before inserting the expression type, so `return StringIndex::new();` lowered as a `Unit` return. The success path now uses `self.insert(e.id, t)`. Discarding `keys()` or `clone()` needed a narrow `Stmt::Expr` exception; all other method results still report `E0201`. The stdlib has not migrated to `StringIndex`, so no measured speedup is claimed. Joins need a separate multimap design. |
| Completion evidence | `string_index_basic` prints `0`, `false`, `None`, positions `0` and `1`, duplicate `0`, `[a, b]`, and the clone length `2`. `string_index_group` prints `[east, west, north]` and `[20, 20, 40]`. `string_index_growth` inserts 20 keys across rehashes, preserves positions `0` and `19`, and keeps first-appearance order `k0` through `k19`. `string_index_owned` proves the keys snapshot and clone are independent. `string_index_move` covers parameter, struct, and return moves plus a borrowed reference receiver through explicit `(*idx)`. `string_index_containers` covers `Vec`, `ArrayD`, `Option`, enum payloads, and a struct field cloned through `Vec<Indexed>`. The discard fixture, including discarded results on temporary `StringIndex` receivers, is leak-free under ASan. Generated C contains the exact layout, `vsi_*`, `v_drop_StringIndex_`, and `v_clone_StringIndex_`, with no `<error>` text and no unresolved helper names. |

### NUM-01

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `stdlib/numeric.valvet`; `valvet-tests/compile-pass/numeric_core.valvet`; `valvet-tests/compile-pass/numeric_core.valvet.stdout` |
| Implementation summary | Added the P0 exact-order surface: `zeros`, `filled`, `clone`, `copy_from`, `fill`, `scale`, `scale_into`, `axpy`, `axpy_into`, `axpy3`, `axpy3_into`, `axpy4`, `axpy4_into`, `dot`, and `norm2`. Loops are increasing-index loops. `axpy` chains are left-associated. `_into` forms write only `src.len()` elements and require a long enough destination. |
| Sources verified | `01_ecosystem_architecture.md` section 6.2; NUM-01 row in `06_actionable_task_list.md`; generated C reviewed for absence of `Vec.add` or `Vec.mul` calls. |
| Tests executed | `numeric_core.valvet`; exact values cross-checked with Python formulas in the same operation order; ASan/UBSan with leak detection; full suite; clippy. |
| Issues found | `zeros` and `filled` require `var` bindings internally because index assignment mutates. Reference parameters need explicit dereference for method calls. No hidden allocation in the loops. |
| Completion evidence | `numeric_core` prints `0, 3.5, 1.78125, 7.416198487095663, 2, 10, 3, 15, 3.5, 10.09375, 6.375, 7.5, 30.09375, 10, 30.25, 3, 4, 9, 9`. Generated C contains no `vvec_Vec_Float64_add` or `vvec_Vec_Float64_mul` call from `std.numeric`. |

### NUM-02

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `stdlib/numeric.valvet`; `valvet-tests/compile-pass/numeric_residuals.valvet`; `valvet-tests/compile-pass/numeric_residuals.valvet.stdout` |
| Implementation summary | Added `rms`, `rms_parts`, `scaled_residual`, and `scaled_residual2`. `rms` returns `0.0` for `n == 0`. `rms_parts` accumulates all of `a`, then all of `b`, and divides by the summed length. Both residual forms use `alpha + max(abs(u0), abs(u1)) * rho` with left-to-right evaluation. |
| Sources verified | NUM-02 row in `06_actionable_task_list.md`; `01_ecosystem_architecture.md` section 6.2; existing port formulas named in the roadmap. |
| Tests executed | `numeric_residuals.valvet`; Python cross-check of every printed value; ASan/UBSan; full suite. |
| Issues found | `rms_parts` accepts unequal component lengths by design. `mean` of an empty vector prints `-nan`; `rms` avoids that path and returns `0.0`. |
| Completion evidence | Fixture prints `0`, `3.5355339059327378`, `3.5355339059327378`, `2.1602468994692869`, `0.66666666666666663`, `-0.59999999999999998`, `0.5714285714285714`, `0.40000000000000002`, `0.66666666666666663`, `0.8571428571428571`, `1`. Bits match the Python reference. |

### IO-01

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `crates/valvet_core/src/sema.rs`; `crates/valvet_core/src/codegen.rs`; `valvet-tests/compile-pass/write_no_newline.valvet`; `valvet-tests/compile-pass/write_no_newline.valvet.stdout`; `crates/valvet_core/tests/codegen_fp.rs` |
| Implementation summary | `write` shares the one-argument printable check with `println` but does not emit W1002. Codegen maps `write` to the existing `vfmt_*` helpers, which never append a newline. |
| Sources verified | IO-01 row in `06_actionable_task_list.md`; existing `vfmt_*` and `vprint_*` helpers in `crates/valvet_core/src/codegen.rs`. |
| Tests executed | `write_no_newline.valvet`; `valvet check` confirms no warning; generated-C test; full suite. |
| Issues found | `write` is intended for scalar and composite values but must not be used for the three-line oracle protocol without a following `println`. |
| Completion evidence | Output is `a5true1.5` followed by the final newline from `println("")`. Test `write_uses_formatters_without_newline` asserts `vfmt_*` and no `vprint_*` in `main`. |

### PERF-01

| Field | Value |
| --- | --- |
| Status | Complete for the repository fixture; BDF and weak binaries are external |
| Files changed | `crates/valvet_core/src/codegen.rs`; `crates/valvet_core/tests/codegen_fp.rs`; `docs/library_ecosystem_roadmap/03_performance_and_machine_code.md` |
| Implementation summary | `cc_flags` emits `-ffunction-sections`, `-fdata-sections`, and `-Wl,--gc-sections` for the `dev` profile only. Release and math profiles are unchanged. |
| Sources verified | PERF-01 row in `06_actionable_task_list.md`; local GCC behavior measured with `size`; output comparison. |
| Tests executed | `dev_profile_garbage_collects_unused_sections_only`; size measurement on `numeric_core`; byte comparison of program output. |
| Issues found | The roadmap asked for BDF and weak binaries, but those port artifacts are not in this checkout. The repository fixture demonstrates the mechanism without claiming port-specific ratios. |
| Completion evidence | Dev text fell from 53,572 to 14,504 bytes, a 72.9 percent drop. Both binaries printed identical output. |

### DX-01

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `crates/valvet_core/src/sema.rs`; `crates/valvet_core/tests/front_end.rs`; `valvet-tests/compile-pass/literal_context.valvet`; `valvet-tests/compile-pass/literal_context.valvet.stdout`; `valvet-tests/compile-fail/literal_narrowing.valvet`; `valvet-tests/compile-fail/literal_narrowing.valvet.error` |
| Implementation summary | Integer literals consult the expected type first. `Int64` and fitting `Int32` are selected directly. Exactly representable `Float64` literals are accepted. Out-of-range `Int32` literals keep the range default and fail at the call site. Plain assignment resolves the place before the value. Comparisons infer a literal operand from the other side. Index expressions clear the ambient expectation. |
| Sources verified | DX-01 row in `06_actionable_task_list.md`; existing literal and binary checking in `crates/valvet_core/src/sema.rs`. |
| Tests executed | `literal_context.valvet` covers argument, assignment, comparison, and loop conditions. `literal_narrowing.valvet` rejects `2147483648` for `Int32`. `front_end` test covers positive and negative cases. Full suite. |
| Issues found | The first implementation changed `w[0]` in a `Float64` assignment into a `Float64` index. Index checking now clears `expected`. Comparisons initially did not propagate a type; the binary path now does. `stdlib/math.valvet`, `stdlib/numeric.valvet`, and `stdlib/complex.valvet` casts were cleaned. Older modules such as `stdlib/frame.valvet`, `stdlib/infer.valvet`, and `stdlib/plot.valvet` now emit W1003 for redundant explicit `Int64` casts; the test suite does not treat warnings as errors, and those modules were left unchanged to keep this batch bounded. |
| Completion evidence | Fixture prints `6, 7, 2, 8, true, 3`. Negative fixture reports `E0201` at the argument. All 300 fixtures pass. |

### DX-02

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `crates/valvet_core/src/parser.rs`; `crates/valvet_core/tests/front_end.rs`; `valvet-tests/multi-file/dotted/main.valvet`; `valvet-tests/multi-file/dotted/main.valvet.stdout`; `valvet-tests/multi-file/dotted/math.valvet`; deleted `valvet-tests/multi-file/dotted/main.valvet.error`; `valvet-tests/multi-file/dotted-std/main.valvet`; `valvet-tests/multi-file/dotted-std/main.valvet.stdout` |
| Implementation summary | `parse_import` accepts `Tok::Dot` wherever `Tok::ColonColon` is accepted, including before brace lists. `import std.math::sqrt`, `import std::math::sqrt`, and `import std.math.sqrt` normalize to the same path. |
| Sources verified | DX-02 row in `06_actionable_task_list.md`; existing import parser and resolver. |
| Tests executed | `import_dotted_and_colon_paths_normalize` parser test; dotted multi-file run; mixed spelling plus alias run; full suite. |
| Issues found | The previous `E0602` test was replaced because the diagnostic is intentionally removed. A duplicate import alias resolves to the same function without double execution. |
| Completion evidence | `multi-file/dotted` prints `6`; `multi-file/dotted-std` prints `3` and `4` using `::`, `.`, and an alias in one file. |

### VER-01

| Field | Value |
| --- | --- |
| Status | Checker complete; port verification blocked on external oracles |
| Files changed | `scripts/compare_oracles.py`; `scripts/oracle_schema.json`; `scripts/README.md`; `scripts/tests/test_compare_oracles.py` |
| Implementation summary | Standard-library-only checker for the three-line label protocol. Compares binary64 values by bit pattern, supports `--one-ulp`, emits JSON summaries, and supports `--all` with an external manifest. Missing, extra, malformed, and differing labels fail the run. |
| Sources verified | VER-01 row in `06_actionable_task_list.md`; repository oracle labels in README; no oracle data was added to the repository. |
| Tests executed | `python3 -m unittest discover -s scripts/tests -v`: six tests pass, covering exact equality, one-ulp, missing/extra, malformed input, absent root, and manifest mode. |
| Issues found | Port artifacts are absent, so the checker cannot verify port 08 or port 10 on a clean checkout. The manifest and files must be supplied externally. |
| Completion evidence | Unit tests pass. The CLI exits 2 for a missing oracle root and exits 0 only when the manifest comparison is clean. |

## P1 tasks

### CPLX-01

| Field | Value |
| --- | --- |
| Status | Complete in-repository fixture; port migration external |
| Files changed | `valvet-tests/compile-pass/complex_pow.valvet`; `valvet-tests/compile-pass/complex_pow.valvet.stdout`; `stdlib/complex.valvet` cleanup only |
| Implementation summary | The existing `cnew`, `cpow_int`, complex add/sub/mul/div, and struct API were exercised. No API change was needed. Two needless `Int64` casts in `stdlib/complex.valvet` were removed after DX-01. |
| Sources verified | CPLX-01 row in `06_actionable_task_list.md`; `stdlib/complex.valvet`. |
| Tests executed | `complex_pow.valvet`; full suite; ASan/UBSan. |
| Issues found | Port 09 source is external, so only the library path is verified. |
| Completion evidence | Fixture prints powers of `1 + 2i`: `1, 0, 1, 2, true, -3, 4, -11, -2, 0.2, -0.4`. |

### LINALG-01

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `stdlib/linalg.valvet`; `valvet-tests/compile-pass/linalg_complex2.valvet`; `valvet-tests/compile-pass/linalg_complex2.valvet.stdout` |
| Implementation summary | Added `CMat2`, `CVec2`, and `solve2x2_complex` using `det = ad - bc`, `x = (d * rhs.x - b * rhs.y) / det`, and `y = (a * rhs.y - c * rhs.x) / det`. Complex products and differences are bound to temporaries so borrowed arguments have stable places. |
| Sources verified | LINALG-01 row in `06_actionable_task_list.md`; `stdlib/complex.valvet`; Cramer's rule. |
| Tests executed | Real 2x2 system and a complex diagonal system; full suite; ASan/UBSan. |
| Issues found | Nested temporary borrows are not accepted, so each intermediate is bound with `val`. Port 09 source is external. |
| Completion evidence | Fixture prints `-4, -0, 4.5, 0` for a real system and `1, -1, 1.5, 1.5` for a complex diagonal system. |

### ARR-03

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `crates/valvet_core/src/parser.rs`; `crates/valvet_core/src/sema.rs`; `crates/valvet_core/src/ownership.rs`; `crates/valvet_core/src/codegen.rs`; `crates/valvet_core/tests/front_end.rs`; `docs/implementation/ARCHITECTURE.md`; `valvet-tests/compile-pass/arrayd_basic.valvet` and `.stdout`; `arrayd_strings.valvet` and `.stdout`; `arrayd_index_oob.valvet` and `.exit`; `arrayd_rank_mismatch.valvet` and `.exit`; `arrayd_bad_shape.valvet` and `.exit`; `arrayd_axis_oob.valvet` and `.exit`; `valvet-tests/compile-fail/arrayd_dim_type.valvet`, `arrayd_from_vec_type.valvet`, `arrayd_index_arity.valvet`, `arrayd_unknown_method.valvet`, `arrayd_use_after_move.valvet`, and `arrayd_zeros_type.valvet` with `.error` sidecars |
| Implementation summary | Added `ArrayD<T>` as a runtime-rank, row-major, owning container. The C layout is `typedef struct { T *data; int64_t len; int64_t rank; int64_t shape[32]; int64_t strides[32]; } VArrayD_T;` with no VLA. `from_vec` steals the data buffer; `filled` and `zeros` allocate a new one. Shape validation checks rank 1 to 32, non-negative dimensions, product and stride overflow, and data length. Methods are `rank`, `len`, `dim`, `shape`, `strides`, `to_vec`, and deep `clone`. Indexing checks the runtime rank and each axis, then uses stored strides. `Vec<T>` stays 1D and `Mat` stays 2D. Views, slicing, `select`, stride-0 broadcasting, and borrowed array views are deferred. |
| Sources verified | ARR-03 row in `06_actionable_task_list.md`; MAT-01 index-list lowering; existing `Vec` layout and drop/clone conventions; `mdspan` row-major stride model. |
| Tests executed | `arrayd_basic` and `arrayd_strings`; four abort fixtures; six compile-fail fixtures; seven `arrayd_*` front-end tests in `crates/valvet_core/tests/front_end.rs`; full debug and release fixture suites; ASan/UBSan/LSan on the string and numeric fixtures; `cargo fmt`; `cargo clippy`. |
| Issues found | Invalid shapes and out-of-range access abort through `fprintf` plus `abort()` instead of returning `ShapeError`; this keeps the existing `Mat` and `Vec` trap behavior. `from_vec` takes ownership of the data vector, so use-after-move is `E0301`. A shape-only `ArrayD<T>` use still records `Vec<Int64>` for its metadata layout; `from_vec` and `to_vec` helpers are emitted only when `Vec<T>` is recorded. |
| Completion evidence | `arrayd_basic` prints `2, 6, 2, 3, 1, 6, 20, [2, 3], [3, 1], [1, 20, 3, 4, 5, 6], 6, 0, 5`. `arrayd_strings` prints owned `String` reads, writes, clone, and `to_vec` output `a, d, B, B, [a, B, c, d], a, x, x, y`. The rank-mismatch fixture aborts with `array index rank mismatch`; the bad-shape fixture aborts with `negative array dimension`; the axis fixture aborts with `array axis out of bounds`; the OOB fixture aborts with `index out of bounds`. Generated C contains the fixed `shape[32]`/`strides[32]` layout, `.shape[0]`, `.strides[0]`, and the runtime rank check. |

### ARR-04

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `crates/valvet_core/src/ast.rs`; `crates/valvet_core/src/parser.rs`; `crates/valvet_core/src/sema.rs`; `crates/valvet_core/src/ownership.rs`; `crates/valvet_core/src/codegen.rs`; `crates/valvet_core/tests/front_end.rs`; `docs/implementation/ARCHITECTURE.md`; `valvet-tests/compile-pass/array_fixed_basic.valvet` and `.stdout`; `array_fixed_strings.valvet` and `.stdout`; `array_fixed_shape.valvet` and `.exit`; `array_fixed_index_oob.valvet` and `.exit`; `array_fixed_rank8.valvet` and `.stdout`; `array_mat_coexist.valvet` and `.stdout`; `valvet-tests/compile-fail/array_fixed_arity.valvet`, `array_fixed_rank.valvet`, and `array_fixed_move.valvet` with `.error` sidecars |
| Implementation summary | Added `Array<T, N>` for compile-time ranks 1 to 8. The parser accepts `Array<T, N>` in type positions and `Array<T, N>::...` associated calls, encodes the rank in the internal base name (`Array3`), and rejects ranks outside the range. Sema checks indexing arity exactly. Codegen aliases the `ArrayD<T>` layout with a rank-specific typedef and emits rank-specific `from_vec`, `filled`, `zeros`, drop, and clone wrappers. Index lowering uses the stored shape and strides without a runtime rank check. Constructors still take a runtime `Vec<Int64>` shape; the wrappers trap with `array shape rank mismatch` when its length is not `N`. `rank()` returns the compile-time rank as an `Int64` constant. `Vec<T>` remains 1D and `Mat` remains 2D. |
| Sources verified | ARR-04 row in `06_actionable_task_list.md`; ARR-03 runtime layout and kernels; the type and associated-call parsing patterns used by `Vec<T>` and `ArrayD<T>`. |
| Tests executed | `array_fixed_basic`, `array_fixed_strings`, `array_fixed_rank8`, and `array_mat_coexist`; the shape and out-of-bounds abort fixtures; three compile-fail fixtures; six `array_fixed_*` front-end tests in `crates/valvet_core/tests/front_end.rs`; full debug and release fixture suites; ASan/UBSan/LSan on the numeric and owned-element fixtures; `cargo fmt`; `cargo clippy`. |
| Issues found | The reused `ArrayD` layout still stores `rank` and fixed `shape[32]`/`strides[32]`, so `Array<T, N>` does not have a distinct fixed-rank ABI; it is a compile-time rank tag over the runtime layout. Shape dimensions and the shape vector length are runtime values, so the length is validated at construction rather than in sema. The parser recovers from an out-of-range rank with rank 1 so the front end reports one `E0201` instead of cascading parse errors. No user-level `Mat`/array conversion or borrowed view was added; `Mat` remains a separate 2D `Float64` type, and the coexistence bridge is the shared `Vec<Float64>` runtime verified by `array_mat_coexist`. |
| Completion evidence | `array_fixed_basic` prints `2, 6, 2, 3, 1, 6, 20, [2, 3], [3, 1], [1, 20, 3, 4, 5, 6], 6, 0, 5`. `array_fixed_strings` prints `a, d, B, B, [a, B, c, d], x, x, y`. `array_fixed_rank8` prints `8, 1, 0, 7`. `array_mat_coexist` prints `1, 4, 6`, confirming that `Mat`, `Array<Float64, 2>`, and `ArrayD<Float64>` share the runtime without symbol conflicts. Generated C contains `typedef VArrayD_Float64 VArray2_Float64;`, `varrayn_Array2_Float64_from_vec`, the rank-specific drop/clone wrappers, and no `array index rank mismatch` for `Array<T, N>` indexing. The shape fixture aborts with `array shape rank mismatch`; the OOB fixture aborts with `index out of bounds`; arity, rank, and move fixtures report `E0201`, `E0201`, and `E0301`. |

### MAT-01

| Field | Value |
| --- | --- |
| Status | Complete |
| Files changed | `crates/valvet_core/src/ast.rs`; `crates/valvet_core/src/parser.rs`; `crates/valvet_core/src/resolver.rs`; `crates/valvet_core/src/sema.rs`; `crates/valvet_core/src/ownership.rs`; `crates/valvet_core/src/codegen.rs`; `crates/valvet_core/tests/front_end.rs`; `docs/implementation/ARCHITECTURE.md`; `valvet-tests/compile-pass/mat_get_set.valvet`; `valvet-tests/compile-pass/mat_get_set.valvet.stdout`; `valvet-tests/compile-pass/mat_index_oob.valvet`; `valvet-tests/compile-pass/mat_index_oob.valvet.exit`; `valvet-tests/compile-fail/mat_index_arity.valvet`; `valvet-tests/compile-fail/mat_index_arity.valvet.error`; `valvet-tests/compile-fail/mat_index_type.valvet`; `valvet-tests/compile-fail/mat_index_type.valvet.error`; `valvet-tests/compile-fail/vec_index_arity.valvet`; `valvet-tests/compile-fail/vec_index_arity.valvet.error` |
| Implementation summary | `ExprKind::Index` and `Place::Index` now carry an index list. The parser accepts `m[i, j]`, the resolver rewrites every index, and sema checks `Mat` arity 2 and integer index types through one helper shared by reads and writes. Codegen lowers the read and the store to the same row-major slot `data[i * cols + j]` with row then column bounds checks. Existing `Vec` indexing keeps arity 1. A `Mat` instantiation now also records the `Vec<Float64>` layout referenced by the Mat kernel block, so a Mat-only program links. No `ShapeError` layout change was needed. |
| Sources verified | MAT-01 row in `06_actionable_task_list.md`; existing `Mat` layout in `crates/valvet_core/src/codegen.rs`; existing `Vec` index bounds behavior. |
| Tests executed | `mat_get_set.valvet`; `mat_index_oob.valvet`; three compile-fail fixtures; `mat_multi_index_read_write_accepted_and_lowered`, `mat_only_program_records_vec_float64_layout`, `mat_multi_index_arity_rejected`, `mat_multi_index_type_rejected`, and `vec_multi_index_arity_rejected` in `crates/valvet_core/tests/front_end.rs`; full debug and release fixture suites; ASan/UBSan with leak detection on `mat_get_set`; `cargo fmt`; `cargo clippy`. |
| Issues found | An out-of-range matrix index aborts with the existing `index out of bounds` trap, matching `Vec`; it does not return a recoverable error. This keeps `ShapeError` and all existing add/sub/mul kernels unchanged. A pre-existing Mat-only link failure was exposed by the new fixture: the Mat kernel block referenced `VVec_Float64` without recording its layout. Recording `Vec<Float64>` with every `Mat` instantiation fixed it. |
| Completion evidence | The fixture prints `1`, `4`, `[[1, 20], [30, 4]]`, `50`, `[[1, 20], [30, 50]]`, and `7` after reads and writes at `[0, 0]`, `[1, 1]`, `[0, 1]`, and `[1, 0]`, including a write through a `struct` field base. Generated C contains `.rows`, `.cols`, and `data[i * cols + j]` for both read and store. The out-of-bounds fixture exits by `abort()` and arity/type fixtures report `E0201`. |

### NUM-03

| Field | Value |
| --- | --- |
| Status | Partial |
| Files changed | `stdlib/numeric.valvet`; `valvet-tests/compile-pass/numeric_reductions.valvet`; `valvet-tests/compile-pass/numeric_reductions.valvet.stdout` |
| Implementation summary | Added `add`, `sub`, `sum`, `mean`, `min_vec`, `max_vec`, `sum_sq`, and `sd` with exact-order loops. The private helpers in `stdlib/model.valvet`, `stdlib/plot.valvet`, and `stdlib/infer.valvet` were not removed or rewired. |
| Sources verified | NUM-03 row in `06_actionable_task_list.md`; existing `fsd`, `fmin`, `fmax`, `fmean`, `vec_add`, `vec_sub`, `vec_mean`, and `sum_sq` bodies. |
| Tests executed | `numeric_reductions.valvet`; Python cross-check; full suite. |
| Issues found | Most existing call sites pass owned temporaries. The new API takes `&Vec<Float64>`, and borrowing a temporary is not accepted. Rewiring requires binding every intermediate or changing the API, so the consolidation was not forced. `std.numeric::min_vec` and `max_vec` return NaN for empty input, while the existing plot helpers document a non-empty precondition. |
| Completion evidence | New functions produce `sum=10`, `mean=2.5`, `min=1`, `max=4`, `sum_sq=6`, `sd=1.2909944487358056`, and correct `add`/`sub` values. The migration itself remains open. |

## Validation snapshot

Commands run on the final tree:

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `cargo test --workspace`: pass. Includes 6 codegen FP tests, 218 front-end tests, 2 fuzz tests, and the existing CLI and pipeline suites.
- `target/debug/valvet test valvet-tests`: 340 passed, 0 failed.
- `target/debug/valvet test valvet-tests --profile release`: 340 passed, 0 failed.
- `target/debug/valvet test --doc`: 8 passed, 0 failed.
- ASan/UBSan with leak detection on `math_scalar`, `numeric_core`, `numeric_residuals`, `numeric_reductions`, `linalg_complex2`, `vec_owned_ops`, `complex_pow`, `mat_get_set`, `arrayd_basic`, `arrayd_strings`, `array_fixed_basic`, `array_fixed_strings`, `array_fixed_rank8`, `array_mat_coexist`, `string_index_basic`, `string_index_group`, `string_index_growth`, `string_index_owned`, `string_index_move`, `string_index_discard`, and `string_index_containers`: output matched each golden file.
- `python3 -m unittest discover -s scripts/tests -v`: 6 passed.
- `python3 scripts/sqrt_sweep.py --count 1000000 --batch 10000 --valvet target/release/valvet --json /tmp/opencode/sqrt_sweep.json`: 1,000,000 checked, 0 mismatches. Summary stored in `evidence/sqrt_sweep_1m.json`.

## External blockers

- Port sources and oracle files are outside the repository. Port 08, port 09, port 10, BDF, and weak migration checks cannot run here.
- FND-04 needs tuple returns from ARR-02 or a different return representation.
- ODE-01, STOCH-01, NUM-04, and later P2 tasks need port sources or exact formulas that are not in this checkout.
