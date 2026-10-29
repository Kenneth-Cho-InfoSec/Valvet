# Plan: typed, missing-aware frames and the data-science stack

Status: planning only. No implementation has been started by this document.

Target: post-0.5.0 local tree.

Basis: `stdlib/frame.valvet`, `stdlib/csv.valvet`, `stdlib/schema.valvet`,
`stdlib/stats.valvet`, `stdlib/model.valvet`, `stdlib/factor.valvet`,
`docs/implementation/PLAN-0.2.0.md`, and the current fixture harness.

Interpretation: "all of them" means the full recommended stack:

1. Typed, missing-aware columns, typed CSV loading, and stats interop.
2. Multi-key grouped aggregation and grouped summarise.
3. Typed joins.
4. Grouped window functions.
5. Parquet and Arrow interop.
6. Lazy plans.

Dependency order:

```
P1 -> P2 -> P3 -> P4 -> P5A -> P5B -> P6
```

P3 and P4 are independent after P2, but P4 reuses the group index introduced
in P2. P6 consumes P1 through P5. P5B (writers) depends on P5A (readers) and
on the `bytes_to_string` builtin introduced in P5A.

Every phase lands green before the next phase starts. No phase replaces
`DynamicFrame`, and no phase changes existing SQL, plot, validation, model,
or string-frame fixture output.

## 0. Decisions and invariants

### 0.1 Coexistence

`DynamicFrame` remains the string-backed compatibility frame. It keeps
`names: Vec<String>`, `cols: Vec<Vec<String>>`, and `nrows: Int64`.
`TypedFrame` is added beside it, not in place of it.

Existing APIs that take `&DynamicFrame` keep their signatures. Typed frames
cross into those APIs through `tf_to_dynamic`. The first typed release does
not rewrite `std::sql`, `std::plot`, `std::validate`, `std::features`, or
`std::model`. Those rewrites are separate decisions.

### 0.2 Typed column model

The first typed column model uses one `Column` enum:

```valvet
pub enum Column {
    Int(Vec<Missing<Int64>>),
    Float(Vec<Missing<Float64>>),
    Bool(Vec<Missing<Bool>>),
    Str(Vec<Missing<String>>),
    Factor(Factor),
}
```

`Missing<String>` is supported by the current compiler and was probe-tested
during planning. The first implementation uses `Vec<Missing<T>>` for
correctness and stats interop. A value buffer plus a validity bitmap is a
later optimization after profiling, not part of this plan.

`Factor` keeps its existing code-0 missing convention. Typed operations that
rebuild a factor column through `factor_from_strings` may compact levels to
the levels present in the result. That is documented and locked by fixtures.
Preserving unused levels needs a future `factor_from_codes` builtin, which is
not part of this plan.

### 0.3 Missing semantics

`Missing` is distinct from `Option` and from `NaN`. The typed frame stores
`Missing` per cell where the column type supports it. A factor stores missing
as code 0.

Typed CSV loading treats the existing NA spellings from `std::csv::is_na`
(`""`, `"NA"`, `"null"`) as `Missing`. Strict typed loading also rejects
unparseable non-NA cells with a column and row in the error. Lenient typed
loading maps unparseable cells to `Missing`.

Aggregations use `MissingPolicy::Skip` or `MissingPolicy::Propagate` from
`std::stats`. The exact per-aggregate semantics are listed in P2.

### 0.4 API style

No closures, no traits, no user generics, and no general `Map<K,V>`.

Grouping, summarising, joining, and windowing take column names and small
first-order enums or structs. `Agg`, `AggSpec`, `SortKey`, `WindowSpec`, and
`FilterTerm` are data. They are not function values.

`StringIndex` is used for string-key lookups, column-name uniqueness checks,
and the local right-side join multimap. It is not extended into a general map.
The join multimap is built from `StringIndex` plus parallel head, tail, and
next arrays. It stays inside `std::typed_join`.

### 0.5 Determinism and exactness

All new outputs are deterministic:

- Group keys keep first-appearance order.
- Join output keeps left-row order, then right-match order, then unmatched
  right rows for right and full joins.
- Window output keeps the original frame row order. Sorting only controls
  evaluation order inside a partition.
- Ties in stable sorts keep input order.
- No `-ffast-math`, no implicit FMA, and no reordering of existing arithmetic.

Numeric aggregation semantics are fixed in P2 and locked by fixtures. Integer
sums stay integer and exact within the existing wrapping semantics. Float
sums and means use `std::stats` kernels, including their Kahan summation, so
typed float results are deterministic but may differ in the last bits from
the old string-frame `frame_group_sum` simple-sum loop. That difference is
documented and tested.

### 0.6 Scope boundaries

Deferred explicitly:

- Replacing `DynamicFrame` or changing existing frame fixture output.
- Typed SQL, typed plotting, typed validation, and typed model frames. Use
  `tf_to_dynamic` for those paths in this plan.
- Closures, traits, user generics, general maps, and lazy-lazy joins.
- Parquet nested, repeated, dictionary, decimal, date, timestamp, gzip,
  zstd, and brotli support.
- Arrow stream format, dictionary encoding, nested types, and compression.
- A cost-based lazy optimizer. P6 is a flat plan with a small rule set.

### 0.7 Core changes

P1 through P4 are stdlib-only. P5A adds `read_file_limit` for bounded binary
reads and `bytes_to_string` for checked binary buffers, because the Snappy
reader must return a byte-exact `String`. P5B adds writers on top of those
builtins. P6 is stdlib-only.

No ABI change is planned beyond those two builtins. Both are checked,
length-aware, and covered by front-end and fixture tests.

## 1. P1: typed columns, typed CSV, and stats interop

### 1.1 Goal

A user can load a CSV into a `TypedFrame`, inspect typed and missing-aware
columns, pass float columns directly to `std::stats`, and convert back to
`DynamicFrame` for existing SQL, plot, validation, and model code.

### 1.2 New files

#### `stdlib/typed_frame.valvet`

Module: `std.typed_frame`.

Imports:

```valvet
import std::frame::DynamicFrame;
import std::csv::is_na;
import std::schema::parse_bool_col;
import std::schema::parse_factor_col;
import std::schema::parse_float_col;
import std::schema::parse_int_col;
import std::schema::parse_str_col;
```

Public types and functions:

```valvet
pub enum Column {
    Int(Vec<Missing<Int64>>),
    Float(Vec<Missing<Float64>>),
    Bool(Vec<Missing<Bool>>),
    Str(Vec<Missing<String>>),
    Factor(Factor),
}

pub struct TypedFrame {
    names: Vec<String>,
    cols: Vec<Column>,
    nrows: Int64,
}

pub enum ColBuild {
    Int(Vec<Missing<Int64>>),
    Float(Vec<Missing<Float64>>),
    Bool(Vec<Missing<Bool>>),
    Str(Vec<Missing<String>>),
    Factor(Vec<String>),
}

pub struct SortKey {
    col: String,
    asc: Bool,
}

pub fun tf_new(names: Vec<String>, cols: Vec<Column>, nrows: Int64): Result<TypedFrame, String>
pub fun tf_clone(f: &TypedFrame): TypedFrame
pub fun tf_names(f: &TypedFrame): Vec<String>
pub fun tf_nrows(f: &TypedFrame): Int64
pub fun tf_ncols(f: &TypedFrame): Int64
pub fun tf_schema(f: &TypedFrame): Vec<String>
pub fun tf_col_index(f: &TypedFrame, name: String): Option<Int64>
pub fun tf_col_at(f: &TypedFrame, index: Int64): Option<Column>
pub fun tf_col(f: &TypedFrame, name: String): Option<Column>
pub fun tf_has_col(f: &TypedFrame, name: String): Bool
pub fun tf_kind(f: &TypedFrame, name: String): Option<String>
pub fun tf_kinds(f: &TypedFrame): Vec<String>
pub fun tf_int_col(f: &TypedFrame, name: String): Option<Vec<Missing<Int64>>>
pub fun tf_float_col(f: &TypedFrame, name: String): Option<Vec<Missing<Float64>>>
pub fun tf_bool_col(f: &TypedFrame, name: String): Option<Vec<Missing<Bool>>>
pub fun tf_str_col(f: &TypedFrame, name: String): Option<Vec<Missing<String>>>
pub fun tf_factor_col(f: &TypedFrame, name: String): Option<Factor>
pub fun tf_select(f: &TypedFrame, names: Vec<String>): Result<TypedFrame, String>
pub fun tf_filter_mask(f: &TypedFrame, mask: Vec<Bool>): TypedFrame
pub fun tf_take_rows(f: &TypedFrame, rows: Vec<Int64>): Result<TypedFrame, String>
pub fun tf_drop_na(f: TypedFrame, col: String): TypedFrame
pub fun tf_set_column(f: TypedFrame, name: String, col: Column): Result<TypedFrame, String>
pub fun tf_to_dynamic(f: &TypedFrame): DynamicFrame
pub fun tf_from_dynamic(f: &DynamicFrame, schema: Vec<String>): Result<TypedFrame, String>

pub fun col_len(c: &Column): Int64
pub fun col_kind(c: &Column): String
pub fun col_clone(c: &Column): Column
pub fun col_is_missing(c: &Column, row: Int64): Bool
pub fun col_to_string(c: &Column, row: Int64): String
pub fun col_float(c: &Column, row: Int64): Missing<Float64>
pub fun col_int(c: &Column, row: Int64): Missing<Int64>
pub fun col_bool(c: &Column, row: Int64): Missing<Bool>
pub fun col_str(c: &Column, row: Int64): Missing<String>
pub fun col_factor(c: &Column): Option<Factor>

pub fun col_builder(c: &Column): ColBuild
pub fun col_build_push(b: &mut ColBuild, c: &Column, row: Int64)
pub fun col_build_push_missing(b: &mut ColBuild)
pub fun col_build_finish(b: ColBuild): Column

pub fun tf_key_token(c: &Column, row: Int64): String
pub fun tf_key_join(tokens: Vec<String>): String
pub fun tf_row_key(f: &TypedFrame, keys: Vec<String>, row: Int64): Result<String, String>
```

