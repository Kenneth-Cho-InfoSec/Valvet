# PLAN-GAP-FSHARP: F# gaps

Goal: take the F# features that fit a first-order, no-closure language, and
name exactly what stays deferred until closure/generics stages. Full CEs,
async workflows, quotations, SRTP/generics need Stage-2 closures or
monomorphization — not here.

Conventions: `S(x)` = `crates/valvet_core/src/sema.rs`, `C(x)` = codegen.rs,
`P(x)` = parser.rs. Anchors are 0.3.0-era; grep quoted strings on drift.

Batch order: F2 (comprehensions) -> F1 (par_map) -> F3 (docs idiom).
F4/F5 are explicit non-goals with unlock conditions.

---

## F2: list comprehensions without closures (parser desugar)

Gap: no comprehension syntax; `map` needs a named `fun` even for inline
expressions. But comprehensions over first-order expressions desugar to
plain loops — no closure required.

### Exact changes

1. `P`, new primary arm (verified: `parse_primary` has NO `LBracket` arm;
   `[` today appears only in postfix index, `P:2702`, so the anchor is free):
   - On `Tok::LBracket`, speculatively parse `[ e for x in v ]` / `[ e for
     x in v if c ]`: after `[`, parse `e` via `parse_expr`; if next is the
     `for` keyword (contextual check like `mode`: `Ident("for")`? No — `for`
     is a real keyword token `Tok::For`; match it directly), consume and
     parse `x` (ident), expect `in`, parse `v` (use `parse_nostruct_expr`
     to match `for` headers), optional `if c` (`parse_nostruct_expr`),
     expect `]`.
   - If the shape does not match (no `for` after `e`), emit today's error
     (whatever `[` in primary position produces now — lock current behavior
     with a fixture first: likely E0005; keep it byte-identical).
   - Desugar to `ExprKind::Block` (Batch-1 infrastructure: sema
     `check_block_expr`, codegen inline `emit_stmt`, ownership scoped walk
     all exist):
     `{ var $t_out: Vec<T> = vec![]; for (x in v) { if c { $t_out.push(e) } }; $t_out }`
     with `$t` names from the existing `tmp_name()` hygienic counter
     (`$` is not an ident char).
   - Element type problem: `push` needs a known `Vec<T>`. Same rule as
     `vec![]` (`S`, "`vec![]` needs an expected `Vec<T>` type"): take `T`
     from the ambient `self.expected` when it is `Vec<T>`; else E0201
     pointing at the comprehension ("annotate the binding"). No inference
     engine, no new rules.
   - `for`/`if` desugared nodes reuse `ForIter::Each`, `Stmt::If`,
     `ExprKind::Method{push}` shapes exactly as the parser builds them for
     written code (spans: cover the whole comprehension; inner spans reuse
     the parsed sub-spans so errors point inside).
2. Ownership/sema/codegen: no changes (all nodes pre-existing; verify the
   gate suite, especially move-through-`$t_out` — `push` moves owned
   elements in, mirroring written loops).
3. Fixtures: `[x * (2 as Int64) for x in v]`, filtered form, nested
   (`for` over the result of another comprehension), annotated vs
   unannotated (E0201 case), Python-oracle value agreement.
4. EBNF: extend the expression rule with a comprehension alternative.

Risks: `for` header parsing inside `[...]` vs `..` ranges and `in` — reuse
the exact `Tok::For` arm code path by refactoring it into a helper both
sites call (do not duplicate). `if` without `else` in statement position
is already Unit-typed (existing rule), so the filter form needs no else.

---

## F1: `par_map` over named functions (sema + codegen builtin)

Gap: `par_sum` (`S:6036`, `C:2259-2320` runtime + `C:4518` call site) sums;
`map` (`S` `check_functional`, function operand is `args[1]` via
`fun_value_sig`) is serial. `par_map(v, f, nthreads)` needs no closures (surface order matches `map`):
`f` is a named item like in `map`.

### Exact changes

