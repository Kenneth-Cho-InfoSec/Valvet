# Test Strategy

- Rust unit tests: lexer tokens, parser AST, sema errors, ownership rejections, codegen snippets (`cargo test --workspace`).
- Fixture suites under `valvet-tests/`: `lexer/ parser/ typecheck/ ownership/ codegen/ runtime/ diagnostics/ compile-pass/ compile-fail/ multi-file/ conformance/ regression/` — each fixture `*.valvet` (+`.stdout`/`.exit`/`.error` sidecars). Harness: `valvet test` runs compile-pass (build+run+verify), compile-fail (expect E-code), runtime (output/exit).
- End-to-end: CLI-driven native executables only (no internal-only testing for acceptance).
- MVP acceptance: 7 tests from master prompt (§12) in `valvet-tests/`.
- Quality: `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, release build. No weakening lints / deleting failures.
- Fuzzing: `fuzz_soup` harness shipped (token soup + fixture mutations, no-panic property; 2 tests in `cargo test`).