Implementation notes:

- `tf_new` validates `names.len() == cols.len()`, unique names, and
  `col_len(col) == nrows` for every column. Unique names use `StringIndex`.
- `tf_key_token` is type-tagged and length-aware: `M` for missing, `I` plus
  the integer text, `F` plus the float text, `B0`/`B1`, `S` plus length plus
  the string bytes, and `C` plus length plus the factor level text. Factor
  code 0 is `M`; a present factor uses `C`. `tf_key_join` prefixes every token
  with its length and `:`. This is collision-free for empty strings,
  delimiters, Unicode, and NUL bytes.
- `tf_take_rows` and `tf_set_column` use `ColBuild`. Factor builders finish
  with `factor_from_strings`, so levels may compact. This is the documented
  v1 behavior.
- `tf_to_dynamic` renders `Missing` as `"NA"`, numbers through `to_string`,
  booleans as `"true"`/`"false"`, and factors through `as_strings`.
- `tf_from_dynamic` uses the supplied schema. A schema entry is one of
  `"int"`, `"float"`, `"bool"`, `"string"`, or `"factor"`.
- `tf_filter_mask` follows the existing frame rule: a mask shorter than
  `nrows` treats missing positions as false.
- `tf_col` and `tf_col_at` return cloned columns. Bulk paths use
  `ColBuild` and do not clone whole columns per row.

#### `stdlib/typed_csv.valvet`

Module: `std.typed_csv`.

Imports:

```valvet
import std::csv::CsvTable;
import std::csv::read_csv;
import std::csv::read_csv_lenient;
import std::csv::read_json;
import std::csv::infer_schema_ex;
import std::typed_frame::TypedFrame;
import std::typed_frame::tf_new;
import std::schema::check_bool_col;
import std::schema::check_factor_col;
import std::schema::check_float_col;
import std::schema::check_int_col;
import std::schema::parse_bool_col;
import std::schema::parse_factor_col;
import std::schema::parse_float_col;
import std::schema::parse_int_col;
import std::schema::parse_str_col;
```

Public functions:

```valvet
pub fun tf_from_table(t: CsvTable, schema: Vec<String>, strict_types: Bool): Result<TypedFrame, String>
pub fun read_csv_typed(path: String, schema: Vec<String>, strict: Bool): Result<TypedFrame, String>
pub fun read_csv_typed_infer(path: String, strict: Bool): Result<TypedFrame, String>
pub fun read_csv_typed_lenient(path: String, max_bad: Int64, schema: Vec<String>): Result<TypedFrame, String>
pub fun read_json_typed(path: String, schema: Vec<String>, strict_types: Bool): Result<TypedFrame, String>
pub fun read_json_typed_infer(path: String, strict_types: Bool): Result<TypedFrame, String>
```

Implementation notes:

- `read_csv_typed` uses `read_csv(path, strict)` for row structure and
  `tf_from_table(t, schema, strict)` for type strictness.
- `read_csv_typed_lenient` uses `read_csv_lenient(path, max_bad)` and
  `strict_types = false`.
- `read_csv_typed_infer` uses `infer_schema_ex(t, true)`, then
  `tf_from_table(t, schema, strict)`.
- `tf_from_table` checks schema length against `t.header.len()`, validates
  each schema spelling, checks strict columns before parsing, and prefixes
  errors with the column name and zero-based row.
- `"factor"` uses `parse_factor_col`; NA spellings become code 0.
- `"string"` uses `parse_str_col`; NA spellings become `Missing`.

#### `stdlib/typed_stats.valvet`

Module: `std.typed_stats`.

Imports:

```valvet
import std::typed_frame::TypedFrame;
import std::typed_frame::Column;
import std::typed_frame::col_is_missing;
import std::typed_frame::tf_float_col;
import std::typed_frame::tf_new;
import std::stats::MissingPolicy;
import std::stats::cov;
import std::stats::cor;
import std::stats::max_v;
import std::stats::mean;
import std::stats::median;
import std::stats::min_v;
import std::stats::n_missing;
import std::stats::quantile;
import std::stats::sd;
import std::stats::sum_v;
import std::stats::var_v;
```

Public functions:

```valvet
pub fun tf_mean(f: &TypedFrame, col: String, policy: MissingPolicy): Option<Float64>
pub fun tf_sum(f: &TypedFrame, col: String, policy: MissingPolicy): Option<Float64>
pub fun tf_median(f: &TypedFrame, col: String, policy: MissingPolicy): Option<Float64>
pub fun tf_min(f: &TypedFrame, col: String, policy: MissingPolicy): Option<Float64>
pub fun tf_max(f: &TypedFrame, col: String, policy: MissingPolicy): Option<Float64>
pub fun tf_var(f: &TypedFrame, col: String, policy: MissingPolicy): Option<Float64>
pub fun tf_sd(f: &TypedFrame, col: String, policy: MissingPolicy): Option<Float64>
pub fun tf_quantile(f: &TypedFrame, col: String, policy: MissingPolicy, q: Float64): Option<Float64>
pub fun tf_cor(f: &TypedFrame, x: String, y: String, policy: MissingPolicy): Option<Float64>
pub fun tf_cov(f: &TypedFrame, x: String, y: String, policy: MissingPolicy): Option<Float64>
pub fun tf_n_missing(f: &TypedFrame, col: String): Option<Int64>
pub fun tf_describe(f: &TypedFrame, cols: Vec<String>): Result<TypedFrame, String>
```

`tf_describe` returns one row per requested column with columns
`column` (String), `n` (Int), `n_missing` (Int), `mean` (Float), `sd`
(Float), `min` (Float), and `max` (Float). Requested columns must be
numeric. Non-numeric requests return `Err`.

### 1.3 Modified files

#### `stdlib/schema.valvet`

Add:

```valvet
pub fun parse_str_col(col: Vec<String>): Vec<Missing<String>>
pub fun parse_factor_col(col: Vec<String>): Factor
pub fun check_int_col(col: Vec<String>): Option<String>
pub fun check_float_col(col: Vec<String>): Option<String>
pub fun check_bool_col(col: Vec<String>): Option<String>
pub fun check_factor_col(col: Vec<String>): Option<String>
```

`parse_str_col` maps NA-like cells to `Missing` and present cells to
`Missing::Present`. `parse_factor_col` maps NA-like cells to `""` and calls
`factor_from_strings`. `check_*_col` returns the first zero-based row that is
not NA and does not parse, with a short reason. `check_factor_col` always
returns `None` because any non-NA string is a valid level.

Existing `parse_int_col`, `parse_float_col`, and `parse_bool_col` keep their
current lenient behavior and signatures.

#### `stdlib/csv.valvet`

Add:

```valvet
pub fun infer_schema_ex(t: &CsvTable, detect_bool: Bool): Vec<String>
```

`infer_schema` delegates to `infer_schema_ex(t, false)` so existing
`infer_schema` output is unchanged. `infer_schema_ex` promotes in the order
int, float, bool, string when `detect_bool` is true. With `detect_bool`
false, the old int, float, string behavior is preserved.

#### Documentation and examples

- `README.md`: add a typed-frame section with a short CSV, accessor, stats,
  and `tf_to_dynamic` example.
- `docs/implementation/ARCHITECTURE.md`: add `std.typed_frame`,
  `std.typed_csv`, and `std.typed_stats` to the module map, and state that
  `DynamicFrame` remains.
- `docs/implementation/TEST_STRATEGY.md`: add typed-frame and typed-CSV
  fixture categories and the oracle policy.
- `docs/implementation/IMPLEMENTATION_STATUS.md`: add a P1 status block only
  after the phase lands.
- `docs/implementation/PLAN-0.2.0.md`: add a one-line pointer to this plan
  for the unfinished `Frame<S>` item.

