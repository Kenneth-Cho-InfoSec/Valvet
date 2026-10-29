# StringIndex specification

Status: implemented in this working tree. See `09_completion_status.md`
COL-03 for files, tests, issues, and evidence. This document remains the
implementation plan and exact-change record. No commit, tag, push, or publish
has been made.

Baseline: `d90e067` plus the uncommitted Stage A/B/C array work and the local
0.5.0 version bump. Do not commit, tag, push, or publish as part of this
specification.

## 1. Decision

Implement one built-in container named `StringIndex`.

`StringIndex` is an ordered `String -> Int64` index. It stores each distinct
string key once, in first-appearance order, and returns the zero-based position
of that key. Callers keep the actual values in parallel vectors.

This is the container recommended by the dictionary/map audit. It is not a
general `Map<K,V>`.

In scope:

- `String` keys only.
- Implicit `Int64` positions.
- First-appearance iteration order through `keys()`.
- `get`, `get_or_insert`, `contains`, `len`, `keys`, and `clone`.
- Hash-backed average O(1) lookup and insertion.
- Deep clone and deterministic drop.
- Use as a struct field, enum payload, `Option` payload, `Vec` element,
  `ArrayD` element, and function parameter or return value.

Out of scope:

- A general `Map<K,V>` or `Dict<K,V>`.
- Integer, Float64, Bool, struct, or enum keys.
- Deletion or tombstones.
- Map literals.
- `idx[key]` indexing syntax.
- Borrowed key views.
- A multimap or bucket type for joins.
- Caching an index inside `DynamicFrame`.
- `println` formatting for `StringIndex` itself.
- Any change to arithmetic order, allocation behavior, or the existing
  `Vec<T>`, `Mat`, `ArrayD<T>`, or `Array<T, N>` layouts.

## 2. Public surface

### 2.1 Type

```valvet
val idx = StringIndex::new()
```

`StringIndex` is a built-in, zero-arity, move-only type. It is a built-in type
name and cannot be redefined by a user struct or enum.

### 2.2 Constructor

```valvet
StringIndex::new(): StringIndex
```

`new` takes no type arguments and no value arguments. It returns an empty
index with no allocations.

### 2.3 Methods

| Method | Signature | Semantics |
| --- | --- | --- |
| `len` | `idx.len(): Int64` | Number of distinct keys. |
| `contains` | `idx.contains(key: String): Bool` | True when the key exists. |
| `get` | `idx.get(key: String): Option<Int64>` | `Some(position)` or `None`. |
| `get_or_insert` | `idx.get_or_insert(key: String): Int64` | Existing position, or append the key and return its new position. |
| `keys` | `idx.keys(): Vec<String>` | Fresh snapshot in first-appearance order. Keys are cloned. |
| `clone` | `idx.clone(): StringIndex` | Deep copy of keys and hash table. |

The receiver is borrowed by every method, matching the existing pointer model
for `Vec`, `Mat`, `Factor`, and arrays. `get_or_insert` mutates the receiver
through that borrow, like `Vec::push`. A `val` binding can therefore call
`get_or_insert`; the binding itself is not reassigned.

Method receivers do not auto-deref, matching `Vec` and the other built-ins. A
reference parameter (`&StringIndex` or `&mut StringIndex`) must call
`(*idx).len()` or `(*idx).get_or_insert(k)`. A plain `StringIndex` receiver is
borrowed by the call.

Key arguments are borrowed. `get_or_insert` clones the key only when it
inserts. This keeps `idx.get_or_insert(keys[i])` from moving a string out of a
vector.

### 2.4 Example

```valvet
val idx = StringIndex::new()
println(idx.len())                 // 0
println(idx.get("east"))           // None
println(idx.get_or_insert("east")) // 0
println(idx.get_or_insert("west")) // 1
println(idx.get_or_insert("east")) // 0
println(idx.contains("west"))      // true
println(idx.get("west"))           // Some(1)
println(idx.keys())                // [east, west]
```

Parallel values:

```valvet
val keys = vec!["east", "west", "east", "north"]
val values = vec![10, 20, 10, 40]
val idx = StringIndex::new()
var sums: Vec<Int64> = vec![]
for (i in 0..keys.len()) {
    val at = idx.get_or_insert(keys[i])
    if (at == (sums.len() as Int64)) {
        sums.push(values[i])
    } else {
        sums[at] = sums[at] + values[i]
    }
}
println(idx.keys())  // [east, west, north]
println(sums)        // [20, 20, 40]
```

### 2.5 Rejected forms

- `idx["east"]` is rejected with `E0401`. Use `.get` or `.get_or_insert`.
- `println(idx)` is rejected with `E0201`. Print `.keys()` or `.len()`.
- `StringIndex::new(1)` is rejected with `E0201`.
- `StringIndex<Int64>::new()` is not a supported constructor form.
- A key of any type other than `String` is rejected with `E0201`.

