# Holt — Rust + LLVM Compiler

Holt is a small, statically-typed language (draft 0.1 EBNF in `references/ebnf-0.1.txt`) with `do … end` blocks, type-first declarations, and `has … end` for struct/class/trait/enum bodies. The compiler is written in Rust and lowers to LLVM via `inkwell`.

## Layout

```
holt/
├── holt/              # `holt` main binary — `holt build`/`holt run`
│   └── src/main.rs    # subcommands, status lines + single progress bar, delegates to `compiler` lib
├── compiler/          # `compiler` library + legacy `holtc` bin (lexer, parser, sema, codegen)
│   └── src/           # token.rs, lexer.rs, ast.rs, parse/, sema/, codegen/, lib.rs
├── examples/          # .hlt programs (see below)
├── stdlib/std/io.hlt  # pure-Holt standard library (import std::io)
└── references/        # EBNF, phases, llvm-mapping, toolchain
```

## Prerequisites

- Rust (edition 2024)
- LLVM 21 (`llvm-config --version` → 21.x, `inkwell` feature `llvm21-1`)
- `clang` for linking (Xcode CLT on macOS)

## Build & Run

```sh
cargo build
cargo run -p holt -- build examples/hello_io.hlt        # status lines + progress bar
cargo run -p holt -- run examples/hello_io.hlt          # build, run, delete binary
cargo run -p holt -- build examples/empty.hlt --help    # build subcommand
cargo run -p compiler -- examples/hello_io.hlt         # legacy direct driver
cargo test
```

`holt build <file>` is the main entry point (workspace member `holt`). It shows a
single progress bar covering the whole compile plus static status lines on
stderr with a right-aligned brand-green (#00A693) prefix: `Compiling` …
`Checking`/`Checked` … `Compiling <obj>` … `Compiled <src> → <exe> in 0.12s`.
`--verbose` adds per-phase lines, `--quiet` silences everything but errors.

The driver lexes → parses → resolves imports → type-checks → emits LLVM IR → writes a `.o` via `TargetMachine` → links with `clang` to produce a proper binary (extension stripped, no `.out` suffix).

## Examples

Moved out of `compiler/examples/` into top-level `examples/` — fewer, more comprehensive (now 7):

| File | Covers |
|------|--------|
| `empty.hlt` | Phase 0 empty file |
| `basics.hlt` | Phase 1: `int`/`bool`/`void`, arithmetic, `and`/`or`/`not`, `if`/`else`, `while`, functions, recursion |
| `data_control.hlt` | Phase 2–3: `struct` (`public`/`private` + `= expr` default, `User u2 = has … end` omitted `Type`), field access, literals, `T[]`, `match` (`|`/`or` + `(a,b)` tuple), `string`, `break`/`continue`, `loop`, `for … in`, `defer` |
| `abstraction.hlt` | Phase 4: `class` + `this`, constructors (`initialize` sugar), `open` (class only) / `override` / `sealed`, `trait` + `implements`, `enum` with payloads, `get`/`set` properties (public by default, separate allowed), `public`/`private` |
| `hello_io.hlt` | `import std::io` (`print`/`println`/`printInt`/`putChar`) |
| `advanced.hlt` | Phase 5: `distinct`/`typedef`, `extend` (`open class` + `extend` `field`/`operator`/`property`/`conversion`), `init`, `extern` (`struct`/`enum`/`const` + `extern "c" printf`), generics + `where`, `operator`/`convert`, closures (`\|…\|`), string interpolation (`{expr}`), `float`/`double`, `any`, plus `struct User` omitted `has` and `CounterEx` separate accessors |
| `variadic.hlt` | T-14: variadic `...` (`...int vda`→`int[]`, `...T vda` `where`, `... vda` derived last, `...string vda, bool cond` middle), generics + `extern` `...` |

```sh
cargo run -p holt -- build examples/basics.hlt && ./examples/basics.out; echo $?
cargo run -p holt -- build examples/abstraction.hlt && ./examples/abstraction.out; echo $?
# legacy:
cargo run -p compiler -- examples/basics.hlt && ./examples/basics.out; echo $?
```

## Language Notes (do not ignore)

- Blocks: `do … end` (not `{}`)
- Decls: `int x = 1` (no `let`)
- Terminators: newline or `;`
- Bodies: `has … end` for struct/class/enum/trait
- Literals: `Type has field = expr end` and, for struct or constructor-less class, `has field = expr end` with inferred `Type` (e.g. `User u2 = has name = "bbb" end` when `User u2` declares `User`)
- Match: `match expr do pat -> expr end`
- `defer` runs on every scope exit
- `initialize` only on class constructors; `this.field = field` sugar and optional `do … end` block
- Constructors and `get`/`set` accessors are `public` by default; other class members are `private` unless `public`. `get`/`set` may be declared together or separately (`int x get … end` + `int x set … end`) and `open` is only for `class`, not methods

Spec is authoritative: `references/ebnf-0.1.txt`. Phases: `references/phases.md`. LLVM mapping: `references/llvm-mapping.md`.

## Status

Phase 5 (Advanced) is complete — all language features lower to LLVM and are exercised by `advanced.hlt` (generics, closures, interpolation, operators, `extern` `struct`/`enum`/`const`, `distinct` etc., exit 20), `variadic.hlt` (T-14 `...`), `data_control.hlt` (T-15 `struct` visibility/default, T-17 `match` `|`/`or`/`(a,b)`), `abstraction.hlt` (T-15 `class` fields, T-20 `extend` `field`/`operator`/`property`), `hello_io.hlt` (T-21 `import std::io`), `basics.hlt` (T-1..T-4) and `empty.hlt`. Earlier phases: `abstraction.hlt` (exit 233), `basics.hlt` (exit 230), `data_control.hlt` (exit 72), `variadic.hlt` (exit 0), `hello_io.hlt` (exit 0).
