# Architecture

```
.valvet → [lexer] → tokens → [parser] → AST (spans) → [sema: resolve+typecheck]
        → typed HIR → [ownership] → checked HIR → [codegen: C11] → cc → ELF
```
All stages emit `Diagnostic{file,line,col,code,message,help}`; user errors never panic the compiler.

## ADR-001: C11 emission + GCC backend (0.1.0)
- Considered: Inkwell/LLVM (no LLVM toolchain in env; heavy binding pin risk), Cranelift (no vendored crates, offline env).
- Chosen: handwritten C11 emitter + user-local GCC 16 (`~/.local/vgcc`, extracted from local Arch package; system binutils `ld`, system glibc). Verified: compiles+links clean binary.
- Valvet semantics live in AST→HIR→checks; codegen is a thin lowering. Backend swap (LLVM/Cranelift) later does not touch grammar/typing/ownership.
- Runtime: small embedded C prelude (`VStr` owned strings, print helpers); no GC; deterministic `vstr_free` at scope exit incl. early returns.

## Modules (single lib crate `valvet_core`, explicit boundaries)
`source` (file+span) → `diag` → `token`/`lexer` → `ast` → `parser` → `sema` (symbols, types, HIR) → `ownership` → `codegen` (HIR→C, runtime prelude, cc driver). CLI crate `valvet` maps check/build/run/test.

## Built-in containers
`Vec<T>` is one-dimensional. `Mat` is a row-major 2D `Float64` buffer. `v[i]` and `m[i, j]` copy the addressed element out; they never create a view. An indexed assignment writes the same `data[i * cols + j]` slot. `Vec` takes exactly one index and `Mat` exactly two; both check bounds row first, then column, and abort through the shared `index out of bounds` trap. Nested `Vec<Vec<T>>` stays jagged and is not a matrix.

`ArrayD<T>` is the runtime-rank N-dimensional container. It owns one row-major element buffer and stores `len`, `rank`, `shape[32]`, and `strides[32]` in elements. Rank is validated at runtime to `1..=32`; the fixed metadata arrays keep the C layout identical for every rank and avoid VLAs. `ArrayD<T>` is move-only, deep-clones with `.clone()`, and drops its elements then its buffer.

Construction is through `ArrayD<T>::from_vec(shape: Vec<Int64>, data: Vec<T>)`, `ArrayD<T>::filled(shape: Vec<Int64>, value: T)`, and numeric-only `ArrayD<T>::zeros(shape: Vec<Int64>)`. `from_vec` takes ownership of the data vector and steals its buffer; `filled` and `zeros` allocate a new buffer. Invalid rank, negative dimensions, shape-product or stride overflow, and a data length that does not match the shape abort with a diagnostic on stderr. `from_vec` and the shape methods use `Vec<Int64>` metadata.

Methods are `rank()`, `len()`, `dim(axis)`, `shape()`, `strides()`, `to_vec()`, and `clone()`. `shape()` and `strides()` return owned `Vec<Int64>` snapshots; `to_vec()` returns an owned row-major `Vec<T>` copy. Indexing accepts one index per axis, checks `indices.len() == rank` at runtime, then checks each axis against `shape[k]` and computes `sum(indices[k] * strides[k])`. Reads copy or clone the element out and writes assign the same slot.

`Array<T, N>` is the compile-time-rank form for `N` in `1..=8`. It is represented internally as `Ty::Generic { base: "ArrayN", args: [T] }` with the rank digit in the base (`Array3` for `Array<T, 3>`). It reuses the `ArrayD<T>` C layout and kernels through a typedef (`typedef VArrayD_Float64 VArray2_Float64;`) plus rank-specific wrappers. Sema checks the index count exactly, so indexing emits no runtime rank check. Constructors still take a runtime `Vec<Int64>` shape; the wrappers reject a shape whose length is not `N` with `array shape rank mismatch`, and the dimensions themselves remain runtime values. `rank()` returns `N` as an `Int64` constant. Drop, clone, `filled`, `zeros`, and `from_vec` wrappers keep generic struct, enum, and `Option` fields working. `Array<T, N>` is move-only and supports the same methods as `ArrayD<T>`.