## 3. Semantics

### 3.1 Key equality

Keys compare with the existing `vstr_eq` bytewise equality. A null `VStr` and
an empty `VStr` are equal, matching `String` equality elsewhere. No locale,
Unicode normalization, case folding, or numeric coercion is applied.

### 3.2 Order

The keys array is append-only. A new key is appended at the end. The hash table
never determines iteration order. `keys()` walks the keys array from index `0`
to `len - 1`.

Duplicate `get_or_insert` calls return the original position and do not append
another key.

### 3.3 Positions

Positions are `Int64` values in `0..len`. They are stable for the lifetime of
the index because deletion is not supported.

Indexing a `Vec` or `ArrayD` element clones the element whenever the element
is owned. Therefore `v[i].get_or_insert(k)` and `v[i].idx.get_or_insert(k)`
mutate a clone, not the stored element. Mutate a local and assign it back, or
keep the index in a local variable or struct field. This matches the existing
ownership behavior for every owned element type.

### 3.4 Complexity

Let `n` be the number of distinct keys and `L` the key length.

- `len`, `contains`, `get`, and `get_or_insert`: average O(L), worst O(n * L)
  with linear probing.
- `keys`: O(n * L) because every key is cloned.
- `clone`: O(n * L).
- drop: O(n * L) for freeing keys plus O(n) for the arrays.

Do not claim a measured speedup for any stdlib path until that path is migrated
and measured. The hash table removes the repeated linear key scan, but fixture
output and allocation behavior must be verified independently.

### 3.5 Allocation and failure

The index uses checked growth. Allocation failure and size overflow abort with
the existing runtime messages:

- `alloc error`
- `allocation size overflow`

No new error code is introduced. These are runtime traps, not diagnostics.

### 3.6 Determinism

The hash table uses the existing FNV-1a helper `v_hash_str`. It is an internal
implementation detail. Output never depends on bucket order.

## 4. Internal representation

The generated C layout is:

```c
typedef struct {
    VStr *keys;
    uint64_t *hashes;
    int64_t len;
    int64_t cap;
    int64_t *buckets;
    int64_t bucket_cap;
} VStringIndex_;
```

Fields:

- `keys`: owned `VStr` values in first-appearance order.
- `hashes`: cached FNV-1a hash for each key, parallel to `keys`.
- `len`: number of keys.
- `cap`: capacity of `keys` and `hashes`.
- `buckets`: open-addressed table. `0` means empty; `i + 1` means entry `i`.
- `bucket_cap`: power-of-two bucket count.

The table keeps a load factor of at most 0.5. It is created with at least 8
buckets. `get_or_insert` doubles the bucket count when
`(len + 1) * 2 > bucket_cap`.

The layout is private to codegen. Changing the hash function, bucket count, or
probing scheme does not change the language surface.

## 5. Runtime helpers

Codegen emits these static helpers for every program that records a
`StringIndex` instantiation. Names are fixed and must not collide with user
symbols. User functions are mangled with a `v_` prefix, doubled underscores,
and an arity suffix, so none of these fixed names can collide. The `vsi_`
prefix is reserved for this container.

```c
static VStringIndex_ vsi_new(void);
static int64_t vsi_len(VStringIndex_ *v);
static int vsi_contains(VStringIndex_ *v, VStr key);
static VOption_Int64 vsi_get(VStringIndex_ *v, VStr key);
static int64_t vsi_get_or_insert(VStringIndex_ *v, VStr key);
static VVec_String vsi_keys(VStringIndex_ *v);
static void v_drop_StringIndex_(VStringIndex_ *v);
static VStringIndex_ v_clone_StringIndex_(VStringIndex_ v);
```

Implementation requirements:

1. `vsi_new` returns a zeroed struct.
2. `vsi_rehash` normalizes `ncap` to at least 8 and a power of two, doubles
   until `ncap > len`, allocates a zeroed bucket array, reinserts every key
   using its cached hash, frees the old bucket array, and stores the new
   capacity.
3. `vsi_reserve` takes a total required capacity, not an additional amount.
   It grows `keys` and `hashes` together with overflow checks.
4. `vsi_find_hashed` returns `-1` when `bucket_cap == 0`. Otherwise it probes
   linearly with `(hash & (bucket_cap - 1))`, compares the cached hash before
   calling `vstr_eq`, and stops at the first empty bucket.
5. `vsi_get_or_insert` computes the hash once and calls `vsi_find_hashed`.
   If the key exists, it returns that position without allocating. If the key
   is absent, let `position` be the current `len`. It grows the bucket table
   when `bucket_cap == 0` or `(len + 1) * 2 > bucket_cap`, reserves `keys` and
   `hashes` for `len + 1`, clones the key with `vstr_clone`, appends it at
   `position`, sets `len` to `position + 1`, then recomputes the first empty
   probe slot in the current bucket array and stores `position + 1`. It
   returns `position`. It must not reuse a bucket slot computed before a
   rehash.
