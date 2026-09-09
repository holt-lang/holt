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
    io.hlt        # print println printInt putChar eprint eprintln readLine readInt (pure Holt + extern)
    types.hlt     # doc-only manifest of the implicit environment
  README.md
runtime/          # optional tiny C/Rust helpers linked via clang (future: when Holt cannot express an operation)
compiler/src/
  ast.rs          # Item::Import
  parse/mod.rs    # parse_import
holt/src/main.rs  # resolver: qualified-name → file, inline items before sema
                  # (selective imports always carry the module's extern blocks)
compiler/src/codegen/mod.rs  # `declare_extern` lowering only (one decl per libc
                             # symbol, duplicates skipped); internal
                             # get_or_declare_* helpers serve compiler lowering
                             # (assert, interpolation), never user IO names
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

## Current Phase (real stdlib: IO owned by `stdlib/`)

**Shipped (`stdlib/std/io.hlt`) — pure Holt, no compiler intrinsics:**
- `void print(string s)` — no newline, via `extern i32 printf(string fmt, ...)`
- `void println(string s)` — with newline, via `extern i32 puts(string s)`
- `void printInt(int n)` — decimal, via `printf("%ld\n", n)`
- `void putChar(char c)` — single char, via `extern i32 putchar(char c)`
- `void eprint` / `void eprintln(string s)` — stderr via
  `extern int write(int fd, string buf, int count)` on fd 2 (no `FILE*`
  global, portable macOS/Linux)
- `string readLine()` — stdin line sans newline via `extern string calloc`
  + `extern int scanf(string fmt, ...)` (`"%255[^\n]%*c"`, 255-byte cap)
- `int readInt()` — stdin integer via `scanf("%ld%*c", out n)`

**Not yet:** file IO, formatting/interpolation helpers, buffering.
See `REAL_STDLIB.md` for the boundary rule.

Implementation note: Holt `string` is `ptr (i8*)` null-terminated in
`codegen/mod.rs`. `extern` §36 is stable, so stdlib declares libc directly and
the compiler lowers ordinary calls (plus `module.get_function` fallback for
`extern` callees). Sema has no `print`-family shortcut: without
`import std::io` these names are `undefined function` by design. See
complementary skill `holt-stdlib-real` (`REAL_STDLIB.md`) for the full
compiler-owns vs stdlib-owns contract.

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
- `extern` FFI (§36) → DONE for IO: stdlib declares libc directly via
  `extern` blocks; compiler only lowers them (no intrinsification).
  Remaining: `FILE*`/stderr plumbing for `eprint`, string-buffer append for `readLine`.
- Generics → `std::vec`, `std::option`, `std::result`.
- Traits / extensions → `extend string do … end` helpers.

Base directory for this skill: /Users/rivethorn/Dev/Holt/holt-rs/.opencode/skills/stdlib
Relative paths (`stdlib/`, `compiler/`, `runtime/`) are relative to workspace root unless noted.
