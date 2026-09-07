---
name: holt-stdlib
description: Standard library for the Holt programming language — pure Holt sources, import semantics, and phased IO-first implementation. Use when the user asks about stdlib, `import std::`, runtime, or wants to add built-in library functions.
---

# Holt Standard Library

Guide for building `stdlib` for Holt (EBNF draft 0.1).

## Core Decisions (locked)

1. **Pure Holt sources** — stdlib is Holt code under `stdlib/` (e.g. `stdlib/std/io.hlt`), not hand-written LLVM IR. Runtime `extern "c"` helpers live in `runtime/` only when Holt cannot express an operation.
2. **Import semantics confirmed — EBNF §32** — `import qualified-name [:: "{" import-list "}"] statement-terminator` is canonical. Qualified path `a::b::c` maps to `stdlib/a/b/c.hlt`. No alternate syntax.
3. **Go with proposal** — phased approach from `references/phases.md` + LLVM mapping (`references/llvm-mapping.md`). Stdlib blocks on Phase 5 capabilities (generics, FFI) without inventing workarounds.
4. **IO-first minimal** — this iteration ships only a handful of IO functions. No collections / strings / math until compiler stabilizes.

## Layout

```
stdlib/
  std/
    io.hlt        # print, println, printInt, putChar (pure Holt + extern)
  README.md
runtime/          # optional tiny C/Rust helpers linked via clang (future: when extern §36 lands)
  io.c            # fallback if Holt cannot directly call libc
compiler/src/
  ast.rs          # Item::Import
  parse/mod.rs    # parse_import
  main.rs         # resolver: qualified-name → file, inline items before sema
  codegen/mod.rs  # declare external puts/printf, intrinsify std::io fns or lower extern
```

## EBNF §32 Contract

```
import-declaration =
    "import" , qualified-name , [ "::" , "{" , import-list , "}" ] , statement-terminator ;

import-list = identifier , { "," , identifier } ;
qualified-name = identifier , { "::" , identifier } ;
```

* `import std::io` → whole module.
* `import std::io::{print, println}` → selected symbols (resolver filters; sema still sees only selected).
* Resolver searches `stdlib/<path>.hlt` relative to workspace root, then relative to importer file. No `import "./foo"` custom syntax.

## Current Phase (minimal IO)

**Shipped (`stdlib/std/io.hlt`):**
- `void print(string s)` — no newline, via `puts`/`printf`
- `void println(string s)` — with newline
- `void printInt(int n)` — decimal, via `printf("%ld")`
- `void putChar(char c)` — optional, via `putchar`

**Not yet:** file IO, `readLine`, `eprint`, formatting/interpolation, buffering. Deferred until Phase 5 FFI (`extern "c" from "stdio.h"`) is stable.

Implementation note: Holt `string` is currently `ptr (i8*)` null-terminated in `codegen/mod.rs`. Stdlib IO declares `extern` libc `puts`/`printf`/`putchar` when `extern` §36 parser lands; until then codegen intrinsifies `print` family by declaring those symbols as LLVM declarations and emitting calls directly (no Holt `extern` needed for this iteration).

## Workflow When Extending Stdlib

1. Add `stdlib/std/<mod>.hlt` as pure Holt (type-first `int x`, `do…end`, `has…end`).
2. If new module needs FFI, add `extern` block per EBNF §36 and corresponding `runtime/*.c`.
3. Update `parse` for any new syntax the module uses, then `sema` (name resolution), then `codegen` (LLVM lowering + `module.verify()`).
4. Add `compiler/examples/stdlib_<mod>.hlt` that `import std::<mod>` and is compiled+linked as regression.
5. Keep `stdlib/README.md` symbol index in sync.

## Testing

- Unit: `cargo run -- <example>.hlt` → `./example.out` and check stdout.
- Example: `compiler/examples/stdlib_io.hlt` imports `std::io` and calls each shipped function.
- CI: verify `module.verify()` never skipped before `write_to_file`.

## Anti-Patterns

- No Rust-implemented stdlib shims that bypass Holt source (except tiny `runtime/` C helpers).
- No `import` syntax outside EBNF §32.
- No stdlib feature that requires generics/traits/closures before Phase 5 monomorphization exists.
- No `{}` blocks or `let` — stdlib uses Holt surface (`do…end`, `int x = …`).

## Future Roadmap (deferred)

- Module resolver: caching, cycle detection, visibility `public`/`private` enforcement.
- String ABI switch to `{ptr,i64}` length struct (breaks current `ptr`).
- `extern` FFI (§36) → remove intrinsification, stdlib calls libc directly via `extern` decls.
- Generics → `std::vec`, `std::option`, `std::result`.
- Traits / extensions → `extend string do … end` helpers.

Base directory for this skill: /Users/rivethorn/Dev/Holt/holt-rs/.opencode/skills/stdlib
Relative paths (`stdlib/`, `compiler/`, `runtime/`) are relative to workspace root unless noted.