6. `vsi_get` returns `(VOption_Int64){ .tag = 1 }` for absence and
   `(VOption_Int64){ .tag = 0, .u = { .v0 = { .v___0 = at } } }` for presence.
7. `vsi_keys` creates a fresh `VVec_String` with
   `vvec_Vec_String_with_capacity(v->len)`, then clones each key with
   `vvec_Vec_String_push`.
8. `v_drop_StringIndex_` tolerates null pointers. It frees each key when
   `keys` is non-null, then frees `keys`, `hashes`, and `buckets`, sets all
   three pointers to `NULL`, and resets `len`, `cap`, and `bucket_cap` to zero.
9. `v_clone_StringIndex_` returns `vsi_new()` when `len == 0`. Otherwise it
   chooses a power-of-two `bucket_cap >= 8` with `bucket_cap >= 2 * len`,
   allocates a fresh keys/hashes array, clones each key, and copies each cached
   hash. A simple implementation sets `len` and `cap` on the new struct, then
   calls `vsi_rehash(&copy, bucket_cap)` to build the bucket array, and returns
   the copy.
10. Every allocation path checks `SIZE_MAX / sizeof(...)` and integer growth
    before adding, multiplying, or doubling. A count that cannot be represented
    aborts with `allocation size overflow`.

The helpers are emitted only once per program. They use `v_hash_str`,
`vstr_clone`, `vstr_free`, `vstr_eq`, `valloc_or_abort`, and the existing
`Vec<String>` helpers, all of which are already in the prelude or are recorded
as dependencies.

## 6. Exact file changes

### 6.1 `crates/valvet_core/src/ast.rs`

- In `impl Ty::name`, special-case zero-argument `StringIndex` before the
  generic `format!("{}<{}>", ...)` path:

  ```rust
  if base == "StringIndex" && args.is_empty() {
      return "StringIndex".to_string();
  }
  ```

  This keeps diagnostics and `type_name` from printing `StringIndex<>`.

- Update the `Ty::Generic` doc comment to mention that zero-arity builtins
  include `StringIndex`.

- Do not add a new `Ty` variant and do not add a new `ExprKind`. `StringIndex`
  uses `Ty::Generic { base: "StringIndex", args: vec![] }`.

### 6.2 `crates/valvet_core/src/parser.rs`

- Add a helper after `parse_array_rank`:

  ```rust
  fn parse_assoc_tail(
      &mut self,
      base: String,
      base_span: Span,
      type_args: Vec<Ty>,
      start: usize,
  ) -> Option<Expr>
  ```

  It consumes `::`, the method name, and the parenthesized argument list, then
  builds `ExprKind::AssocCall`. It must preserve the current error messages for
  a missing `::`, a missing method name, a missing `(`, and malformed argument
  lists.

- In `parse_primary`, replace the inline associated-call tail for
  `Vec`/`ArrayD`/`Array` with a call to `parse_assoc_tail`.

- In `parse_primary`, before the enum-variant branch
  (`Type::Variant(args)`), add:

  ```rust
  if name == "StringIndex" && matches!(self.cur(), Tok::ColonColon) {
      return self.parse_assoc_tail(
          "StringIndex".to_string(),
          sp,
          vec![],
          sp.start,
      );
  }
  ```

  This is required because `StringIndex::new()` has no `<...>` type argument
  list and would otherwise be parsed as an enum variant.

- Do not add a map literal or string-index syntax.

### 6.3 `crates/valvet_core/src/sema.rs`

- Add `"StringIndex"` to `is_builtin_type_name`.

- In `resolve_ty`, accept both forms:

  - `Ty::Generic { base: "StringIndex", args: [] }`
  - `Ty::Named("StringIndex")`, which resolves to the zero-arity generic form.

  Add `("StringIndex", 0)` to the builtin generic match and add
  `"StringIndex"` to the bare zero-arity match next to `Mat`, `Rng`, and
  `Factor`.

- Update the unknown-generic help text to include `StringIndex`.

- In `record_ty`, add `base == "StringIndex"` to the accepted builtin bases.
  When the base is `StringIndex` with no arguments, record both dependencies:

  ```rust
  self.record_vec_inst(&Ty::Str);
  self.record_ty(&Ty::Generic {
      base: "Option".to_string(),
      args: vec![Ty::Int64],
  });
  ```

  This guarantees that `VVec_String` and `VOption_Int64` layouts and helpers
  exist even when the program only constructs an empty index.

- In `check_index_operands`, after the `ty_has_error(bt)` guard and before the
  index-type loop, add a targeted diagnostic:

  ```rust
  if matches!(bt, Ty::Generic { base, args } if base == "StringIndex" && args.is_empty()) {
      self.err(
          "E0401",
          span,
          "StringIndex cannot be indexed (use `.get(key)` or `.get_or_insert(key)`)",
          None,
      );
      return None;
  }
  ```

  This makes `idx["a"]` and `idx[0]` fail with `E0401` instead of the generic
  integer-index error.

