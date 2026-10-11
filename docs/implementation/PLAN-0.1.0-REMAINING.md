# Plan — Completing 0.1.0 (11 remaining items)

Basis: `valvet-design/` D2–D5/D7/D9 + `revision/01` (all 26 VR resolved, no open contradictions in scope);
external references: Rust Fuzz Book (`cargo fuzz init`, fuzz-target pattern), NLL design
(smallcultfollowing “liveness and location”, RFC 2094 liveness rules), C tagged-union lowering
(Cyclone manual §4 pattern), multi-file module practice (“Env per module/file”, langdev).
Verified against current source: `Break`/`Continue`/`DotDot` tokens do NOT exist; `Match`/`For`/`Loop`
lex but rejected (`parser.rs:130`, `782`, `786`); `?`/`.` rejected in postfix; no generics anywhere.

Dependency order: A → B → C → D → E → F → G → H → I (fuzz runs alongside from A).
Each item lists exact file changes, spec refs, tests, and done-criteria.

## Phase A — Loops, casts, fuzz harness (independent; start first)
**STATUS: DONE 2026-10-10** (27+2 unit tests, 25/25 fixtures debug+release, ASan clean, clippy/fmt clean)

## Phase B — Structs & enums: DONE 2026-10-10
Shipped: nominal types (`Ty::Named` resolved in place), struct/enum decls (named/tuple/unit;
struct-variants E0800), struct literals (colon syntax), tuple-struct calls, `Type::Variant`
construction, `.name`/`.0` field access, field-unknown/missing/duplicate diagnostics,
recursion rejection (no indirection yet), per-field move tracking (conservative whole-use),
structural Copy analysis, C layouts + drop/clone helpers, deep poison-on-move.
Gates: 36+2 unit tests, 34/34 fixtures debug+release, ASan clean incl. struct moves.

## Phase C — Match/if-let: DONE 2026-10-10
Shipped: patterns (wild/lit/bind/qualified+bare variants/struct/or/guards), `if let`
desugared to match, statement + value matches, exhaustiveness E0204 (bools/enums/else),
arm agreement, or-binding agreement, conservative scrutinee moves on owned bindings,
sequential-if lowering (else-chains mis-handle guard-false — found by testing),
no-struct-literal rule in conditions (Rust's rule; exception-safe flag handling —
fixed a real parser panic found during the suite run), unit-variant optional parens.
Gates: 44+2 unit tests, 41/41 fixtures debug+release, ASan clean incl. guards.