### 1.4 Fixtures

All compile-pass fixtures need a `.stdout` sidecar. Runtime errors are
printed by the fixture, not left as compiler diagnostics.

| Fixture | Oracle |
| --- | --- |
| `valvet-tests/compile-pass/typed_frame_basic.valvet` | Names, kinds, nrows, and typed column values. |
| `valvet-tests/compile-pass/typed_frame_missing.valvet` | `Missing` vs present values, `n_missing`, and `tf_drop_na`. |
| `valvet-tests/compile-pass/typed_frame_convert.valvet` | `tf_from_dynamic` then `tf_to_dynamic` round trip for int, float, bool, string, factor. |
| `valvet-tests/compile-pass/typed_frame_move.valvet` | Move-only frame through a function, struct field, and return; no use after move. |
| `valvet-tests/compile-pass/typed_frame_empty.valvet` | Empty frame with and without columns; zero-row stats. |
| `valvet-tests/compile-pass/typed_frame_stats.valvet` | Direct `std::stats` calls on `tf_float_col`; Skip and Propagate. |
| `valvet-tests/compile-pass/typed_frame_describe.valvet` | `tf_describe` output and non-numeric `Err`. |
| `valvet-tests/compile-pass/typed_csv_infer.valvet` | Int, float, bool, and string inference; NA spellings. |
| `valvet-tests/compile-pass/typed_csv_schema.valvet` | Explicit schema including factor; typed column values. |
| `valvet-tests/compile-pass/typed_csv_missing.valvet` | Missing values survive strict and lenient loading. |
| `valvet-tests/compile-pass/typed_csv_lenient.valvet` | Unparseable cells become Missing only in lenient mode. |
| `valvet-tests/compile-pass/typed_csv_bad_schema.valvet` | Strict type error names the column and row. |
| `valvet-tests/compile-pass/typed_json_basic.valvet` | JSON table to typed frame with explicit schema. |
| `valvet-tests/compile-pass/schema_str_factor.valvet` | `parse_str_col`, `parse_factor_col`, and strict check helpers. |
| `valvet-tests/compile-pass/typed_group_key_encoding.valvet` | Key tokens for empty strings, `:`, `|`, Unicode, NUL, missing, and factor. |
| `valvet-tests/compile-pass/typed_frame_set_column.valvet` | Replace and append behavior, including factor compaction. |
| `valvet-tests/compile-pass/typed_frame_filter_mask.valvet` | Short mask behavior matches `frame_filter_mask`. |
| `valvet-tests/compile-pass/typed_frame_take_rows.valvet` | Row selection, bounds error, and factor compaction. |
| `examples/typed_frames.valvet` | README-style end-to-end example; run by `valvet test --doc`. |

Add one compile-fail fixture after the type checker behavior is confirmed:

| Fixture | Expected |
| --- | --- |
| `valvet-tests/compile-fail/typed_frame_col_type.valvet` | Assigning `tf_float_col` to `Vec<Missing<Int64>>` is rejected. Record the actual E-code in `.error`. |

### 1.5 Oracles

- Numeric values: compare to a pure-Python standard-library script using
  `csv`, `statistics`, and `math`; `numpy` is available but is not required.
- Missing policy: hand-computed Skip and Propagate matrices, plus
  `n_missing`.
- Typed CSV: a small CSV with `""`, `"NA"`, `"null"`, quoted commas, a
  newline inside quotes, and a factor column.
- Round trip: typed -> dynamic -> typed with the same schema must preserve
  present values and missingness. Factor levels may compact and must be
  asserted explicitly.

### 1.6 Gates

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `target/debug/valvet test valvet-tests`
- `target/release/valvet test valvet-tests`
- `target/debug/valvet test --doc`
- ASan/UBSan/LSan on `typed_frame_move`, `typed_frame_convert`,
  `typed_frame_set_column`, `typed_frame_take_rows`, and
  `typed_group_key_encoding`.
- All pre-existing fixtures and doc examples must remain byte-identical
  (432 fixtures and 17 doc fences/examples at completion).

### 1.7 Risks

- `Vec<Missing<T>>` doubles per-cell metadata. This is accepted for v1.
- Factor rebuilding compacts levels. Lock this with fixtures and document it.
- `tf_col` clones a column. Bulk operations must use `ColBuild`.
- Strict typed loading is a new semantic. Keep it opt-in through the strict
  flag and preserve lenient behavior for existing `parse_*_col` users.

## 2. P2: multi-key grouped aggregation and grouped summarise

### 2.1 Goal

A user can group a `TypedFrame` by one or more keys and produce one row per
group with several named aggregations. No closures and no general map.

### 2.2 New file

#### `stdlib/grouped.valvet`

Module: `std.grouped`.

Imports:

```valvet
import std::typed_frame::Column;
import std::typed_frame::ColBuild;
import std::typed_frame::SortKey;
import std::typed_frame::TypedFrame;
import std::typed_frame::col_build_finish;
import std::typed_frame::col_build_push;
import std::typed_frame::col_build_push_missing;
import std::typed_frame::col_builder;
import std::typed_frame::col_is_missing;
import std::typed_frame::col_kind;
import std::typed_frame::tf_col;
import std::typed_frame::tf_col_at;
import std::typed_frame::tf_key_join;
import std::typed_frame::tf_nrows;
import std::typed_frame::tf_row_key;
import std::typed_frame::tf_set_column;
import std::typed_frame::tf_take_rows;
import std::stats::MissingPolicy;
```

Public types and functions:

```valvet
pub enum Agg {
    Count,
    CountNonMissing(String),
    NMissing(String),
    Sum(String, MissingPolicy),
    Mean(String, MissingPolicy),
    Min(String, MissingPolicy),
    Max(String, MissingPolicy),
    Var(String, MissingPolicy),
    Sd(String, MissingPolicy),
}

pub struct AggSpec {
    name: String,
    agg: Agg,
}

pub struct GroupIndex {
    ids: Vec<Int64>,
    first_rows: Vec<Int64>,
    n_groups: Int64,
}

pub struct GroupedFrame {
    frame: TypedFrame,
    keys: Vec<String>,
    index: GroupIndex,
}

pub fun agg_count(): Agg
pub fun agg_count_non_missing(col: String): Agg
pub fun agg_n_missing(col: String): Agg
pub fun agg_sum(col: String): Agg
pub fun agg_mean(col: String): Agg
pub fun agg_min(col: String): Agg
pub fun agg_max(col: String): Agg
pub fun agg_var(col: String): Agg
pub fun agg_sd(col: String): Agg

pub fun agg_spec(name: String, agg: Agg): AggSpec

pub fun tf_group_index(f: &TypedFrame, keys: Vec<String>): Result<GroupIndex, String>
pub fun tf_group_by(f: TypedFrame, keys: Vec<String>): Result<GroupedFrame, String>
pub fun tf_summarise(g: GroupedFrame, specs: Vec<AggSpec>): Result<TypedFrame, String>
pub fun tf_group_count(g: GroupedFrame): Result<TypedFrame, String>

pub fun grouped_keys(g: &GroupedFrame): Vec<String>
pub fun grouped_n_groups(g: &GroupedFrame): Int64
pub fun grouped_group_ids(g: &GroupedFrame): Vec<Int64>
pub fun grouped_first_rows(g: &GroupedFrame): Vec<Int64>
```

`agg_*` constructors default to `MissingPolicy::Skip`. Callers that need
Propagate write the `Agg` variant directly.

### 2.3 Group index

`tf_group_index`:

1. Validates that every key exists and that key names are unique.
2. Builds a `StringIndex` from `tf_row_key` over all rows.
3. Records `ids[row]`, `first_rows[group]`, and `n_groups`.
4. Keeps first-appearance order.
5. With no keys, returns one group for a non-empty frame and zero groups for
   an empty frame.
6. Treats missing keys as a group, not as an error.

`tf_group_by` consumes a frame and stores the index. `tf_summarise` consumes
the grouped frame.

### 2.4 Aggregation semantics

| Agg | Accepted source | Output | Missing behavior |
| --- | --- | --- | --- |
| `Count` | any | Int | Counts rows, including missing. |
| `CountNonMissing(col)` | any | Int | Counts rows where `col_is_missing` is false. |
| `NMissing(col)` | any | Int | Counts missing cells. |
| `Sum(col, policy)` | Int or Float | Int for Int source, Float for Float source | Skip ignores Missing; Propagate returns Missing if any Missing; all-missing returns Missing. Int sums stay integer. Float sums use `std::stats::sum_v`. |
| `Mean(col, policy)` | Int or Float | Float | Skip ignores Missing; Propagate returns Missing; all-missing returns Missing. Int mean uses an exact Int sum and count. Float mean uses `std::stats::mean`. |
| `Min(col, policy)` | Int, Float, Str, Bool | Same as source | Skip ignores Missing; Propagate returns Missing; all-missing returns Missing. Str compares bytewise. Factor is an `Err` in v1. |
| `Max(col, policy)` | Int, Float, Str, Bool | Same as source | Same as Min. |
| `Var(col, policy)` | Int or Float | Float | Uses `std::stats::var_v`; sample variance; Missing policy as above. |
| `Sd(col, policy)` | Int or Float | Float | Uses `std::stats::sd`; sample standard deviation. |