- In `ExprKind::AssocCall`, before the array branch, add a `StringIndex` branch:

  ```rust
  if base == "StringIndex" {
      if !type_args.is_empty() {
          self.err("E0201", *base_span, "`StringIndex` takes no type arguments", None);
          return Ty::Error;
      }
      let t = Ty::Generic {
          base: "StringIndex".to_string(),
          args: vec![],
      };
      if method != "new" {
          self.err(
              "E0401",
              *method_span,
              format!("unknown associated function `StringIndex::{}`", method),
              Some("available: new".into()),
          );
          return Ty::Error;
      }
      if !args.is_empty() {
          self.err("E0201", *method_span, "`StringIndex::new` takes no arguments", None);
      }
      self.record_ty(&t);
      return self.insert(e.id, t);
  }
  ```

  The success path must use `self.insert(e.id, t)`, not `return t`. The
  early return bypasses the type insertion at the end of `check_expr`.
  Without it, `return StringIndex::new();` lowers as a `Unit` return and
  emits `return;` from a non-void function.

- In `ExprKind::Method`, after the `Factor` arm and before `Ty::Str`, add:

  ```rust
  Ty::Generic { base, args } if base == "StringIndex" && args.is_empty() => {
      let t = self.check_string_index_method(method, *method_span, call_args);
      self.record_ty(&t);
      t
  }
  ```

- Add `check_string_index_method` next to `check_factor_method`:

  ```rust
  fn check_string_index_method(
      &mut self,
      method: &str,
      method_span: Span,
      args: &[Expr],
  ) -> Ty
  ```

  Required behavior:

  - `len`: reject any argument, return `Int64`.
  - `contains`: require exactly one argument of type `String`, return `Bool`.
  - `get`: require exactly one argument of type `String`, return
    `Option<Int64>`.
  - `get_or_insert`: require exactly one argument of type `String`, return
    `Int64`.
  - `keys`: reject any argument, return `Vec<String>`.
  - `clone`: reject any argument, return `StringIndex`.
  - Unknown method: `E0401` with the list
    `len, contains, get, get_or_insert, keys, clone`.

  Key checks must use `self.check_expr` and must report `E0201` with a
  message such as `` `get` expects a String key, found Int32 ``. The `found`
  type is whatever `check_expr` returns; do not force a default. Do not set
  `self.expected` for the key; a string literal is already `Str`, and a
  non-string key must fail rather than coerce.

- Update the `E0401` fallback message in `ExprKind::Method` so the list of
  built-ins with methods includes `StringIndex`. The list currently names
  `Vec`, `Mat`, `ArrayD`, `Array`, `Factor`, `String`, and `Rng`.

- Do not add `StringIndex` to `printable` or `record_print_insts`. Printing the
  index itself stays rejected.

- In `is_copy_ty`, add `Ty::Generic { base, .. } if base == "StringIndex" =>
  false`.

- In `Stmt::Expr`, allow discarding a method result only when the receiver
  type is `StringIndex` and the method is `keys` or `clone`. Both are pure
  snapshots, and codegen still materializes and frees their owned results.
  All other method results keep the existing `E0201` "returns a value" check.

### 6.4 `crates/valvet_core/src/ownership.rs`

- In `ufcs_hit`, add:

  ```rust
  Ty::Generic { base, args } if base == "StringIndex" && args.is_empty() => false,
  ```

  This matches sema: a `StringIndex` method never falls back to a free
  function.

- No change is needed to `ExprKind::AssocCall`; `StringIndex::new()` takes no
  arguments and moves nothing.

- No functional change is needed to `ExprKind::Method`; only `push` moves its
  argument, and `get`, `contains`, and `get_or_insert` borrow their key. Add a
  comment that `get_or_insert` clones on insertion.

- `is_copy_ty` in sema already makes the type move-only. The ownership pass
  therefore gets use-after-move, assignment, parameter passing, and return
  behavior from the existing machinery.

### 6.5 `crates/valvet_core/src/codegen.rs`

- Add a layout node:

  ```rust
  enum LNode {
      // existing variants
      StringIndex(String),
  }
  ```

- In `layout_order`:

  - In `dep_of_ty`, add a `StringIndex` branch that pushes
    `LNode::StringIndex(key)`.
  - In the generic-instantiation node selection, add
    `else if base == "StringIndex" { LNode::StringIndex(key.clone()) }`.
  - Do not add layout dependencies. The `StringIndex` layout contains only
    raw pointers and counts. Its helpers use `Vec<String>` and
    `Option<Int64>`, but those are helper dependencies, not layout members.

