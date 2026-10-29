# Test Strategy

## Rust tests

`cargo test --workspace` covers the lexer, parser, sema, ownership, codegen
shapes, CLI security policy, and the no-panic fuzz-soup property. Current
totals: 223 front-end, 9 codegen, 6 core unit, 5 CLI security, and 3 fuzz
tests, for 246 tests.

`cargo fmt --all --check` and
`cargo clippy --workspace --all-targets -- -D warnings` are release gates.
Lints are not weakened and failures are not deleted.

## Fixture suites

Fixtures under `valvet-tests/` use `*.valvet` plus `.stdout`, `.exit`, or
`.error` sidecars. `valvet test` builds and runs compile-pass fixtures,
checks expected E-codes for compile-fail fixtures, and verifies runtime
output and exit status. Multi-file projects and conformance cases run
through the same CLI path. The generated C is compiled with release flags
by default, including when the driver itself is `target/debug/valvet`.

Current fixture total: 432 passed, 0 failed. The typed-frame, binary-format,
and lazy-plan coverage is concentrated in:

- `typed_frame_*`, `typed_csv_*`, `typed_json_*`, and `schema_*`
- `typed_group_*`, `typed_join_*`, and `typed_window_*`
- `parquet_*`, `arrow_*`, `bytes_*`, and `snappy_*`
- `lazy_plan_*`
- `while_cond_owned_temp` for owned temporaries in loop conditions

`valvet test --doc` compiles the README fences and `examples/` programs.
Current total: 17 passed, 0 failed.

## Binary fixtures

`scripts/make_parquet_fixtures.py` and `scripts/make_arrow_fixtures.py`
generate the checked-in binary fixtures using only the Python standard
library. `valvet-tests/data/README.md` records provenance, byte sizes, and
SHA256 values. Re-running the generators should leave the tree unchanged.

The malformed fixtures are deliberate:

- `parquet_truncated` and `parquet_bad_offset`
- `parquet_nested` and `parquet_dictionary`
- `arrow_truncated`, `arrow_nested`, `arrow_dictionary`, and
  `arrow_unsupported`
- `arrow_multiple_batches` for the one-batch reader limit

They must return `Err` or a controlled diagnostic, never crash or hang.

`scripts/fuzz_binary_formats.py` mutates every binary fixture in a fixed
order, runs one prebuilt probe against each case, and fails on a signal,
timeout, or non-zero exit. A clean `Err` and a successful parse are both
acceptable. Current smoke total: 266 cases. This is deterministic mutation
smoke, not coverage-guided fuzzing.

## Sanitizers

Sanitizer runs use a GCC wrapper:

```sh
#!/bin/sh
exec /path/to/gcc -fsanitize=address,undefined \
  -fno-omit-frame-pointer -g -O1 "$@"
```

Then:

```sh
VALVET_CC=/tmp/opencode/vgcc-asan \
ASAN_OPTIONS=detect_leaks=1:halt_on_error=1 \
UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1 \
target/debug/valvet run valvet-tests/compile-pass/typed_frame_basic.valvet
```

Delete `target/valvet/<stem>` and `target/valvet/<stem>-*.hash` before a
forced rebuild. The linker cache can otherwise reuse a non-sanitized
executable when only `VALVET_CC` changes.

Current result: ASan/UBSan/LSan clean on 88 typed-frame, grouped, join,
window, Parquet, Arrow, bytes, Snappy, and lazy-plan fixtures, with stdout
identical to each `.stdout` sidecar.

## Release and documentation gates

The fixture suite is also run through the release CLI binary:

```sh
cargo build --release --workspace
target/release/valvet test valvet-tests --jobs 8
```

No commit, tag, push, or publish is part of the test gate. Binary fixture
generation and sanitizer runs are local and non-destructive; malformed
inputs are copied under `/tmp/opencode` before mutation.
