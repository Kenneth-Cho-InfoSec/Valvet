# Specification Gaps (audited 2026-10-10)

Source: `valvet-design/grammar/valvet.ebnf` + D2/D3/D4/D5/D9 + `revision/01-contradiction-register.md` (all 26 VR items already resolved by repair pass — no open contradictions affecting MVP).

## Gaps requiring implementation decisions
1. **Val-reassignment code**: D9 table has no dedicated code. Adopt **E0205** `assignment to immutable binding`. Routine; no designer approval needed.
2. **Unknown identifier code**: use **E0401** (D9 family E04xx name resolution). Routine.
3. **`print` builtin**: not in stdlib spec (which has `println` via std). ~~MVP temporary: `print(x)` for the 5 primitive types. Documented deferred → `std::io::println`. Observable behavior (stdout text) identical.~~ **RESOLVED 2026-10-11 (Phase I)**: `println(x)` is the 0.1.0 output builtin (builtin dispatch: primitives + `Vec`/`Option`/`Result` of printable types, nested; `[1, 2]`, `Some(7)`/`None`, `Ok(1)`/`Err(e)` formats); `print` remains as a deprecated alias emitting **W1002** (warning channel: `Diagnostic::warning`, `Checked::warnings`, stderr on success). Full stdlib-source `println` (with `Display`) comes in 0.2.0 with the package system.
4. **String equality**: MVP supports `==`/`!=` on String via runtime strcmp. Spec implies PartialEq for String; consistent.
5. **`main` signatures**: accept `fun main()` / `-> Int32` / `-> Result<..>` rejected in MVP (Result deferred) with E0800-style "not yet supported". Return value = exit code when Int32. **RESOLVED (Phase E + wrapper-drop fix)**: `main(): Int32` returns the exit code; `main(): Result<Int32, E>` / `Option<Int32>` map `Ok(n)`/`Some(n)` to exit n, failure to drop + exit 1 (verified exits 43 and 42; all-Copy payloads covered by `try_main_option_copy`/`try_main_result_copy`).
6. **Integer literal range**: E0203 on annotated overflow; unconstrained default Int32 (D3).
7. **Missing `;` recovery**: sync to next newline/brace (D5 §5g). Implemented.

## Deferred without gaps
when/defer/generics/closures/unsafe/FFI/frames — EBNF productions exist; rejected with controlled diagnostics (E0800 at item level; `defer` in statement position reports E0005 with position — same no-silent-acceptance guarantee). `match`/`for`/`loop`/lifetimes shipped in 0.1.0 (struck from this list).