- In `emit_adts`:

  - Add `LNode::StringIndex(key) => self.emit_string_index_layout(key)?` to
    the layout match.
  - Add `StringIndex` to the early-return guard in `emit_union_layout`:
    `if base == "Vec" || base == "Mat" || base == "Factor" || base ==
    "StringIndex" { return Ok(()); }`. The layout-order branch should already
    prevent `StringIndex` from reaching `LNode::Union`, but the guard keeps a
    future recording mistake from emitting an empty tagged union.

  - Add a forward-declaration branch for `base == "StringIndex" &&
    args.is_empty()` that emits:

    ```c
    static void v_drop_StringIndex_(VStringIndex_ *v);
    static VStringIndex_ v_clone_StringIndex_(VStringIndex_ v);
    ```

  - Add a helper-emission branch in the two-pass loop after the existing
    pass-gating `continue` and after the `Factor` branch, before the generic
    `emit_inst_drop`/`emit_inst_clone` fallback:

    ```rust
    if base == "StringIndex" && args.is_empty() {
        self.emit_string_index_fns()?;
        continue;
    }
    ```

    The branch must stay below the pass gate so the fixed-name helpers are
    emitted once, in pass 0. Without the branch, the fallback would emit an
    empty tagged-union helper.

- Add `emit_string_index_layout`:

  ```rust
  fn emit_string_index_layout(&mut self, key: &str) -> Result<(), Ice>
  ```

  It looks up `(base, args)` from `generic_insts`, requires
  `base == "StringIndex"` and empty args, computes
  `ctype_inst("StringIndex", &[])`, and emits the C typedef from section 4.

- Add `emit_string_index_fns`:

  ```rust
  fn emit_string_index_fns(&mut self) -> Result<(), Ice>
  ```

  It emits the helpers from section 5 in dependency order:
  `vsi_new`, `vsi_rehash`, `vsi_reserve`, `vsi_find_hashed`, `vsi_len`,
  `vsi_contains`, `vsi_get`, `vsi_get_or_insert`, `vsi_keys`,
  `v_drop_StringIndex_`, `v_clone_StringIndex_`.

  Use `ctype_inst("StringIndex", &[])` for the struct type,
  `ctype_inst("Option", &[Ty::Int64])` for the `get` result, and
  `ctype_inst("Vec", &[Ty::Str])` for the `keys` result. Do not hardcode the
  helper type names beyond the fixed `vsi_`/`v_*_StringIndex_` symbols.

- In `ExprKind::AssocCall`, before the array branch, add:

  ```rust
  if base == "StringIndex" {
      if !type_args.is_empty() || method != "new" || !args.is_empty() {
          return Err(Ice("unsupported StringIndex associated call".into()));
      }
      let ct = ctype_inst("StringIndex", &[]);
      return Ok(self.new_tmp_ty(&ct, "vsi_new()"));
  }
  ```

  `new_tmp_ty` registers the fresh value in the temporary frame so the normal
  move and drop paths apply.

- In `ExprKind::Method`, before the `let (elem_ty, iname) = match &rt` block,
  add:

  ```rust
  if matches!(&rt, Ty::Generic { base, args } if base == "StringIndex" && args.is_empty()) {
      let saved = self.cur_move;
      self.cur_move = false;
      let r = self.mat_str(receiver)?;
      self.cur_move = saved;
      return self.emit_string_index_method(method, &r, args);
  }
  ```

- Add `emit_string_index_method`:

  ```rust
  fn emit_string_index_method(
      &mut self,
      method: &str,
      r: &str,
      args: &[Expr],
  ) -> Result<String, Ice>
  ```

  Behavior:

  - `len` -> `vsi_len(&r)`
  - `contains` -> `vsi_contains(&r, key)`
  - `get` -> `vsi_get(&r, key)`
  - `get_or_insert` -> `vsi_get_or_insert(&r, key)`
  - `keys` -> materialize `vsi_keys(&r)` with `new_tmp_ty` and
    `ctype_inst("Vec", &[Ty::Str])`
  - `clone` -> materialize `v_clone_StringIndex_(r)` with `new_tmp_ty` and
    `ctype_inst("StringIndex", &[])`
  - unknown -> `Ice("unknown StringIndex method")`

  Materializing the two owned results is required so a discarded
  `idx.keys();` or `idx.clone();` statement is freed by the frame. `mat_str`
  sees the temp and reuses it instead of creating a second temp, so normal
  move-position use still allocates exactly once.

  Add a helper:

  ```rust
  fn string_index_key(&mut self, args: &[Expr]) -> Result<String, Ice>
  ```

  It takes the first argument and returns an `Ice` when there is no argument,
  sets `cur_move = false`, calls `mat_str`, and restores `cur_move`. `mat_str`
  is required so a string literal or a call result becomes a temporary that is
  freed at statement end. The index clones the key, so the caller's slot stays
  live.