`NaN` follows `std::stats` semantics for Float columns. It is a present value,
not `Missing`; document this in the fixture header.

Output schema is key columns first, in the requested key order, then one
column per `AggSpec` in spec order. A key column takes the value from the
group's first row. Factor key columns may compact levels.

`tf_summarise` with an empty spec list returns the key columns only.
`tf_group_count` is `tf_summarise` with one `Agg::Count` column named
`count`.

### 2.5 Modified files

- `stdlib/typed_frame.valvet`: add `tf_order_rows` and `tf_arrange_keys` if
  not already present in P1. `tf_order_rows` is a stable multi-key sort over
  a `Vec<Int64>` row list, with missing values last in both ascending and
  descending order. `tf_arrange_keys` applies it to the whole frame.
- `stdlib/typed_frame.valvet`: add `tf_column_names` only if needed by
  `tf_summarise`; otherwise use `tf_schema`.
- `stdlib/frame.valvet`: no change in P2.
- `README.md`, `docs/implementation/IMPLEMENTATION_STATUS.md`: add grouped
  aggregation examples and status after the phase lands.
- `docs/implementation/DICT_SPEC.md`: append a note that typed grouping uses
  `StringIndex` for composite string keys and that the dynamic
  `frame_group_*` migration is separate.

### 2.6 Fixtures

| Fixture | Oracle |
| --- | --- |
| `valvet-tests/compile-pass/typed_group_multi.valvet` | Two keys, first-appearance order, Count/Sum/Mean. |
| `valvet-tests/compile-pass/typed_group_agg_all.valvet` | Every `Agg` variant on int, float, string, and bool columns. |
| `valvet-tests/compile-pass/typed_group_missing.valvet` | Missing keys, missing values, Count vs CountNonMissing vs NMissing. |
| `valvet-tests/compile-pass/typed_group_propagate.valvet` | Skip vs Propagate for every numeric aggregate. |
| `valvet-tests/compile-pass/typed_group_order.valvet` | First-appearance order and stable key order. |
| `valvet-tests/compile-pass/typed_group_global.valvet` | Empty key list gives one global group. |
| `valvet-tests/compile-pass/typed_group_empty.valvet` | Zero rows, zero groups, empty key and aggregate columns. |
| `valvet-tests/compile-pass/typed_group_factor.valvet` | Factor key output and level compaction. |
| `valvet-tests/compile-pass/typed_group_bad_key.valvet` | Missing key, duplicate key, and duplicate spec name return `Err`. |
| `valvet-tests/compile-pass/typed_group_exact_int.valvet` | Large Int64 sum stays integer and exact. |
| `valvet-tests/compile-pass/typed_group_float_oracle.valvet` | Float sums and means match `std::stats` Kahan results. |
| `examples/typed_group.valvet` | README-style group-by and summarise example. |

Oracle: pure Python standard library, with the same first-appearance key
order and sample variance. Compare Int sums exactly; compare Float values by
the printed `to_string` output or a documented tolerance. Lock the exact
printed values in `.stdout`.

### 2.7 Gates

All P1 gates, plus ASan/UBSan/LSan on `typed_group_multi`,
`typed_group_missing`, `typed_group_factor`, and `typed_group_empty`.
Existing frame group fixtures must not change.

### 2.8 Risks

- Composite key encoding must be collision-free. The length-prefixed token
  format is mandatory; do not join raw values with a delimiter.
- `StringIndex` is move-only. Keep it local to `tf_group_index`; store only
  integer ids in `GroupIndex`.
- Float aggregation differences from the old string-frame loops must be
  documented, not hidden.
- High-cardinality keys can allocate one string per row. This is accepted
  for v1; a hashed typed key is a later optimization.

## 3. P3: typed joins

### 3.1 Goal

Inner, left, right, and full joins over typed frames, with one or more key
columns, deterministic order, duplicate-key cartesian behavior, and an audit
that never silently drops rows.

### 3.2 New file

#### `stdlib/typed_join.valvet`

Module: `std.typed_join`.

Imports:

```valvet
import std::frame::JoinAudit;
import std::frame::audit_summary;
import std::frame::join_audit_new;
import std::typed_frame::ColBuild;
import std::typed_frame::Column;
import std::typed_frame::TypedFrame;
import std::typed_frame::col_build_finish;
import std::typed_frame::col_build_push;
import std::typed_frame::col_build_push_missing;
import std::typed_frame::col_builder;
import std::typed_frame::col_kind;
import std::typed_frame::tf_col_at;
import std::typed_frame::tf_key_join;
import std::typed_frame::tf_new;
import std::typed_frame::tf_nrows;
import std::typed_frame::tf_row_key;
import std::typed_frame::tf_schema;
```

Public types and functions:

```valvet
pub struct TypedJoinResult {
    frame: TypedFrame,
    audit: JoinAudit,
}

pub fun tf_join_inner(left: &TypedFrame, lkeys: Vec<String>, right: &TypedFrame, rkeys: Vec<String>): Result<TypedJoinResult, String>
pub fun tf_join_left(left: &TypedFrame, lkeys: Vec<String>, right: &TypedFrame, rkeys: Vec<String>): Result<TypedJoinResult, String>
pub fun tf_join_right(left: &TypedFrame, lkeys: Vec<String>, right: &TypedFrame, rkeys: Vec<String>): Result<TypedJoinResult, String>
pub fun tf_join_full(left: &TypedFrame, lkeys: Vec<String>, right: &TypedFrame, rkeys: Vec<String>): Result<TypedJoinResult, String>
pub fun tf_join_audit_summary(r: &TypedJoinResult): String
```

Internal helpers:

```valvet
enum JoinKind { Inner, Left, Right, Full }
struct RightIndex { keys: StringIndex, heads: Vec<Int64>, tails: Vec<Int64>, next: Vec<Int64> }
fun join_core(left: &TypedFrame, lkeys: Vec<String>, right: &TypedFrame, rkeys: Vec<String>, kind: JoinKind): Result<TypedJoinResult, String>
fun build_right_index(right: &TypedFrame, rkeys: Vec<String>): Result<RightIndex, String>
fun output_names(left: &TypedFrame, right: &TypedFrame, rkeys: Vec<String>): Result<Vec<String>, String>
```

### 3.3 Semantics

- Key counts must match and must be at least one. Duplicate key names are
  `Err`.
- Every key column must exist on its side. Key kinds must match exactly
  (`Int` to `Int`, `Float` to `Float`, `Bool` to `Bool`, `Str` to `Str`,
  `Factor` to `Factor`). Mixed types are `Err`; users convert explicitly.
- Missing keys never match. A left row with a missing key is left-only; a
  right row with a missing key is right-only. This is a deliberate difference
  from the string frame, where `"NA"` is an ordinary key.
- Duplicate keys produce the cartesian product. The audit `matched` field
  counts output matched rows, matching the existing `JoinAudit` meaning.
- Output columns are all left columns, then right non-key columns. If a right
  non-key name collides with a left or already-used name, append `_right`;
  if that still collides, append `_2`, `_3`, and so on. The suffix rule is
  deterministic and documented.
- Row order: matched and left-only rows in left order, with right matches in
  right-table order. For right and full joins, unmatched right rows follow
  in right-table order. Inner joins omit unmatched rows.
- Right index construction uses `StringIndex` plus `heads`, `tails`, and
  `next` arrays. This is a local multimap, not a general map.
- Factor output columns use `ColBuild::Factor` and may compact levels.

### 3.4 Modified files

#### `stdlib/frame.valvet`

Add:

```valvet
pub fun join_audit_new(matched: Int64, left_only: Int64, right_only: Int64, left_rows: Int64, right_rows: Int64, out_rows: Int64): JoinAudit
```

`JoinAudit` fields stay private. `audit_summary` remains the public summary
function. Existing join functions are unchanged.

#### Other

- `stdlib/typed_frame.valvet`: add `tf_col_at` and `tf_new` only if not
  already present from P1.
- `README.md`, `docs/implementation/IMPLEMENTATION_STATUS.md`: add join
  examples and status.
- `docs/implementation/ARCHITECTURE.md`: note the local right-side multimap
  and that it is not part of `StringIndex`.

### 3.5 Fixtures