`StringIndex` is an ordered string-keyed index with `Int64` positions. `StringIndex::new()` creates an empty index. Methods are `len()`, `contains(key)`, `get(key)`, `get_or_insert(key)`, `keys()`, and `clone()`. Keys are unique. `get` returns `Option<Int64>`; `get_or_insert` returns the existing position or appends the key and returns its new position. `keys()` returns a `Vec<String>` snapshot in first-appearance order. The index is move-only, borrows key arguments, clones a key only on insertion, and deep-clones with `.clone()`. Internally it stores insertion-ordered keys, cached FNV-1a hashes, and an open-addressed bucket table with a load factor of at most 0.5; key/hash storage and the bucket table grow independently. `StringIndex` is not a general `Map<K,V>`: deletion, non-string keys, map literals, `idx[key]`, printing the index itself, and join multimaps are out of scope.

Views, `slice`, `select`, stride-0 broadcasting, and borrowed array views are not implemented. `Mat` remains a separate 2D `Float64` type. It shares the `Vec<Float64>` runtime with arrays but has no implicit conversion or view; a program can use `Mat`, `Array<Float64, 2>`, and `ArrayD<Float64>` together.

## Typed frames, binary formats, and lazy plans (0.5.0)
`TypedFrame` is the columnar counterpart to `DynamicFrame`, not a
replacement. It stores `names: Vec<String>`, `cols: Vec<Column>`, and
`nrows: Int64`. `Column` is an enum over Int64, Float64, Bool, String, and
Factor. Primitive columns are `Vec<Missing<T>>`; Factor reuses the existing
`Factor` representation. The v1 layout deliberately favors simple,
move-only vectors over a validity bitmap. `ColBuild` is a per-type builder
struct because the emitter does not code-generate `&mut` enum matching
correctly. `tf_new` validates names, column lengths, and row count.

CSV and JSON schema parsing live in `std::schema`; `std::typed_csv` builds
typed frames through `CsvTable`. Strict loading rejects malformed rows,
lenient loading counts and bounds bad rows, and inference maps strings to
Int64, Float64, Bool, String, and Factor. `std::typed_stats` operates on
typed columns and returns typed summaries. `tf_to_dynamic` and
`tf_from_dynamic` are the explicit bridge to SQL, plots, validation, and
models.

Grouping (`std::grouped`) uses `StringIndex` only for local group-key
positions, preserving first-appearance order. A missing key is its own
group. `AggSpec` stores a name, aggregation kind, source column, and missing
policy; aggregation kinds are an enum. Joins (`std::typed_join`) encode each
key as a length-prefixed, type-tagged token string, never as a general
`StringIndex` multimap. Missing keys do not match by default, duplicate keys
produce cartesian rows, and duplicate output names receive deterministic
suffixes. Windows (`std::typed_window`) partition through the same group
index, compute per-partition values, and scatter them back by original row
id so the frame order never changes.

Lazy plans (`std::lazy`) are flat `Vec<PlanStep>` lists, not recursive
enums. A `LazyFrame` carries a source description, known columns, and a
`known` flag so `lazy_explain` and the optimizer remain first-order. Steps
cover select, filter, summarise, join to a materialized frame, arrange, and
limit. `lazy_collect` executes the plan; `lazy_optimize` removes redundant
selects, merges adjacent selects, and moves a limit ahead of a select.
Lazy-lazy joins are deferred.

Binary support is a narrow, standard-library-only implementation split
across `std::bytes`, `std::snappy`, `std::thrift`, `std::flatbuf`,
`std::parquet`, and `std::arrow`. Readers use the bounded `read_file_limit`
builtin and return `Result` errors for truncation, bad offsets, unsupported
encodings, and length overflow. The Parquet reader accepts plain,
uncompressed data pages and the supported Snappy subset. The Arrow reader
accepts primitive Int64, Float64, Bool, and Utf8 columns in a file-format
IPC stream. Nested, dictionary-encoded, and compressed Arrow inputs are
rejected. Writers emit one Parquet row group or one Arrow record batch,
with PLAIN/uncompressed Parquet and no Arrow compression or dictionaries.
Factor columns are written as strings by both writers.

## Ownership + borrowck (0.1.0)
Copy: Int32/Int64/Float64/Bool/Unit (+ shared `&T`). Owned (move on assign-arg-return, use-after-move E0301): String, `Vec<T>`, `Mat`, `ArrayD<T>`, `Array<T, N>`, `Factor`, `StringIndex`, ADTs with owned content, `&mut T`. `var` reassignment frees old; `val` reassignment is E0205. References `&T`/`&mut T` with AST-level NLL: loans end at last use, field-precise overlap, exclusivity E0302, dangling returns E0303, moves-while-borrowed E0301. Scoped frees on all exits (incl. `return` in nested blocks).