- `cty`, `cty_val`, `drop_call`, `clone_call`, `emit_destroy`, and
  `valvet_ty_of_cty` need no special cases. `StringIndex` flows through the
  existing generic arms and resolves to `VStringIndex_`,
  `v_drop_StringIndex_`, and `v_clone_StringIndex_`.

- Add a `StringIndex` branch to both `emit_poison` and `poison_text`, before
  the generic `Option`/`Result`/`Missing` fallback. This is mandatory. A moved
  `StringIndex` must not free its keys, hashes, or buckets from the source
  slot. The branch emits:

  ```c
  <lval>.keys = NULL;
  <lval>.hashes = NULL;
  <lval>.buckets = NULL;
  <lval>.len = 0;
  <lval>.cap = 0;
  <lval>.bucket_cap = 0;
  ```

  This covers direct moves, struct fields, enum payloads, `Option` payloads,
  and constructor temporaries. Without it, `val idx = StringIndex::new()`
  would leave the moved-from constructor temp live, and scope exit would
  double-free the index.

- Do not add `StringIndex` to `fmt_call`, `emit_print_fmt`, or
  `record_print_insts`.

### 6.6 `crates/valvet_core/tests/front_end.rs`

Add these tests:

1. `string_index_constructors_and_methods_accepted_and_lowered`
   - Construct with `StringIndex::new()`.
   - Call `len`, `contains`, `get`, `get_or_insert`, `keys`, and `clone`.
   - Assert `front_end` succeeds.
   - Assert generated C contains `VStringIndex_`, `vsi_get_or_insert`,
     `v_drop_StringIndex_`, `v_clone_StringIndex_`, `VOption_Int64`, and
     `vvec_Vec_String_with_capacity`.
   - Assert the exact layout string from section 4 is present.

2. `string_index_get_returns_option_int64`
   - `val x: Option<Int64> = idx.get("a")` succeeds.
   - `val x: Int64 = idx.get("a")` fails with `E0201`.

3. `string_index_key_must_be_string`
   - `idx.get(1)` fails with `E0201`.

4. `string_index_unknown_method`
   - `idx.nope()` fails with `E0401`.

5. `string_index_is_move_only`
   - Passing an index to a function and then using it fails with `E0301`.

6. `string_index_not_printable`
   - `println(idx)` fails with `E0201`.

7. `string_index_not_indexable`
   - `idx["a"]` fails with `E0401`.

8. `string_index_method_arity`
   - `idx.len(1)`, `idx.contains()`, `idx.get()`, `idx.get("a", "b")`,
     `idx.get_or_insert()`, `idx.keys(1)`, and `idx.clone(1)` fail with
     `E0201`.

9. `string_index_clone_and_keys_lower_to_helpers`
   - Assert `vsi_keys` and `v_clone_StringIndex_` appear when those methods are
     used.
   - Assert a struct with a `StringIndex` field emits its own drop and clone
     helpers, and that those helpers call `v_drop_StringIndex_` and
     `v_clone_StringIndex_`.

### 6.7 Fixtures

Create these compile-pass fixtures and stdout sidecars:

- `valvet-tests/compile-pass/string_index_basic.valvet`
  - Empty index, absent `get`, duplicate `get_or_insert`, `contains`, `keys`,
    and `clone`.
  - Expected design output:

    ```text
    0
    false
    None
    0
    1
    0
    2
    true
    Some(0)
    Some(1)
    None
    [a, b]
    2
    [a, b]
    ```

- `valvet-tests/compile-pass/string_index_group.valvet`
  - Parallel keys and values, first-appearance grouping, sums by position.
  - Expected design output:

    ```text
    [east, west, north]
    [20, 20, 40]
    ```

- `valvet-tests/compile-pass/string_index_growth.valvet`
  - Insert at least 20 distinct keys so the bucket table rehashes more than
    once.
  - Assert every returned position equals the insertion index, duplicate
    positions are stable after growth, `len` is correct, and `keys()` preserves
    first-appearance order.
  - Clone the grown index and check a duplicate lookup in the clone.
  - Expected design output:

    ```text
    20
    0
    19
    true
    None
    20
    k0
    k19
    20
    0
    ```

- `valvet-tests/compile-pass/string_index_owned.valvet`
  - `StringIndex` as a struct field, moved into the struct, dropped at scope
    exit.
  - `keys()` snapshot is independent of the index.
  - `clone()` is deep.

- `valvet-tests/compile-pass/string_index_move.valvet`
  - Move a `StringIndex` into a function and leave the source binding in scope
    until function exit.
  - Move a struct containing a `StringIndex` into a function and leave the
    source binding in scope until function exit.
  - Return a `StringIndex` from a function and leave the function's local in
    scope until return.
  - Borrow a `StringIndex` through a reference parameter and call
    `(*idx).len()` and `(*idx).contains(...)`. Method receivers do not
    auto-deref, so the explicit dereference is required.
  - This fixture exists specifically to catch missing poison handling under
    ASan/LSan. It must not use the moved-from binding after the move.

