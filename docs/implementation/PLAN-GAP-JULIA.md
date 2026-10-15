# PLAN-GAP-JULIA: Julia gaps

Goal: take the Julia features that fit a static first-order language:
arity overloads, broadcast sugar over the existing named-function `map`,
a stdlib Complex stepping stone, Unicode-aware string verbs, and
Time/Duration in the existing civil-algorithm style. Macros, full
multidispatch, parametric types, and FFI stay deferred with named unlocks.

Conventions: `S(x)` = `crates/valvet_core/src/sema.rs`, `C(x)` = codegen.rs,
`P(x)` = parser.rs. Anchors are 0.3.0-era; grep quoted strings on drift.

Batch order: J2 (broadcast sugar) -> J4 (unicode) -> J5 (time) -> J3
(stdlib Complex) -> J1 (arity overloads, largest). J6 documents deferrals.

---

## J2: broadcast sugar `f.(v)` over named-function `map` (parser only)

Gap: `map` requires writing `map(v, f)` (verified: function operand is
`args[1]` via `fun_value_sig`, `S:~5089` region); Julia's `f.(v)` reads
better in pipelines. No closures involved — `f` stays a named item.

### Exact changes

1. `P`, `parse_postfix` Dot arm (the `.name(` method path; other branch
   errors E0005 `"expected field name after `.`"`): when the token after
   `.` is `LParen` (not an ident), parse it as broadcast:
   - Receiver expression `e` must be a plain `Var(f)` (named function);
     anything else → E0005 ("broadcast needs a named function, `f.(v)`").
   - Parse call args `(a, b, ...)` with the existing call-arg loop; v1
     accepts exactly 1 argument (the vec), else E0005 (multi-arg
     broadcast waits for zip semantics — deferred explicitly).
   - Build `ExprKind::Call { name: "map", args: [v, Var(f)] }` (note the
     order: vec first, function second, matching `map`'s operand rule).
   - Spans cover `f.(v)` so `map`'s arity/type errors point at user code
     (same desugar discipline as `|>` and `??`).
2. No sema/ownership/codegen changes (`map` path handles checking, moves,
   and kernels; UFCS never sees this node since it is a real `Call`).
3. Fixtures: `double.(v)` agrees with `map(v, double)` bit-for-bit;
   `f.(v)` with unknown `f` → `map`'s unknown-function E0401 (lock it);
   non-`Var` base (`(g(h)).(v)`, method-call base) → E0005.
4. EBNF: extend the postfix/call rule with the broadcast alternative.

Risks: `f . (v)` with spaces — tokens are space-insensitive, so this also
parses as broadcast; acceptable (matches call syntax). Interaction with
tuple-field access `pair.0`: unchanged (IntLit arm untouched).

---

## J4: Unicode-aware string verbs (builtins + C helpers)

Gap: strings are documented byte-oriented; `emit_str_method` (`C:2059`)
lowers `trim/clone/len/byte_at/slice/split` onto byte-indexed `vstr_*`
helpers (`C:1966-1971` pattern). R/Julia count and case-fold by character.

### Exact changes

1. `S`, new string-method arms next to the existing ones (find the
   `check_str_method` dispatch by grepping `"split"` in `S`): all take
   `&String`-ish receivers like their byte twins —
   - `nchars() -> Int64`: Unicode scalar count (not bytes).
   - `to_upper() / to_lower() -> String`: full-string case fold. v1 scope:
     ASCII + Latin-1 fast path, correct scalar counting for the rest;
     full Unicode tables explicitly deferred (document; Turkish dotted-I
     and friends stay unhandled).
   - `is_alpha() -> Bool`? Cut unless wanted — keep the batch to the
     three above.
   - Method list messages (the `"available on String: ..."` E0401 strings,
     e.g. the UFCS probe text) gain the new names.
2. `C`, `emit_str_method` new arms emitting `vstr_nchars(r)`,
   `vstr_to_upper(r)`, `vstr_to_lower(r)`; C helpers next to the `vstr_*`
   block: UTF-8 lead-byte decoder (1–4 byte, overlong/invalid → U+FFFD,
   never abort — strings stay total), Latin-1 case tables.
3. Ownership: string methods borrow (existing rule) — no changes.
4. Fixtures: ASCII + multi-byte (2/3/4-byte) + invalid-byte cases with a
   Python-oracle (`str.upper/lower/len`) agreement file; byte verbs keep
   their fixtures untouched.
5. EBNF: none (methods, like all verbs).

Risks: case folding that changes byte length (e.g. `ß` → `SS`) must
reallocate exactly (mirror `vstr_concat`'s measure-then-fill discipline);
ASan/UBSan over the decoder with fuzz-adjacent fixtures (invalid
continuations, truncated tails).

---

## J5: Time and Duration, Hinnant-style (stdlib only)

Gap: `stdlib/date.valvet` has civil-date algorithms (`days_from_civil`,
`date_from_ymd`, `date_add_days`, `date_diff_days`, `datetime_*` with
`Datetime { nanos }`, `date.valvet:12-13,47-224`) but no time-of-day or
duration arithmetic. Zones explicitly out.

### Exact changes

1. `stdlib/date.valvet` (or new `stdlib/time.valvet` if the file grows
   past review comfort — prefer new file, `module std.time;`, importing
   nothing):
   - `pub struct Time { nanos: Int64 }` (since midnight), `pub struct
     Duration { nanos: Int64 }`, mirroring `Date { days }`.
   - `time_from_hms`, `time_to_string`/`time_parse` (same `Option` +
     bounds-check style as `date_parse`), `time_add/duration` ops as
     named funs (`time_plus(t, d)`, `time_minus`, `duration_add/sub`,
     `duration_neg`), `duration_from_hms/secs`, `duration_as_secs`
     (Float64), comparisons returning Int64 like `date_compare`.
   - Datetime bridge: `datetime_time(t: Datetime) -> Time`,
     `datetime_date(t: Datetime) -> Date` (modulo 86_400e9 with Euclidean
     semantics for negative nanos — document; civil algorithms already in
     file handle the date half).
   - Pure Valvet, Int64 only (no floats except `duration_as_secs`;
     Float64 paths use the existing `as` casts).
2. Fixtures vs Python `datetime` oracle (incl. leap-second-adjacent and
   negative-modulo cases; zones never asserted).
3. No core changes.

Risks: none structural. Keep the `0.0/0.0`-NaN idiom out (times are total;
use `Option` like the date parsers).

---

## J3: Complex as a stdlib stepping stone (stdlib only now, builtin later)

Gap: no complex scalar; spectral/FFT-adjacent work (next to the
distributions track) wants it. A new `Ty` variant ripples every Ty match
(sema/codegen/ownership/units), so v1 is pure stdlib.

### Exact changes

1. New `stdlib/complex.valvet`, `module std.complex;`:
   - `pub struct Complex { re: Float64, im: Float64 }` plus named funs
     (no operator overloading exists — the pipe is the honest chaining
     form): `cnew/cadd/csub/cmul/cdiv/cneg/conj/cabs/carg`,
     `cscale`, `cexp` (Euler), `cpow_int` (binary exponentiation).
   - Division guards zero modulus with NaN results (file's convention).
2. Fixtures vs Python `cmath` oracle (multiply/divide/exp/abs/arg at
   1e-12 relative; branch cut of `arg` asserted explicitly).
3. No core changes. Documented follow-up (not this batch): builtin
   `Complex` scalar over C99 `_Complex` when spectral kernels land —
   that change touches `Ty`, all three matches, and printing; say so
   plainly in the file header so nobody starts it casually.

Risks: ergonomics (named funs are clunky) — accepted as the honest price
of no operator overloading; UFCS helps (`x.cadd(y)` works on method-less
struct types — verify during implementation, else plain calls).

---

## J1: arity overloads (sema + resolver + codegen; largest item)

Gap: duplicate `fun` names are E0401 (registration near `S:188-197`
`fun_names.insert`). Julia shows dispatch is the feature; full type
dispatch needs runtime tags or call-site monomorphization (deferred).
Arity overloads are the tractable first step and already cover most
real uses (defaults-by-arity, scalar/vec variants).

### Exact changes

1. `S` registration (~`S:188-205`): key `fun_names`/`sigs` by
   `(qname, arity)` instead of `qname`. Duplicate (name, arity) stays
   E0401 with the current message; same name new arity is now legal.
   - `sigs: HashMap<NodeId, ...>` is already per-decl id — unchanged.
   - `resolve_fun(name)` (`S:668`) gains arity: `resolve_fun_arity(name,
     arity) -> Option<NodeId>`; keep `resolve_fun` for the call sites
     that don't know arity yet, then update each (below).
2. Call-site updates (inventory from grep — verify each during
   implementation):
   - `S:6294` user-call path: resolve with `args.len()`; unknown-arity →
     E0201 naming the known arities ("`f` takes 1 or 3 arguments, found
     2" — the existing `arity.valvet` fixture pins E0201 for this case;
     E0401 stays for truly unknown names).
   - UFCS arm (`S:~2886`, `resolve_fun(method)`): resolve with
     `call_args.len() + 1`.
   - `S:2277`, `S:2988`, `S:3397`, `S:6281`, `S:6285` (ctor guards,
     `Some`/`None` checks): these ask "does a user fn shadow this
     builtin/ctor name" — any-arity existence is the right question;
     keep them on a name-only lookup (new `fun_name_exists` helper).
   - `fun_value_sig` (`S:~5089`, used by `map`/`par_map`): resolve with
     the expected arity from context (map needs 1-arg `f(Vec-elem)`...
     read each caller: the sig check already validates params, so
     resolve-then-validate works if resolution tries arities in order —
     simplest: resolve by the arity the caller is about to check; where
     the caller doesn't know it (functionals know: unary), pass it in.
   - Ownership `resolve_fun` mirror + codegen `resolve_fun` (`C:851`):
     same arity threading (codegen's is used for dims/symbols at call
     lowering and `functional_sym`).
3. C names: `cfun(module, name)` (`C:6103`) must differ per arity or
   overloads collide in C. Change to `cfun(module, name, arity)` with
   suffix `_aN` (triple-underscore discipline preserved: `v_mod___f_a2`;
   verify no collision with the `__`-escaping rule during
   implementation). Update all `cfun` callers (`C:903,909,3016,3564,
   3570,4631` + `emit_proto`/`emit_fun` paths — grep `cfun(` exhaustively
   at implementation time). `mangle` (locals) untouched.
4. Diagnostics: ambiguous zero-arity-vs... no ambiguity exists (arity
   decides); document that type-based overloading is still E0401-duplicate
   (same name+arity twice) and remains a later stage.
5. Fixtures: same-name 1/2/3-arg overloads incl. cross-module, UFCS
   through overloads, `map` with overloaded fn (resolves unary),
   unknown-arity error message fixture, recursion per-arity.
6. EBNF: none (call syntax unchanged).

Risks: biggest blast radius in this file set (every `resolve_fun`/`cfun`
caller). Mitigate by landing sema+tests first with codegen following in
the same batch (never split: an unresolved C symbol is a link error, and
fixture `build` would catch it — still, keep it atomic). `main` excluded
from overloading (entry rule; keep the existing guard).

---

## J6: deliberate non-goals (no work items, unlock conditions noted)

- Macros/hygiene/metaprogramming, generated functions: need a compile-time
  evaluation story; revisit after closures, if ever (Valvet favors explicit
  desugars today: `??`, comprehensions, `|>`).
- Full multiple dispatch (type-based methods): needs runtime tags or
  call-site monomorphization; J1 arity step does not preclude it.
- Parametric types/generics: wait for monomorphization design (note in
  F4/F5 — same story).
- Green threads/distributed: `par_sum`/`par_map` cover the data case.
- Package registry: single-file plus `valvet new` suffice; revisit on
  demand.
- `ccall` user FFI: grammar already reserves `Extern`/`Unsafe` tokens;
  needs the ownership story for borrowed returns first — designed but
  deferred.
- Shell integration, kwargs/varargs/splatting: `sys_run` exists (Batch D);
  kwargs conflict with strict-arity diagnostics — deferred with prejudice.

---

## Gates (every batch)

fmt, clippy `-D warnings`, unit tests, fixtures debug+release, ASan/UBSan
(incl. decoder fuzz-adjacent cases for J4 and threaded paths for any
`par_*`), zero `<error>`. Numerical items (J3, J5) carry Python-oracle
agreement at 1e-12 relative. Commit per batch; push only on your word.