1. `S`, new `par_map` arm after `par_sum` (`S:6036` pattern):
   - Arity exactly 3, else E0201 (same message shape).
   - `args[0]`: `Vec<Float64>` (v1 scope: Float64 in/out like `par_sum`;
     document widening to Int64/Bool later), `record_vec_inst(&Float64)`
     on success (mirror `S:6057-6060`).
   - `args[1]`: `fun_value_sig(&args[1], "par_map")` (same helper `map`
     uses); require signature `(Float64) -> Float64`, else E0201 naming
     expected vs found (read the `(ps, r)` pair exactly as the `map` arm
     does — grep the arm below the `fun_value_sig(&args[1], "map")` call).
   - `args[2]`: Int thread count (same Int32|Int64 rule as `par_sum`).
   - Returns `Vec<Float64>`; record the instantiation.
2. `C`:
   - Runtime next to the par helpers (`C:2259-2320`): `v_par_map_wrap`
     with the SAME fixed-4096-block, index-ordered-combine discipline and
     the same `VParSumTask`-shaped task struct (new struct, same fields +
     `double (*f)(double)` + output array). Each block applies `f` per
     element; combine is concatenation in index order (no reduction, so
     bit-identity across thread counts holds trivially; still assert it in
     fixtures across 1/2/8 threads).
   - Named-function symbol: `functional_sym` (`C:~3014`, used by
     `emit_functional`) resolves the item to its C symbol — reuse it for
     the per-element call (verify its signature during implementation).
   - Call site next to `par_sum` (`C:4518` pattern): borrow the vec
     (`mat_str`, `cur_move = false`), thread count Copy, emit
     `v_par_map_wrap(&v, f_sym, n)`.
3. Ownership: `map`'s Call arm marks args via `moves_ty`; `par_map` args
   are (borrow vec, fn item, Copy int) — extend the `borrow_only` name list
   if that list exists at the ownership Call arm, else no change (verify:
   grep `split_apply` in ownership.rs — the Call arm names borrow-only
   builtins; add `par_map` there... careful: the ELEMENT function borrows
   each element; the vec itself borrows — correct).
4. Fixtures: agree with serial `map(f, v)` bit-for-bit on 1/2/8 threads;
   arity/type E0201 fixtures; ASan/UBSan on the threaded path (mirror K4
   gates).
5. EBNF: none (builtin, like `par_sum`).

Risks: `f` with side effects (e.g. `push` to a global — none exist; still,
   document purity expectation; threads share nothing since elements are
   Copy doubles). pthread failure fallback mirrors `par_sum` (serial
   fallback path in the same runtime).

---

## F3: active-pattern idiom (docs + fixtures, no compiler change)

Gap: no banana-clip syntax; but `MatchArm` already has `guard`, and
recognizers are ordinary named `fun`s returning `Option`.

### Exact changes

1. Docs (README or docs tour): the idiom —
   `match (s) { _ if is_int(s) -> ..., _ -> ... }` plus total-partition
   helpers returning `Option` payloads matched with `Some(x)` arms.
   Show the regex-date example from the F# docs translated to Valvet's
   `std::re` verbs.
2. Fixtures (compile-pass): 2–3 ports of the canonical active-pattern
   examples (balanced classification, regex date parse) using guards +
   helpers, verified against F# semantics by reading (no F# toolchain in
   CI; mark as behavior-reviewed, not differential-tested).
3. No core changes. Full `(|Name|_|)` syntax explicitly deferred to a
   pattern-position extension (needs parser pattern forms + sema
   `check_pat` arms — fine to add later, not now).

---

## F4/F5: deliberate non-goals (no work items, unlock conditions noted)

- Computation expressions (async/seq/query builders), async/task
  workflows, quotations: need Stage-2 capturing closures. The query half is
  covered by frame verbs over `ColExpr`; async's data half by `par_sum` /
  `par_map`. Revisit after closures.
- Generics/SRTP, quotation-based metaprogramming, OOP interop: no CLR,
  no objects by design; generics wait for monomorphization (fits the C
  backend: `cfun`/`mangle` gain per-instantiation suffixes, `generic_insts`
  already keys them — see `C:6095-6115`).
- Units of measure, DUs, Option/Result, pipelines, exhaustive match: HAVE
  (units additionally convert scales, beyond F#).

---

## Gates (every batch)

fmt, clippy `-D warnings`, unit tests, fixtures debug+release, ASan/UBSan
(incl. threaded paths), zero `<error>`. `par_map` fixtures assert
cross-thread-count bit-identity. Commit per batch; push only on your word.