- `valvet-tests/compile-pass/string_index_discard.valvet`
  - Call `idx.keys();` and `idx.clone();` as discarded expression statements.
  - Repeat both calls on a `StringIndex::new()` temporary receiver, so the
    discarded result and the temporary index are both freed.
  - This fixture exists to catch a missing `new_tmp_ty` on owned method results
    under ASan/LSan.

- `valvet-tests/compile-pass/string_index_containers.valvet`
  - `Vec<StringIndex>`: push, indexed clone read, `clone`, `pop`, and drop.
  - `ArrayD<StringIndex>`: `from_vec`, indexed clone read, `clone`, `to_vec`,
    and drop.
  - `Option<StringIndex>`: `Some`, match binding, and drop. Wrap it in a
    `Vec<Option<StringIndex>>` and clone that vector to exercise the Option
    clone helper.
  - A user enum with a `StringIndex` payload: construct, match binding, and
    drop. Wrap it in a `Vec<Holder>` and clone that vector to exercise the enum
    clone helper.
  - A user struct with a `StringIndex` field: wrap it in a `Vec<Indexed>` and
    clone that vector to exercise the struct clone helper.
  - Move a `StringIndex` into each container and leave the source binding in
    scope until exit, so poison handling is exercised.
  - This fixture exists to catch missing drop, clone, or poison support for
    containment under ASan/LSan.

Create these compile-fail fixtures and `.error` sidecars:

- `valvet-tests/compile-fail/string_index_unknown_method.valvet` -> `E0401`
- `valvet-tests/compile-fail/string_index_key_type.valvet` -> `E0201`
- `valvet-tests/compile-fail/string_index_not_indexable.valvet` -> `E0401`
- `valvet-tests/compile-fail/string_index_use_after_move.valvet` -> `E0301`
- `valvet-tests/compile-fail/string_index_print.valvet` -> `E0201`
- `valvet-tests/compile-fail/string_index_constructor_arity.valvet` -> `E0201`
- `valvet-tests/compile-fail/string_index_method_arity.valvet` -> `E0201`

The fixture harness compares stdout byte-for-byte. The expected outputs above
are design targets and must be verified by running the fixtures, not asserted
from this document.

### 6.8 Documentation

- `README.md`
  - Add `StringIndex` to the user-facing type list.
  - Add a short `StringIndex` example next to the existing collection example.
  - Add a scope note: the language has an ordered string-keyed index, not a
    general `Map<K,V>`.

- `docs/implementation/ARCHITECTURE.md`
  - Add a `StringIndex` subsection under `Built-in containers`.
  - Document first-appearance order, move-only ownership, internal hash table,
    and the method list.
  - Add `StringIndex` to the owned-type list in the ownership section.

- `docs/library_ecosystem_roadmap/05_implementation_roadmap.md`
  - Add a `COL-03` row for the ordered string index in the collections phase.
  - Mark it as effort M and not on the critical path.
  - Reference this specification for exact changes.

- `docs/library_ecosystem_roadmap/06_actionable_task_list.md`
  - Add a detailed `COL-03` row with the exact file list from section 6.
  - Record the out-of-scope items: general maps, deletion, non-string keys,
    map literals, string indexing, and join multimaps.

- `docs/library_ecosystem_roadmap/09_completion_status.md`
  - Add `COL-03` to the summary table only after implementation and validation.
  - Record the files changed, tests executed, issues found, and completion
    evidence. Do not claim completion before the gates pass.

- `docs/implementation/IMPLEMENTATION_STATUS.md`
  - Update test totals and the feature matrix only after implementation.
  - Do not describe `StringIndex` as implemented before it is.

### 6.9 Files that must not change

- `crates/valvet_core/src/resolver.rs`: built-in type names are resolved in
  sema, not by the resolver. No change is required.
- `crates/valvet_cli/src/main.rs`: no CLI change is required.
- `stdlib/*`: no stdlib migration is part of the container implementation.
  Migration is a separate phase in section 8.

## 7. Validation gates