| Fixture | Oracle |
| --- | --- |
| `valvet-tests/compile-pass/typed_join_inner.valvet` | Matched rows, left-only and right-only audit, output order. |
| `valvet-tests/compile-pass/typed_join_left.valvet` | Left rows preserved, missing right cells. |
| `valvet-tests/compile-pass/typed_join_right.valvet` | Right rows preserved, missing left cells. |
| `valvet-tests/compile-pass/typed_join_full.valvet` | Both sides preserved, audit counts. |
| `valvet-tests/compile-pass/typed_join_multi_key.valvet` | Two-key join and output key order. |
| `valvet-tests/compile-pass/typed_join_missing_keys.valvet` | Missing keys never match; left-only and right-only counts. |
| `valvet-tests/compile-pass/typed_join_duplicate_keys.valvet` | Cartesian product and audit. |
| `valvet-tests/compile-pass/typed_join_suffix.valvet` | `_right`, `_2`, and repeated collision handling. |
| `valvet-tests/compile-pass/typed_join_audit.valvet` | `audit_summary` output for all four kinds. |
| `valvet-tests/compile-pass/typed_join_bad_key.valvet` | Missing key, key count mismatch, duplicate key, and type mismatch. |
| `valvet-tests/compile-pass/typed_join_factor.valvet` | Factor keys join by label; output levels compact. |
| `examples/typed_join.valvet` | README-style join example. |

Oracle: pure Python standard library dictionaries and lists, with explicit
first-appearance and right-order rules. Compare all rows and audit fields.

### 3.6 Gates

All P1 gates, plus ASan/UBSan/LSan on `typed_join_full`,
`typed_join_duplicate_keys`, `typed_join_missing_keys`, and
`typed_join_factor`. Existing `join_audit` and `join_dup_keys` fixtures must
not change.

### 3.7 Risks

- Missing-key semantics differ from `DynamicFrame`. Document this in README
  and the fixture header.
- Suffix collision handling must be deterministic. Do not silently drop a
  column.
- The right index is a local multimap. Do not add join methods to
  `StringIndex` or turn it into a general map.
- Factor level compaction is visible. Lock it in fixtures.

## 4. P4: grouped window functions

### 4.1 Goal

Partitioned window verbs over typed frames: `Lag`, `Lead`, `RowNum`,
`Cumsum`, and `Rank`, with optional ordering inside each partition. Output
stays in the original frame row order.

### 4.2 New file

#### `stdlib/typed_window.valvet`

Module: `std.typed_window`.

Imports:

```valvet
import std::grouped::GroupIndex;
import std::grouped::tf_group_index;
import std::typed_frame::ColBuild;
import std::typed_frame::Column;
import std::typed_frame::SortKey;
import std::typed_frame::TypedFrame;
import std::typed_frame::col_build_finish;
import std::typed_frame::col_build_push;
import std::typed_frame::col_build_push_missing;
import std::typed_frame::col_builder;
import std::typed_frame::col_is_missing;
import std::typed_frame::col_kind;
import std::typed_frame::tf_col;
import std::typed_frame::tf_nrows;
import std::typed_frame::tf_order_rows;
import std::typed_frame::tf_set_column;
```

Public types and functions:

```valvet
pub enum WindowExpr {
    Lag(String, Int64),
    Lead(String, Int64),
    RowNum,
    Cumsum(String),
    Rank(String),
}

pub struct WindowSpec {
    partition: Vec<String>,
    order_by: Vec<SortKey>,
}

pub fun window_spec(partition: Vec<String>, order_by: Vec<SortKey>): WindowSpec
pub fun tf_window_mutate(f: TypedFrame, name: String, spec: WindowSpec, expr: WindowExpr): Result<TypedFrame, String>
```

Internal helpers:

```valvet
fun window_lag(src: &Column, order: Vec<Int64>, k: Int64, n: Int64): Column
fun window_lead(src: &Column, order: Vec<Int64>, k: Int64, n: Int64): Column
fun window_rownum(order: Vec<Int64>, n: Int64): Column
fun window_cumsum(src: &Column, order: Vec<Int64>, n: Int64): Result<Column, String>
fun window_rank(src: &Column, order: Vec<Int64>, n: Int64): Result<Column, String>
```

### 4.3 Semantics

- `partition` keys are validated with `tf_group_index`. Missing partition
  keys form a group. An empty partition list is one global partition.
- `order_by` is a stable multi-key sort inside each partition. Missing sort
  values sort last in both ascending and descending order. If `order_by` is
  empty, input row order is used.
- The output column replaces an existing column of the same name or is
  appended. This matches `frame_mutate`.
- `Lag` and `Lead` preserve the source column type and fill out-of-range
  positions with `Missing`. Factor outputs may compact levels.
- `RowNum` is `Int64`, one-based, and resets per partition.
- `Cumsum` preserves Int64 for an Int source and Float64 for a Float source.
  It resets per partition. Bool, Str, and Factor sources are `Err`.
- `Rank` is Float64, one-based, ties averaged, and resets per partition. It
  ranks the named source column, not the `order_by` columns. Missing values
  rank Missing. `order_by` controls evaluation order only.
- The output is scattered back to original row positions. The frame row
  order never changes.
- Empty frames return an empty column of the correct type. Source columns
  are still validated.

### 4.4 Modified files

- `stdlib/typed_frame.valvet`: add `tf_order_rows` and `tf_arrange_keys` if
  not already present from P2. `tf_order_rows` is the stable comparator used
  by both arrange and windows.
- `stdlib/grouped.valvet`: add `grouped_group_ids` and
  `grouped_first_rows` if not already present from P2.
- `README.md`, `docs/implementation/IMPLEMENTATION_STATUS.md`: add window
  examples and status.
- `docs/implementation/ARCHITECTURE.md`: note that window output is scattered
  back to original row order.

### 4.5 Fixtures

| Fixture | Oracle |
| --- | --- |
| `valvet-tests/compile-pass/typed_window_group_lag.valvet` | Lag and Lead per partition, boundary Missing. |
| `valvet-tests/compile-pass/typed_window_group_rank.valvet` | Rank with ties, missing, and multiple partitions. |
| `valvet-tests/compile-pass/typed_window_order.valvet` | `order_by` changes evaluation order but not output row order. |
| `valvet-tests/compile-pass/typed_window_missing.valvet` | Missing partition keys, missing sort keys, missing source values. |
| `valvet-tests/compile-pass/typed_window_empty.valvet` | Empty frame and empty partitions. |
| `valvet-tests/compile-pass/typed_window_bad_col.valvet` | Missing partition, order, or source column returns `Err`. |
| `valvet-tests/compile-pass/typed_window_global.valvet` | Empty partition list behaves like the current global window verbs. |
| `valvet-tests/compile-pass/typed_window_cumsum_int.valvet` | Int cumsum stays exact and resets per partition. |
| `valvet-tests/compile-pass/typed_window_factor.valvet` | Lag/Lead on a factor and level compaction. |
| `examples/typed_window.valvet` | README-style grouped window example. |

Oracle: pure Python standard library per-partition loops for shift, cumsum,
and average rank. Lock exact printed values.

### 4.6 Gates

All P1 gates, plus ASan/UBSan/LSan on `typed_window_group_lag`,
`typed_window_group_rank`, `typed_window_missing`, and
`typed_window_factor`. Existing `frame_window` output must not change.

### 4.7 Risks

- Sorting must be stable. A wrong tie rule changes rank and shift output.
- `Rank` and `order_by` have different jobs. Document this clearly.
- Factor compaction is visible in window outputs.
- Missing sort values must stay last for both directions. Do not reverse the
  whole order for descending sorts.

## 5. P5A: Parquet and Arrow readers

### 5.1 Goal

Read a narrow, documented subset of Parquet and Arrow IPC files into a
`TypedFrame`, with checked bounds, checked arithmetic, allocation caps, and
malformed-input tests. No external crates and no C library dependencies.

### 5.2 New files

#### `stdlib/bytes.valvet`

Module: `std.bytes`.

Public functions:

```valvet
pub struct Varint { value: Int64, next: Int64 }
pub fun bytes_u8(s: &String, at: Int64): Option<Int64>
pub fun bytes_u16_le(s: &String, at: Int64): Option<Int64>
pub fun bytes_u32_le(s: &String, at: Int64): Option<Int64>
pub fun bytes_i32_le(s: &String, at: Int64): Option<Int64>
pub fun bytes_u64_le(s: &String, at: Int64): Option<Int64>
pub fun bytes_i64_le(s: &String, at: Int64): Option<Int64>
pub fun bytes_varint(s: &String, at: Int64): Option<Varint>
pub fun bytes_zigzag(n: Int64): Int64
pub fun bytes_bit(s: &String, bit: Int64): Option<Bool>
pub fun bytes_bitpacked(s: &String, bit_offset: Int64, width: Int64): Option<Int64>
pub fun bytes_slice(s: &String, start: Int64, end: Int64): Option<String>
pub fun bytes_from_string(s: &String): Vec<Int64>
```

All P5A readers return `Option` and never trap. Writer helpers and
`bytes_finish` are added in P5B, using the P5A `bytes_to_string` builtin.

#### `stdlib/thrift.valvet`

Module: `std.thrift`.

