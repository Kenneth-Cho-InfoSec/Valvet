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

## Ownership + borrowck (0.1.0)
Copy: Int32/Int64/Float64/Bool/Unit (+ shared `&T`). Owned (move on assign-arg-return, use-after-move E0301): String, `Vec<T>`, ADTs with owned content, `&mut T`. `var` reassignment frees old; `val` reassignment is E0205. References `&T`/`&mut T` with AST-level NLL: loans end at last use, field-precise overlap, exclusivity E0302, dangling returns E0303, moves-while-borrowed E0301. Scoped frees on all exits (incl. `return` in nested blocks).