Run all of these after implementation:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
valvet test valvet-tests
```

Additional checks:

- The new compile-pass fixtures produce the expected stdout in debug and
  release runs.
- The growth fixture forces multiple bucket rehashes and preserves every
  position and the first-appearance key order.
- The new compile-fail fixtures report the expected error codes.
- Existing fixtures are byte-identical.
- ASan/UBSan/LSan runs on the new fixtures are clean.
- Generated C for a `StringIndex`-only program contains no unresolved helper
  names and no `<error>` text.
- `println(idx)` and `idx[key]` fail before codegen.
- `StringIndex` as a struct field, enum payload, `Option` payload, `Vec`
  element, `ArrayD` element, and function parameter or return value drops,
  clones, and moves cleanly.
- No `-ffast-math`, `-march=native`, or implicit FMA is introduced.
- No arithmetic order or allocation order changes in existing code.

## 8. Migration plan after the container lands

The container is useful by itself, but the audit's performance motivation is
only realized when the stdlib uses it. Migrate in separate changes, one module
at a time, with byte-identical fixtures.

Priority order:

1. `stdlib/frame.valvet`
   - `frame_group_count`
   - `frame_group_sum`
   - `frame_group_mean`
   - `frame_crosstab`
   - `frame_pivot_wider`
   - `frame_select`, `frame_mutate`, `frame_drop_na`, and
     `frame_pivot_longer` only where a local index replaces a repeated scan.

   Pattern: keep a `StringIndex` for keys and parallel `Vec<Int64>` or
   `Vec<Float64>` values. `get_or_insert` supplies the value position. Preserve
   first-appearance order in the emitted frame.

2. `stdlib/sql.valvet`
   - The `GROUP BY` path.
   - Pattern: replace the key scan and per-key scan with one index and
     parallel accumulators.

3. `stdlib/model.valvet`
   - `push_unique`, `factor_levels`, `find_indices`, and `dummy_expand`.
   - `push_unique` cannot become O(1) on its own. Thread an index through the
     caller, or add a helper that takes `&mut StringIndex` and the output
     vector. Inside that helper, call `(*idx).get_or_insert(key)` because
     method receivers do not auto-deref. Do not rebuild the index from the
     output vector on every call.

4. `stdlib/features.valvet`
   - `target_encode`.
   - Pattern: map category to a group position, then accumulate in parallel
     sums and counts.

5. `stdlib/validate.valvet`
   - The `seen` set.
   - Pattern: `contains` before reporting a duplicate, then `get_or_insert`.

6. `stdlib/plot.valvet`
   - Facet panel keys.
   - Preserve first-appearance panel order.

Do not migrate `join_core` in this phase. Joins need multiple positions per key
and therefore a multimap or bucket structure. A single `StringIndex` is not a
join index. Keep the nested-loop join until a separate multimap specification
exists.

Do not cache a `StringIndex` inside `DynamicFrame`. `frame_mutate` rebuilds
`names` and `cols`, so a cached index would need invalidation or rebuilding.
Use a local index per operation.

Every migration change must:

- Preserve first-appearance order.
- Preserve arithmetic evaluation order.
- Preserve the existing allocation behavior outside the index itself.
- Keep fixture stdout byte-identical.
- Add or update a focused fixture when the behavior is not already covered.

## 9. Risks

- The hash table probe loop can hang if the load factor reaches 1. The resize
  check must keep at least one empty bucket.
- `vsi_rehash` must use the cached hashes, not recompute them after keys move.
- `vsi_reserve` must grow `keys` and `hashes` together. Growing one without the
  other leaves the layout inconsistent.
- `vsi_clone` must clone keys before the source is dropped and must rebuild the
  bucket array from the copied hashes.
- `get_or_insert` must clone the key. Moving the caller's string would make
  `idx.get_or_insert(keys[i])` invalid.
- String literal keys must go through `mat_str` so their temporary is freed at
  statement end.
- `record_ty` must record `Vec<String>` and `Option<Int64>` for every
  `StringIndex`, or codegen will emit helpers that reference missing layouts.
- The `LNode` order must emit the `StringIndex` layout before any struct, enum,
  union, `Vec`, or `ArrayD` layout that names it.
- The generic drop/clone fallback must not treat `StringIndex` as a tagged
  union.
- `emit_poison` and `poison_text` must null `keys`, `hashes`, and `buckets` for
  a moved-from `StringIndex`. Missing poison handling double-frees the
  constructor temp, a moved field, or a moved payload at scope exit.
- `Ty::name` must render `StringIndex`, not `StringIndex<>`, in diagnostics.
- Performance claims must be measured after migration. A linear-scan
  implementation would not fix the O(n^2) paths, so the hash table is part of
  this specification, not a later optimization.
- A general `Map<K,V>` remains a separate design. Do not expand this container
  with non-string keys or deletion without a new consumer audit.

## 10. Acceptance summary

The implementation is complete when:

- `StringIndex::new()` and the six methods typecheck and lower.
- `get` returns `Option<Int64>` and `get_or_insert` returns stable positions.
- First-appearance order is preserved by `keys()`.
- Duplicate keys do not append.
- Bucket growth and a clone after growth preserve positions and order.
- `clone` is deep and `keys()` is an independent snapshot.
- Drop, clone, and move behavior pass ASan/UBSan/LSan checks.
- Wrong key types, unknown methods, method and constructor arity, string
  indexing, printing, and use after move produce the specified diagnostics.
- `StringIndex` works as a struct field, enum payload, `Option` payload, `Vec`
  element, `ArrayD` element, and function parameter or return value.
- Existing fixtures remain byte-identical.
- `cargo fmt`, clippy, the workspace tests, and the full fixture suite pass.