Implement the Thrift compact protocol subset needed by Parquet metadata and
page headers: field headers, bool, byte, i16, i32, i64, double, binary,
list header, struct stop, and skip. Depth cap: 32. All lengths are checked.

Public types and functions:

```valvet
pub struct ThriftReader { data: String, pos: Int64 }
pub struct ThriftField { id: Int64, kind: Int64, next: Int64 }

pub fun thrift_reader(data: String): ThriftReader
pub fun thrift_read_field(r: &mut ThriftReader): Option<ThriftField>
pub fun thrift_read_bool(r: &mut ThriftReader, value: Bool): Option<Bool>
pub fun thrift_read_i32(r: &mut ThriftReader): Option<Int64>
pub fun thrift_read_i64(r: &mut ThriftReader): Option<Int64>
pub fun thrift_read_double(r: &mut ThriftReader): Option<Float64>
pub fun thrift_read_binary(r: &mut ThriftReader): Option<String>
pub fun thrift_read_list_header(r: &mut ThriftReader): Option<ThriftList>
pub fun thrift_skip(r: &mut ThriftReader, kind: Int64): Bool
```

`ThriftList` is a small struct with element kind and count. `ThriftWriter`
and the `thrift_write_*` helpers are added in P5B.

#### `stdlib/snappy.valvet`

Module: `std.snappy`.

```valvet
pub fun snappy_decompress(src: String, expected_len: Int64): Result<String, String>
```

Raw Snappy block format only. Support literal tags and copy tags. Reject
negative or over-cap `expected_len`, truncated input, invalid offsets, and
output growth past the cap. Build the decoded result with the P5A
`bytes_to_string` builtin. Writer support is not needed; the Parquet writer
emits uncompressed pages.

#### `stdlib/flatbuf.valvet`

Module: `std.flatbuf`.

Minimal FlatBuffers reader for the Arrow IPC file tables. Writer helpers are
added in P5B.

Public functions:

```valvet
pub struct FbTable { data: String, pos: Int64 }
pub fun fb_root(s: &String): Option<FbTable>
pub fun fb_field(t: &FbTable, field: Int64): Option<Int64>
pub fun fb_i64(t: &FbTable, field: Int64): Option<Int64>
pub fun fb_i32(t: &FbTable, field: Int64): Option<Int64>
pub fun fb_u8(t: &FbTable, field: Int64): Option<Int64>
pub fun fb_string(t: &FbTable, field: Int64): Option<String>
pub fun fb_vector_len(t: &FbTable, field: Int64): Option<Int64>
pub fun fb_vector_offset(t: &FbTable, field: Int64, index: Int64): Option<Int64>
```

Reader helpers are checked and non-recursive. Writer helpers are added in
P5B.

#### `stdlib/parquet.valvet`

Module: `std.parquet`.

```valvet
pub fun read_parquet(path: String): Result<TypedFrame, String>
pub fun read_parquet_schema(path: String): Result<Vec<String>, String>
```

Supported subset:

- Parquet magic `PAR1`, footer metadata, one or more row groups.
- Primitive types: INT32, INT64, DOUBLE, BOOLEAN, BYTE_ARRAY with UTF8.
- Required and optional fields. Optional uses definition level 0 for
  missing. No repetition levels.
- PLAIN encoding only.
- DATA_PAGE v1 only. Reject DATA_PAGE_V2, dictionary pages, delta encodings,
  and nested or repeated schemas with a clear `Err`.
- Compression: UNCOMPRESSED and SNAPPY.
- No statistics, bloom filters, encryption, or page indexes.

Type mapping: INT32 and INT64 to `Column::Int`, DOUBLE to `Column::Float`,
BOOLEAN to `Column::Bool`, UTF8 BYTE_ARRAY to `Column::Str`. Duplicate
column names are `Err`.

Caps: `PARQUET_MAX_FILE_BYTES = 268435456`,
`PARQUET_MAX_ROWS = 268435456`, `PARQUET_MAX_COLUMNS = 4096`,
`PARQUET_MAX_PAGE_BYTES = 67108864`. All offsets, lengths, and counts use
checked arithmetic.

#### `stdlib/arrow.valvet`

Module: `std.arrow`.

```valvet
pub fun read_arrow(path: String): Result<TypedFrame, String>
pub fun read_arrow_schema(path: String): Result<Vec<String>, String>
```

Supported subset:

- Arrow IPC file format only, magic `ARROW1`, footer, one record batch.
- Primitive types: Int64, Float64, Bool, Utf8. Int32 maps to `Column::Int`.
- Validity bitmap for missing values.
- No dictionary encoding, nested types, unions, compression, or stream
  format.
- Duplicate field names are `Err`.

Caps: `ARROW_MAX_FILE_BYTES = 268435456`,
`ARROW_MAX_ROWS = 268435456`, `ARROW_MAX_COLUMNS = 4096`.

#### Fixture generators and data

New scripts, standard library only and deterministic:

- `scripts/gen_parquet_fixtures.py`
- `scripts/gen_arrow_fixtures.py`

New data directory:

- `valvet-tests/data/README.md`: provenance, format subset, SHA256, and
  expected schema for every binary fixture.
- `valvet-tests/data/parquet/plain_basic.parquet`
- `valvet-tests/data/parquet/plain_missing.parquet`
- `valvet-tests/data/parquet/snappy_basic.parquet`
- `valvet-tests/data/parquet/nested.parquet` (unsupported)
- `valvet-tests/data/parquet/dictionary.parquet` (unsupported)
- `valvet-tests/data/parquet/truncated.parquet` (malformed)
- `valvet-tests/data/parquet/bad_offset.parquet` (malformed)
- `valvet-tests/data/arrow/basic.arrow`
- `valvet-tests/data/arrow/missing.arrow`
- `valvet-tests/data/arrow/unsupported.arrow` (unsupported)
- `valvet-tests/data/arrow/truncated.arrow` (malformed)

Fixtures read these paths relative to the repository root, which is the
working directory used by `valvet test` and `valvet test --doc`.

### 5.3 Modified files

#### `crates/valvet_core/src/sema.rs`

Add the `read_file_limit(path: String, max_bytes: Int64):
Result<String, String>` builtin:

- Arity 2.
- Path is `String`; max is `Int64`.
- Negative max is a compile-time or runtime `Err`; use the existing
  diagnostic style for a non-Int argument.
- Hermetic runs deny it with the same message family as `read_file`.
- Record `Result<String, String>`.

Also add `bytes_to_string(v: Vec<Int64>): Result<String, String>`:

- Arity 1.
- Argument is `Vec<Int64>`.
- Every element must be in `0..255`; otherwise `Err`.
- The result is a length-aware `VStr` with exact bytes, including NUL.
- Record `Vec<Int64>` and `Result<String, String>`.

#### `crates/valvet_core/src/codegen.rs`

Add the C helper and lowering for `read_file_limit`:

- `v_read_file_limit` opens in binary mode, seeks to end, checks size against
  `max_bytes` before allocation, rejects negative size, and returns the
  existing `VFileOut` shape.
- Reuse the existing `v_read_file` error strings where possible.
- Emit the wrapper only when `Result<String, String>` is recorded.

Add the C helper and lowering for `bytes_to_string`:

- Allocate `len + 1`, copy each checked byte, set the terminator, and return
  the existing `Result<String, String>` shape.
- Reject a byte outside `0..255` before allocation.
- Preserve NUL bytes through the explicit length.

#### `crates/valvet_core/tests/front_end.rs`

Add tests:

- `read_file_limit_arity_and_types`
- `read_file_limit_hermetic_denied`
- `read_file_limit_records_result`
- `bytes_to_string_arity_and_types`
- `bytes_to_string_records_result`

#### `crates/valvet_cli/tests/security.rs`

Add a local, non-destructive test that a limit larger than the file succeeds
and a smaller limit returns `Err` without reading the file into memory. Use
a temporary file under the approved temp directory.

#### Documentation

- `README.md`: add Parquet and Arrow read examples and the supported subset.
- `docs/implementation/ARCHITECTURE.md`: add the binary I/O modules and the
  no-dependency format subset.
- `docs/implementation/TEST_STRATEGY.md`: add binary fixture data files,
  malformed-input fixtures, and sanitizer/fuzz gates.
- `docs/security/PRELIMINARY_SECURITY_ASSESSMENT.md`: add a binary parser
  section covering bounds checks, allocation caps, Thrift depth, FlatBuffers
  vector caps, Snappy output caps, and malformed-input tests. Reference
  VLV-018.
- `docs/implementation/IMPLEMENTATION_STATUS.md`: add P5A status only after
  the phase lands.

### 5.4 Fixtures