## Phase D — Option/Result: DONE 2026-10-10
Shipped: `Ty::Generic` + generic-arg parsing, Option/Result resolution with
instantiation registry (poor-man's monomorphization; `Vec` E0800), `None` /
`Some` / `Ok` / `Err` constructors with annotation/context inference (never
guessed: bare `None`/`Ok`/`Err` without context are E0201), qualified
`Option::`/`Result::` construction, bare+qualified variant patterns with
coverage, `?.field` (None-shortcircuit, deep-clone into fresh result),
per-instantiation C layouts + drop/clone helpers, deep poison-on-move,
expected-type threading for ctor inference, tag-dispatched destruction.
Also fixed along the way: constructor node types now recorded (was ICE),
`None` literal emission (was `v_None`), resolved decl types in the type table
(codegen read raw annotations), `?.` payload wrapper level, condition-temp
use-after-free in `if`/`while` (temps freed before headers ran), abandoned
temps on early `return` (pop-all), unregistered value-construct results.
Gates: 52+2 unit tests, 49/49 fixtures debug+release, ASan 21/21 clean.

## Phase E — `?` operator: DONE 2026-10-10
Shipped: postfix `?` parsing, T-Prop (Result-requires-Result with identical
error types — `From` conversions deferred; Option-requires-Option; success
type unconstrained; everything else E0702), `main` returning
`Result<Int32, E>`/`Option<Int32>` via an exit-code wrapper (Ok(n)/Some(n)
-> n; Err(e) -> drop + 1), early-return drops on the failure path
(abandoned statement temps popped, scope frees run, error payload moved
into the return slot), owned/copy payload handling on both paths.
Also fixed along the way: `if`/`while` condition temps freed before headers
ran (use-after-free in every owned conditional).
Gates: 59+2 unit tests, 55/55 fixtures debug+release, ASan 24/24 clean.

## Phase F — Vec/vec!: DONE 2026-10-10
Shipped: `vec![...]` (homogeneous, empty needs annotation), `Vec<T>` builtin
generic (never Copy — owns its heap buffer), methods len/push/pop/get/clone/
cast/add_scalar/add/mul/dot (+`for x in vec` snapshot iteration), builtin
`ShapeError{expected,found}` for kernel shape errors, VVec runtime per
instantiation (growth, OOB-None, deep drops/clones), forward declarations
for C23, gated kernel generation (only referenced layouts exist).
Design: pointer mutation + Unit `push` (`v.push(x);`); all other methods
borrow (get/for clone out — no buffer aliases can exist yet, documented,
must be revisited when G adds references); `v = v.push(x)` is E0201;
non-closure methods only (map/filter/mask wait for closures/slices).
Also fixed along the way: multi-use scrutinee double evaluation
(`match (v.pop())` popped per arm — non-Var scrutinees/bases materialize
once), nominal decl types in the type table, Unit initializers rejected.
Gates: 68+2 unit tests, 61/61 fixtures debug+release, ASan 27/27 clean.

### A1. `for`/`loop`/`break`/`continue`
- Spec: D2 §4, D5 (block/tail, separator rules). `for (x in lo..hi|lo..=hi)` over `Int32`/`Int64`
  only in 0.1.0 (general iterators deferred — Vec has no iterator protocol yet); `break v`
  only as `loop` tail (D5), plain `break`/`continue` elsewhere, misuse outside loops → E0005.
- `token.rs`: add `Break, Continue, DotDot, DotDotEq`; extend `keyword()` (`break/continue`);
  extend `symbol()` two-char match (`..`, `..=` — careful: `..` vs float `1..2` handled by
  existing number-lookahead which requires a digit after `.`).
- `ast.rs`: `Stmt::{For{id,var,var_span,lo,hi,inclusive,body:Box<Block>}, Loop{id,body}, Break{id,value:Option<Expr>}, Continue{id}}`
  (keeps `Box<Block>` discipline); `ExprKind` unchanged.
- `parser.rs`: new `parse_for/parse_loop` + `Break/Continue` arms in `parse_stmt`;
  remove `For/Loop` from the E0800 reject list (~line 130); range parsing in a new
  `parse_range` layer between `parse_cmp` and `parse_add` (D5 precedence: `..` binds looser
  than `+`, tighter than comparisons — put it there, not in `parse_cmp`).
- `sema.rs`: range endpoints must be equal int types (E0201); loop var is immutable `Int`
  in a pushed scope; new `loop_depth: u32` field on `Cx`, `break/continue` with depth 0 → E0005;
  `break v` type must match enclosing `loop`-as-tail type; `while`/`for` bodies Unit (reuse
  diverging-block rule).
- `codegen.rs`: `for` → C `for (int32_t x = lo; x < hi; ++x)` in a fresh C scope per D4 Drop
  rules; `loop` → existing `while(1)` emitter; `break v` → assign temp + break (mirror
  `emit_tail_return` temp discipline).
- Tests: `valvet-tests/compile-pass/for_range.valvet` (+`.stdout`), `loop_break_value`,
  `compile-fail/break_outside_loop.valvet` (E0005), `for_float_bounds` (E0201); unit:
  `parser_for_range_forms` in `tests/front_end.rs`.

### A2. `as` casts (+ `try_as` declaration only — impl with D/Option)
- Spec: D3 §1 table (int→int wrap-truncate; float→int saturate incl. NaN→0, ±inf→MIN/MAX;
  int→float round-nearest). No implicit conversions (existing E0201 stays).
- `parser.rs`: new `parse_cast` layer between `parse_unary` and `parse_postfix`
  (D5: `as` binds tighter than `*`); remove `As` from deferred handling.
- `sema.rs`: `ExprKind::Cast{expr, target}` typing per table; disallowed pairs
  (Bool/String involving) → E0201; `try_as` parsed as ordinary call, rejected E0800 until D.
- `codegen.rs`: plain C casts are WRONG for float→int (C UB on overflow — UBSan would fire):
  emit saturating helpers in `PRELUDE` (`v_f2i32/v_f2i64` with `isfinite` + range checks),
  int→int via explicit mask/truncate, int→float direct.
- Tests: `cast_table.valvet` runtime (wrap/saturate/NaN cases), `cast_bool_rejected` (E0201);
  ASan run mandatory (this item exists because of UB risk).

### A3. Fuzz harness (stable-compatible)
- External ref: Rust Fuzz Book pattern (`fuzz_target!(|data| { parse(data) })`), but `cargo-fuzz`
  needs nightly+LLVM+libFuzzer — NONE available here (stable 1.99, user GCC). So:
- New `crates/valvet_core/tests/fuzz_soup.rs`: seeded PRNG token-soup + structural mutation
  of every `valvet-tests/compile-pass/*.valvet`; assert: never a compiler panic/hang
  (`catch_unwind`, per-case size cap 64KB, nesting cap from robustness probes); diagnostics
  of any code are acceptable outcomes. Runs in `cargo test`, no new deps.
- `TEST_STRATEGY.md`: record full `cargo-fuzz` libFuzzer setup as follow-up when nightly
  toolchain available; `ci.yml`: add `cargo test --test fuzz_soup` line (already covered by
  `cargo test --workspace`, just document the seed corpus dir `valvet-tests/fuzz-corpus/`).

## Phase B — Structs & enums (foundation for C/D/E/F)

- Spec: D2 §5 (no `S: Trait` syntax — `#[derive]` only per VR-009), D4 §1 partial moves,
  D4 Drop order (fields reverse-decl).
- `parser.rs`: `parse_struct/parse_enum` replacing E0800 arms; struct literal
  `Point { x: 1.0 }` (colons — NOT `=`), tuple-struct form, `#[derive(...)]` attribute
  skip-with-record (derive codegen handwritten per type, no macro system);
  `.` field access in `parse_postfix` (replace E0800); `::` variant paths in patterns/calls.
- `ast.rs`: `Item::{Struct,Enum}` + `Program` gains `adts` table (or new `sema`-owned table —
  choose sema-owned `structs: HashMap<String,StructDef>, enums: ...` to avoid AST churn);
  `Ty::{Struct(String), Enum(String, Vec<TyArgs>)}` — NO general generics: type args allowed
  ONLY for builtin `Option/Result/Vec` (poor-man's monomorphization, documented stepping stone).
- `sema.rs`: struct-literal field check (unknown field E0401, missing field E0201, wrong type E0201);
  field-access typing; enum variant arity check.
- `ownership.rs`: struct owns fields; field-level moved sets (`moved: HashSet<String>` per
  binding extended to `moved_fields: HashMap<String,HashSet<String>>`); partial-move use
  rejected unless remaining fields `Copy` (D4); `clone()` per Clone contract.
- `codegen.rs`: C `struct` emission; field Drop in reverse declaration order at scope end;
  moves NULL whole struct slot (field-granular NULLing deferred — whole-slot is sound).
- Tests: `struct_basic`, `struct_move_partial_fail` (E0301), `enum_declcheckout` (decl only;
  patterns in C).

## Phase C — `match` (+ guards, exhaustiveness, `if let`)

- Spec: D2 §4, D5 (patterns: `_`, lit, binding, `Variant(pats)`, `a | b`, guards `if`),
  E0204 non-exhaustive-as-value. External ref: C lowering via tag test + `if/else` chains
  (switch only for uninhabited-int scrutinees — keep chains uniformly, no jump tables in 0.1.0).
- `parser.rs`: `parse_match` (replace E0800), `parse_pat` (needs `_` wildcard: currently lexed
  as `Ident("_")` — special-case in pattern position); `if let` as `if` variant in
  `parse_stmt` + `parse_primary` (VR-025: `if (Some(v) = x)` stays E0005).
- `ast.rs`: `Pat::{Wild,Lit(Lit),Bind(String),Variant{path,sub:Vec<Pat>},Or(Vec<Pat>)}`,
  `MatchArm{pat,guard:Option<Expr>,body:Block}`, `ExprKind::Match{scrutinee:Box<Expr>,arms}`.
- `sema.rs`: arm reachability (unreachable arm → warning lint, new `W1001` — first warning code);
  exhaustiveness over Bool (2 arms), int ranges (must have `_`), enums (all variants or `_`)
  else E0204 when used as value; binding types from payload types; guard must be Bool.
- `ownership.rs`: scrutinee borrow during arms; bindings in arms are fresh moved/borrowed slots
  per payload ownership; branch-merge rule reused (conservative union).
- `codegen.rs`: `if/else` chain on tag + payload extraction into arm-scope temps (frame-temps,
  freed per arm — reuses Phase-0 temp discipline); String payloads moved (NULL source variant
  slot — variant slots are temps, sound).
- Tests: `match_int_bool`, `match_guard`, `match_non_exhaustive` (E0204), `if_let_some_none`,
  `match_string_payload_move` (+ ASan).

## Phase D — `Option`/`Result` (needs B)

- Spec: D3 §7 (NOT keywords — ordinary std identifiers per VR-005), `?.` sugar (already specced,
  implement now), `unwrap_or/map` methods as known-fns.
- No-generics constraint → implement as **builtin generic ADTs**: `sema.rs` new `known_def`
  table recognizing `Option<T>/Result<T,E>` for concrete primitive+String+struct payloads;
  each instantiation recorded in `Checked::instantiations`; `Some/None/Ok/Err` resolve as
  variant ctors (value position) AND patterns (pattern position) — disambiguate by position,
  no grammar change needed.
- `codegen.rs`: per-instantiation tagged-union C structs (`Opt_Int32{tag; payload}`, …) +
  payload Drop elaboration (String payloads freed when owner freed — extends temp/scope-free
  logic with a `drop_ty` helper); `?.` lowers to tag-test + temp (reuse match-arm machinery).
- Tests: `option_some_none`, `result_ok_err`, `opt_string_drop` (+ ASan — payload Drop is the
  risk), `question_mark_deferred_still_E0800` (until E).

## Phase E — `?` operator (needs D)

- Spec: D3 §2 T-Prop. Simplification (record in SPECIFICATION_GAPS.md): 0.1.0 requires
  IDENTICAL error types (`E: Into<E2>` conversions deferred with generics/traits).
  `?` on `Option` only in `Option`-returning fns; `?` in `main` only if `main` returns
  `Result` (extend `main` signature check in `sema.rs`).
- `parser.rs`: postfix `?` in `parse_postfix` (replace E0800 arm) → `ExprKind::Try(Box<Expr>)`.
- `sema.rs`: operand must be `Result<T,E>`/`Option<T>`; enclosing fn return must match;
  else E0702 (already in D9 table).
- `codegen.rs`: `VRes` temp → tag test → on `Err`: run scope frees + frame-temp frees
  (reuse `emit_all_frees` + `pop_frame`, same as `return` paths) → `return Err_temp`;
  on `Ok`: unwrap to temp. Mirrors existing early-return discipline — no new free logic.
- Tests: `try_propagates` (runtime exit + stderr), `try_wrong_context` (E0702),
  `try_runs_drops` (String payload freed on propagation — ASan).

## Phase F — `Vec` + `vec!` (needs B for `Vec<String>` Drop)

- Spec: D7 §3 (VR-011/025): equal-lengths-or-`Err(ShapeMismatch)`, NO length-1 auto-broadcast,
  scalar broadcast only via named methods. **Scope cut** (record in D13): 0.1.0 implements
  non-closure methods only — `new/push/pop/len/get/slice/clone/cast/add_scalar/mul/add/dot`;
  `map/filter` (closure-taking) deferred to 0.3.0 with closures; manual `for`-loop iteration
  (Phase A) is the 0.1.0 pattern. Rationale: closures don't exist yet; no silent behavior change.
- `parser.rs`: `vec![...]` in `parse_primary` → `ExprKind::VecMacro(Vec<Expr>)`
  (the sole builtin macro per VR-004; unify element type or E0201; `vec![]` needs annotation).
- `sema.rs`: `Vec<T>` builtin constructor (same instantiation table as D); method call syntax
  `v.push(x)` — requires `.` method dispatch: implement minimal method table
  (`METHODS: &[(&str, Sig)]` per instantiation) rather than full trait resolution (deferred).
- `ownership.rs`: `Vec<String>` owns elements; `get` borrows (tied to vec lifetime —
  rejected if vec moved while borrow live, E0302 path reuse); `clone()` deep for documented
  types.
- `codegen.rs` + `PRELUDE`: per-instantiation `Vec_T{data,len,cap}` + `vvec_T_*` runtime
  (grow-amortized, OOB → controlled abort like existing div-zero path); element Drop loop
  for `String` instantiations only.
- Tests: `vec_push_get_len`, `vec_shape_mismatch` (`Err` at runtime), `vec_string_drop` (ASan),
  `vec_macro_mixed_types` (E0201), `vec_map_deferred_E0800`.

## Phase G — References & NLL borrowck (needs B; biggest risk)
**STATUS: DONE 2026-10-11** (80+2 unit tests, 76/76 fixtures debug+release, ASan/UBSan clean on all 8 borrow pass fixtures, clippy/fmt clean)
Shipped: `&T`/`&mut T`/`&'a T` types (top-level only; nested/generic-arg/struct-field refs are E0201),
`&expr`/`&mut expr` borrows, `*` deref (clone-out for owned values — no buffer aliases),
place targets (`x`, `a.b`, `*r`, `**r`, `m.y` through refs with `->` lowering), lifetime elision
(single-input passthrough ok, ambiguous/undeclared E0303, elided unifies at calls),
AST-level NLL (loans to holder last-use; temporaries live through the call statement;
field-precise overlap; reborrow freeze; call-result region sharing; loop-escape extension),
dangling rejection (`return` + body-tail, E0303), `&mut`-needs-`var` (E0205), move-while-borrowed (E0301),
exclusivity (E0302), missing-borrow call hint, bracket-aware binary lookahead (D5: `*m` at line
start is dereference, not `*` continuation — fixed a real misparse found during the suite run).
Gates: 80+2 unit tests, 76/76 fixtures debug+release, ASan/UBSan 8/8 borrow clean.

- Spec: D4 §2 + D5 reference/lifetime syntax. External refs: RFC 2094 + “liveness and location”
 (GET the core idea: borrow ends at last use, derived from liveness on the CFG).
- `lexer.rs`: lifetime tokens — `'` currently starts char literals: disambiguate `'a` (lifetime:
  `'` + ident NOT followed by `'`) vs `'a'` (char, stays E0800 in 0.1.0 — chars deferred).
  New `Tok::Lifetime(String)`.
- `parser.rs`: `&T/&mut T/&'a T` types; `&expr/&mut expr` borrow expressions in `parse_unary`
  (replace E0800 arm); `*` deref already lexed — add deref expr.
- `sema.rs`: lifetime elision rules (D4: `fn first(v: &[Int32]): &Int32` ok; stored/ambiguous
  refs require explicit `'a` else E0303); explicit-lifetime well-formedness.
- `ownership.rs`: replace scope-end borrow model with **last-use regions on the AST**
  (documented simplification vs full MIR: compute per-borrow live range to last use;
  conservative at loop back-edges — reuse existing loop conservatism). Enforce: shared XOR
  exclusive (E0302), no use-after-free on move of borrowed value, no escaping refs
  (returning local borrow → E0303). Function-param reborrowing per elision.
- `codegen.rs`: references erase to C pointers; validity already proven by borrowck — no
  runtime checks (state this explicitly; it is the design's core claim).
- Tests: `borrow_shared_ok`, `borrow_mut_excl` (E0302), `borrow_end_nll` (use-after-last-use
  allowed — THE NLL test), `borrow_escape` (E0303), `borrow_in_struct` (needs explicit `'a`).

## Phase H — Multi-file modules/imports
**STATUS: DONE 2026-10-11** (92+2 unit tests, 86/86 fixtures debug+release, ASan/UBSan clean on all 3 multi-file pass projects, clippy/fmt clean)
Shipped: `module a.b;` (dots, mandatory in imported files, ignored in entry), `import` single/brace/`as` forms (`::` only; dotted E0602, glob/re-export E0800), `pub` default-private (non-pub cross-module use E0401), file→module mapping from entry dir, cycle E0601, missing/mismatched E0701, per-file diagnostics (`.file` attribution, entry-anchored spans), joint sema with module-qualified names + import aliases ("Env per module"), C TU with `v_mod___fun` + sanitized types + fn prototypes (also fixes single-file forward calls, broken before), `val x: ImportedType` annotations, cross-module enums/match/structs/borrows/Vec, `valvet new` scaffolding, `multi-file/` fixture dirs in `valvet test`.
Also fixed along the way: per-file NodeId collisions in the joint table (resolver renumbers), Variant result + match-coverage qualification, pattern lowering resolution.
Gates: 92+2 unit tests, 86/86 fixtures debug+release, ASan/UBSan 3/3 multi-file clean.

- Spec: D9 §1 + VR-002 (dots in `module` decl only, `::` everywhere else; E0601 cycles,
  E0602 dotted imports, E0701 unresolved). External ref: “Env per module” resolver pattern.
- New `crates/valvet_core/src/resolver.rs`: `CrateGraph{modules: HashMap<ModulePath, Unit>}`;
  file→module mapping from `src/` root (`a/b.valvet` ↔ `module a.b;`); `import` loading with
  cycle detection; `pub`/private visibility filter on cross-module lookup.
- `pipeline.rs`: `front_end_file` → parse main file → resolve imports recursively → joint
  `sema::check` over all units (shared `fun_names`/sigs); errors carry per-file `Source`.
- `sema.rs`: two-phase (register all sigs incl. cross-module `pub` fns, then check bodies).
- `codegen.rs`: mangle with module path (`v_mod__fun`); single C TU output (no separate
  compilation in 0.1.0 — state it).
- `main.rs`: `build/run/check` accept project files; `test` discovers multi-file fixtures;
  new `valvet new` scaffolding (deferred to 0.2.0? NO — D9 CLI lists it; implement minimal
  `new` here: writes `valvet.toml`-less single-dir project — manifest format deferred).
- Tests: `tests/` multi-file fixtures (`a.valvet` + `b.valvet` + expected), `import_cycle`
  (E0601), `dotted_import` (E0602), `private_leak` (E0401 across modules).

## Phase I — `println` stdlib, retire `print`
**STATUS: DONE 2026-10-11** (98+2 unit tests, 90/90 fixtures debug+release, ASan/UBSan clean incl. nested composites, clippy/fmt clean)
Shipped: `println(x)` builtin (builtin dispatch per DD-29: primitives + `Vec`/`Option`/`Result` of printable types, recursive nesting — `[1, 2]`, `Some(7)`/`None`, `Ok(1)`/`Err(e)`); `print` deprecated alias (W1002 warning channel: `Diagnostic::warning`, `Checked::warnings`+`files`, stderr on success); `vfmt_`/`vprint_` per-instantiation helpers (gated on printed types); printable-structural sema check (structs/enums/refs stay E0201 until stdlib `Display`); all 81 call sites migrated to `println`.
Also fixed along the way (all latent, all verified by execution): nested-`Vec` inner layouts never recorded (`record_vec_inst` now delegates to `record_ty`); C layout emission is topological over ADTs+instantiations (fixes `Vec<Option<T>>`, struct-held `Vec`, forward order); struct/enum drop/clone now handle `Generic` fields/payloads (were silently shallow); ctor/literal/tuple-struct payload temps poisoned on move (UAF); `v_drop_`/`v_clone_` names sanitize qualifiers; recursive types through generics rejected (documented unsized rule); single-constructor `?.` deep-clones generic payloads. Pre-existing bugs left alone (statement-`match` with `print()` tails; `vec![None]` needs annotation).
Gates: 98+2 unit tests, 90/90 fixtures debug+release, ASan/UBSan clean (full pass sweep + multi-file).

- Depends on H only if stdlib ships as Valvet sources — CHOSEN: builtin dispatch (same
  mechanism as today's `print`, extended per-type C helpers incl. `Vec`/`Option`/`Result`
  formatting), so H is NOT a prerequisite; stdlib-as-source comes in 0.2.0 with the package
  system. (Record alternative + reason in D13.)
- `sema.rs`: rename known-fn `print` → `println`, keep `print` as deprecated alias emitting
  `W1002` (uses the new warning channel from C); update ALL fixtures + examples
  (`sed` + `valvet test` green gate).
- `codegen.rs`: `vprint_*` extended: `vprint_vec_T`, `vprint_opt_T` (`Some(v)`/`None`),
  `vprint_res_T` (`Ok(v)`/`Err(e)`); dispatch in the existing print arm.
- Tests: update every `.stdout` fixture (mechanical), `println_all_types` runtime,
  `print_deprecated_warns` (W1002, still compiles).

## Cross-cutting (every phase)

- `SPECIFICATION_GAPS.md`: resolve one gap per phase (E-subcodes already assigned stay).
- `COMPILER_PLAN.md` table: flip `Specified` → `Implemented`/`Tested` per phase; never mark
  tested without the listed tests green.
- `revision/` style decision entries for the two scope cuts (Vec non-closure subset; builtin
  println over stdlib-source) + NLL-on-AST simplification.
- Gates per phase (same as 0.1.0): `fmt --check`, `clippy -D warnings`, `cargo test`,
  `valvet test` debug+release, ASan/UBSan over new string-owning fixtures.