| Fixture | Oracle |
| --- | --- |
| `valvet-tests/compile-pass/bytes_basic.valvet` | Little-endian reads, varint, zigzag, bit unpack, bounds failures. |
| `valvet-tests/compile-pass/bytes_to_string_basic.valvet` | ASCII, NUL, high bytes, and invalid byte errors. |
| `valvet-tests/compile-pass/bytes_to_string_roundtrip.valvet` | `read_file` after `write_file` returns the exact byte length. |
| `valvet-tests/compile-pass/snappy_literal.valvet` | Literal-only block decodes to the expected bytes. |
| `valvet-tests/compile-pass/snappy_copy.valvet` | Copy tags, overlapping copies, and output cap. |
| `valvet-tests/compile-pass/snappy_truncated.valvet` | Truncated and invalid blocks return `Err`. |
| `valvet-tests/compile-pass/parquet_read_basic.valvet` | Plain basic file; names, types, rows. |
| `valvet-tests/compile-pass/parquet_read_missing.valvet` | Optional columns and definition levels. |
| `valvet-tests/compile-pass/parquet_read_snappy.valvet` | Snappy page decodes identically to the plain oracle. |
| `valvet-tests/compile-pass/parquet_reject_nested.valvet` | Nested schema returns `Err`. |
| `valvet-tests/compile-pass/parquet_reject_dictionary.valvet` | Dictionary page returns `Err`. |
| `valvet-tests/compile-pass/parquet_reject_truncated.valvet` | Truncated footer or page returns `Err`. |
| `valvet-tests/compile-pass/parquet_reject_bad_offset.valvet` | Out-of-range offset returns `Err`. |
| `valvet-tests/compile-pass/arrow_read_basic.valvet` | File format, primitive columns, rows. |
| `valvet-tests/compile-pass/arrow_read_missing.valvet` | Validity bitmap and missing values. |
| `valvet-tests/compile-pass/arrow_reject_unsupported.valvet` | Dictionary or nested field returns `Err`. |
| `valvet-tests/compile-pass/arrow_reject_truncated.valvet` | Truncated footer or batch returns `Err`. |
| `examples/parquet_read.valvet` | README-style Parquet read example. |
| `examples/arrow_read.valvet` | README-style Arrow read example. |

Oracle: the generator scripts emit a small JSON or CSV sidecar with expected
rows and schema; fixtures print the same values. The generators use only the
Python standard library. Do not add `pyarrow`, `pandas`, or `fastparquet` as
runtime dependencies.

### 5.5 Fuzz and security gates

- Add `scripts/fuzz_binary_formats.py` (new) that mutates the binary
  fixtures, writes temporary files, runs a small Valvet probe, and checks
  for a clean `Err` rather than a signal, hang, or sanitizer report.
- Run the probe under ASan/UBSan/LSan with a per-case timeout.
- Reject inputs over the file, row, column, page, and output caps.
- Reject negative or overflowing lengths and offsets.
- Reject Thrift nesting deeper than 32.
- Reject FlatBuffers vectors whose element count exceeds the remaining
  buffer.
- Reject Snappy output that exceeds the declared length or cap.
- No unbounded recursion in metadata parsers.

### 5.6 Gates

All P1 gates, plus:

- `cargo test --workspace` with the new front-end tests.
- `target/debug/valvet test valvet-tests` and release.
- ASan/UBSan/LSan on every binary fixture, including malformed inputs.
- Fuzz smoke on the binary fixtures.
- No new dependency in `Cargo.toml`.

### 5.7 Risks

- Parquet and Arrow are large formats. The narrow subset is mandatory; do not
  grow it silently.
- Malformed metadata is the main security risk. Every offset and length must
  be checked before use.
- `read_file_limit` is new compiler surface. Keep it small and tested.
- Data files are binary. Keep them deterministic, documented, and generated
  by checked-in scripts.

## 6. P5B: Parquet and Arrow writers

### 6.1 Goal

Write a `TypedFrame` to the same narrow Parquet and Arrow subsets, and read
it back without loss for supported types.

### 6.2 Core change

None. P5B uses the `bytes_to_string` builtin added in P5A. The writer path
must not add another compiler builtin.

### 6.3 Modified files

- `stdlib/bytes.valvet`: add `bytes_put_u8`, `bytes_put_u32_le`,
  `bytes_put_i64_le`, `bytes_put_varint`, `bytes_put_zigzag`, and
  `bytes_finish`. `bytes_finish` calls the P5A `bytes_to_string` builtin.
- `stdlib/thrift.valvet`: add `ThriftWriter` and the `thrift_write_*`
  helpers used by Parquet metadata.
- `stdlib/flatbuf.valvet`: add `fb_finish_table` and the writer helpers used
  by Arrow metadata.
- `stdlib/parquet.valvet`: add `write_parquet(f: &TypedFrame, path: String):
  Result<Int64, String>`.
- `stdlib/arrow.valvet`: add `write_arrow(f: &TypedFrame, path: String):
  Result<Int64, String>`.
- `README.md`, `docs/implementation/IMPLEMENTATION_STATUS.md`,
  `docs/implementation/ARCHITECTURE.md`: add writer support and limits.
- `docs/security/PRELIMINARY_SECURITY_ASSESSMENT.md`: add the checked
  bytes-to-string path and the writer's size caps.

### 6.4 Writer semantics

Parquet writer:

- One row group, one column chunk per column.
- PLAIN encoding, uncompressed pages.
- Required columns for non-missing data; optional columns when the column has
  missing values. A mixed column is optional and uses definition levels.
- Types: Int, Float, Bool, Str. Factor is written as Str and documented.
- No statistics, dictionary, encryption, or page index.

Arrow writer:

- File format, one record batch.
- Int64, Float64, Bool, Utf8. Factor is written as Utf8.
- Validity bitmap for missing values.
- No dictionary, nested types, compression, or stream format.

Both writers return `Err` for write failures and never panic on a supported
column. Both use `bytes_to_string` and `write_file`, and both check every
offset and length.

### 6.5 Fixtures

| Fixture | Oracle |
| --- | --- |
| `valvet-tests/compile-pass/parquet_roundtrip.valvet` | Write and read back Int, Float, Bool, Str, and missing. |
| `valvet-tests/compile-pass/parquet_roundtrip_factor.valvet` | Factor is written as Str and read back as a string column. |
| `valvet-tests/compile-pass/parquet_write_bad_path.valvet` | A failed write returns `Err`. |
| `valvet-tests/compile-pass/arrow_roundtrip.valvet` | Write and read back Int, Float, Bool, Str, and missing. |
| `valvet-tests/compile-pass/arrow_roundtrip_factor.valvet` | Factor is written as Utf8 and read back as a string column. |
| `valvet-tests/compile-pass/arrow_write_bad_path.valvet` | A failed write returns `Err`. |
| `examples/parquet_write.valvet` | README-style writer example. |
| `examples/arrow_write.valvet` | README-style writer example. |

### 6.6 Gates

All P5A gates, plus ASan/UBSan/LSan on both round trips and the writer
rejection fixtures. Verify that written files are readable by the P5A reader
and that a second read produces identical typed values.

### 6.7 Risks

- `bytes_to_string` is a new unsafe-adjacent surface. Reject out-of-range
  bytes and keep the length explicit.
- Writer metadata must be deterministic. Do not include timestamps or random
  IDs.
- Factor is written as string. Document the loss of level metadata.

## 7. P6: lazy plans

### 7.1 Goal

Build a plan without reading data, then execute it with `lazy_collect`.
Plans are flat lists of steps, not recursive enums, because Valvet has no
`Box` and rejects recursive types.

### 7.2 New file

#### `stdlib/lazy.valvet`

Module: `std.lazy`.

Imports:

```valvet
import std::arrow::read_arrow;
import std::grouped::AggSpec;
import std::grouped::tf_group_by;
import std::grouped::tf_summarise;
import std::parquet::read_parquet;
import std::typed_csv::read_csv_typed;
import std::typed_csv::read_csv_typed_infer;
import std::typed_frame::SortKey;
import std::typed_frame::TypedFrame;
import std::typed_frame::tf_arrange_keys;
import std::typed_frame::tf_filter_mask;
import std::typed_frame::tf_select;
import std::typed_frame::tf_take_rows;
import std::typed_join::tf_join_inner;
```

Public types and functions:

```valvet
pub enum LazySource {
    Csv(String, Vec<String>, Bool),
    CsvInfer(String, Bool),
    Parquet(String),
    Arrow(String),
    Memory(TypedFrame),
}

pub struct FilterTerm {
    col: String,
    op: String,
    value: String,
}

pub enum PlanStep {
    Select(Vec<String>),
    FilterAll(Vec<FilterTerm>),
    Summarise(Vec<String>, Vec<AggSpec>),
    JoinInner(TypedFrame, Vec<String>, Vec<String>),
    Arrange(Vec<SortKey>),
    Limit(Int64),
}

pub struct LazyFrame {
    source: LazySource,
    steps: Vec<PlanStep>,
}

pub fun lazy_csv(path: String, schema: Vec<String>, strict: Bool): LazyFrame
pub fun lazy_csv_infer(path: String, strict: Bool): LazyFrame
pub fun lazy_parquet(path: String): LazyFrame
pub fun lazy_arrow(path: String): LazyFrame
pub fun lazy_from_frame(f: TypedFrame): LazyFrame

pub fun lazy_select(f: LazyFrame, cols: Vec<String>): LazyFrame
pub fun lazy_filter_all(f: LazyFrame, terms: Vec<FilterTerm>): LazyFrame
pub fun lazy_summarise(f: LazyFrame, keys: Vec<String>, specs: Vec<AggSpec>): LazyFrame
pub fun lazy_join_inner(f: LazyFrame, lkeys: Vec<String>, right: TypedFrame, rkeys: Vec<String>): LazyFrame
pub fun lazy_arrange(f: LazyFrame, keys: Vec<SortKey>): LazyFrame
pub fun lazy_limit(f: LazyFrame, n: Int64): LazyFrame

pub fun lazy_optimize(f: LazyFrame): LazyFrame
pub fun lazy_collect(f: LazyFrame): Result<TypedFrame, String>
pub fun lazy_explain(f: &LazyFrame): Vec<String>
```

### 7.3 Semantics

- Construction never reads a file. `lazy_csv`, `lazy_parquet`, and
  `lazy_arrow` only store a source description.
- `lazy_collect` loads the source, then applies steps in order.
- `Select` uses `tf_select`.
- `FilterAll` is a conjunction of terms. Supported operators are `=`, `!=`,
  `<`, `<=`, `>`, and `>=`. Values are parsed according to the column type.
  Missing comparisons are false. String comparisons are bytewise. A bad
  column or value returns `Err`.
- `Summarise` calls `tf_group_by` and `tf_summarise`.
- `JoinInner` uses a materialized right `TypedFrame`. Lazy-lazy joins are
  deferred because they need a DAG or a recursive plan.
- `Arrange` uses `tf_arrange_keys`.
- `Limit` uses `tf_take_rows`; negative limits become 0.
- `lazy_explain` returns deterministic strings for the source and each step.
- `lazy_optimize` applies only these rules:
  1. Merge adjacent `Select` steps, keeping the later select order and the
     intersection of columns.
  2. Drop a `Select` that keeps every known source column. If the source
     schema is inferred and unknown, leave the select in place.
  3. Move `Limit` before `Select`.
  4. Replace `Limit(0)` with an empty frame at collect time.
  5. Do not push filters into CSV, Parquet, or Arrow readers in v1.

### 7.4 Modified files

- `stdlib/typed_frame.valvet`: add `tf_arrange_keys` if not already present.
- `stdlib/grouped.valvet`: add any missing accessor used by `lazy_collect`.
- `README.md`, `docs/implementation/IMPLEMENTATION_STATUS.md`: add lazy plan
  examples and status.
- `docs/implementation/ARCHITECTURE.md`: add the flat plan model and explain
  why recursive plan enums are not used.
- `docs/implementation/TEST_STRATEGY.md`: add laziness and plan-explain
  tests.

### 7.5 Fixtures

| Fixture | Oracle |
| --- | --- |
| `valvet-tests/compile-pass/lazy_plan_basic.valvet` | Source plus select, filter, arrange, limit; compare to eager pipeline. |
| `valvet-tests/compile-pass/lazy_plan_lazy_io.valvet` | Construct with a missing path, print `built`, then collect and print `Err`. Proves no eager read. |
| `valvet-tests/compile-pass/lazy_plan_select_limit.valvet` | Adjacent select merge and limit-before-select rule. |
| `valvet-tests/compile-pass/lazy_plan_summarise.valvet` | Lazy group and summarise equals eager typed result. |
| `valvet-tests/compile-pass/lazy_plan_join.valvet` | Lazy inner join equals eager typed join. |
| `valvet-tests/compile-pass/lazy_plan_errors.valvet` | Bad source, bad column, bad filter value, and bad limit. |
| `valvet-tests/compile-pass/lazy_plan_explain.valvet` | Deterministic explain output before and after optimize. |
| `valvet-tests/compile-pass/lazy_plan_empty.valvet` | Empty source, empty select, and limit 0. |
| `examples/lazy_plan.valvet` | README-style lazy example. |

Oracle: compare each lazy fixture to the equivalent eager typed fixture
output. Laziness is proven by the missing-path construction test and by the
absence of file reads before `collect`.

### 7.6 Gates

All P1 gates, plus ASan/UBSan/LSan on the lazy fixtures. No core changes in
P6.

### 7.7 Risks

- A flat plan cannot represent arbitrary DAGs. Do not add a recursive
  `PlanStep` variant; it will be rejected by the type checker.
- `JoinInner` holds a materialized right frame. Document the memory cost.
- Optimizer rules must preserve semantics. When in doubt, leave the plan
  unchanged.
- Laziness is only observable through I/O. The missing-path fixture is the
  required proof.

## 8. Cross-phase test and release gates

Run after every phase:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
target/debug/valvet test valvet-tests
target/release/valvet test valvet-tests
target/debug/valvet test --doc
```

`valvet test` compiles generated C with release flags by default. The
`target/release/valvet` run checks the release CLI driver as well.

Sanitizers:

- Build generated C with `-O1 -g -fno-omit-frame-pointer
  -fsanitize=address,undefined`.
- `ASAN_OPTIONS=detect_leaks=1:halt_on_error=1`
- `UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1`
- Delete `target/valvet/<stem>` and `target/valvet/<stem>-*.hash` before a
  forced rebuild, otherwise the linker cache can reuse a non-sanitized
  executable when only `VALVET_CC` changes.
- Run every new fixture that owns or moves strings, columns, factors, or
  binary buffers.
- Run every malformed Parquet and Arrow fixture under the same settings.
- No signal, hang, sanitizer report, or leaked allocation.

Security:

- Malformed binary input must return `Err`, not trap or abort.
- Every file, row, column, page, vector, and output length is capped.
- Every offset and length uses checked arithmetic.
- No recursion in metadata parsers beyond the documented depth cap.
- No new dependency in `Cargo.toml` for P1 through P6.
- `read_file_limit` and `bytes_to_string` are the only new compiler
  builtins. Both are tested for arity, types, bounds, and hermetic behavior.

Documentation:

- `README.md` examples are runnable and covered by `valvet test --doc`.
- `docs/implementation/IMPLEMENTATION_STATUS.md` records test totals,
  supported subsets, and known limitations after each phase.
- `docs/implementation/ARCHITECTURE.md` records the new modules and the flat
  lazy plan.
- `docs/security/PRELIMINARY_SECURITY_ASSESSMENT.md` records binary parser
  risks and tests.
- `valvet-tests/data/README.md` records binary fixture provenance and
  SHA256 values.

Release discipline:

- No commit, tag, push, or publish without explicit instruction.
- Commit per phase only on request.
- Do not change existing fixture output to make a new phase pass.
- If a phase needs a semantic change to `DynamicFrame`, stop and record it
  as a separate decision.

## 9. Completion checklist

P1:

- [x] `TypedFrame`, `Column`, `ColBuild`, and key helpers exist.
- [x] Typed CSV strict and lenient loading work.
- [x] Stats interop and `tf_describe` work.
- [x] `DynamicFrame` round trip works.
- [x] P1 fixtures and ASan gates pass.

P2:

- [x] Multi-key `tf_group_index`, `tf_group_by`, and `tf_summarise` work.
- [x] Every `Agg` variant has a locked semantic test.
- [x] First-appearance order and missing groups are deterministic.
- [x] P2 fixtures and ASan gates pass.

P3:

- [x] Inner, left, right, and full typed joins work.
- [x] Multi-key, duplicate-key, missing-key, suffix, and audit tests pass.
- [x] Existing dynamic joins are unchanged.
- [x] P3 fixtures and ASan gates pass.

P4:

- [x] Lag, Lead, RowNum, Cumsum, and Rank work per partition.
- [x] Ordering, ties, missing values, and original row order are locked.
- [x] P4 fixtures and ASan gates pass.

P5A:

- [x] Parquet and Arrow readers cover the documented subset.
- [x] Malformed inputs return `Err` under sanitizers.
- [x] `read_file_limit` and `bytes_to_string` are implemented and tested.
- [x] No new dependency.
- [x] P5A fixtures, fuzz smoke, and ASan gates pass.

P5B:

- [x] Parquet and Arrow writers round trip supported types.
- [x] P5B fixtures and ASan gates pass.

P6:

- [x] Lazy construction performs no I/O.
- [x] `lazy_collect` matches eager typed pipelines.
- [x] `lazy_optimize` rules are deterministic and semantic-preserving.
- [x] P6 fixtures and ASan gates pass.

Completion evidence (2026-10-26): 246 Rust tests, 432/432 fixtures through
both debug and release drivers, 17/17 doc fences/examples, 266 deterministic
binary-mutation cases, and ASan/UBSan/LSan clean on the 88 typed/binary/lazy
fixtures plus the 10 earlier security regression fixtures.
