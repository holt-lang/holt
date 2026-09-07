# Holt — Rust + LLVM Compiler

Holt is a small, statically-typed language (draft 0.1 EBNF in `references/ebnf-0.1.txt`) with `do … end` blocks, type-first declarations, and `has … end` for struct/class/trait/enum bodies. The compiler is written in Rust and lowers to LLVM via `inkwell`.

## Layout

```
holt-rs/
├── compiler/          # `holtc` crate (lexer, parser, sema, codegen)
│   └── src/           # token.rs, lexer.rs, ast.rs, parse/, sema/, codegen/
├── examples/          # .hlt programs (see below)
├── stdlib/std/io.hlt  # pure-Holt standard library (import std::io)
├── references/        # EBNF, phases, llvm-mapping, toolchain
└── holt-syntax.nvim/  # Neovim syntax
```

## Prerequisites

- Rust (edition 2024)
- LLVM 21 (`llvm-config --version` → 21.x, `inkwell` feature `llvm21-1`)
- `clang` for linking (Xcode CLT on macOS)

## Build & Run

```sh
cargo build
cargo run -p compiler -- examples/hello_io.hlt
cargo run -p compiler -- examples/abstraction.hlt --emit-llvm
cargo run -p compiler -- examples/empty.hlt --lex
cargo test
```

The driver lexes → parses → resolves imports → type-checks → emits LLVM IR → writes a `.o` via `TargetMachine` → links with `clang` to produce `*.out`.

## Examples

Moved out of `compiler/examples/` into top-level `examples/` — fewer, more comprehensive:

| File | Covers |
|------|--------|
| `empty.hlt` | Phase 0 empty file |
| `basics.hlt` | Phase 1: `int`/`bool`/`void`, arithmetic, `and`/`or`/`not`, `if`/`else`, `while`, functions, recursion |
| `data_control.hlt` | Phase 2–3: `struct`, field access, literals, `T[]`, `match`, `string`, `break`/`continue`, `loop`, `for … in`, `defer` |
| `abstraction.hlt` | Phase 4: `class` + `this`, constructors (`initialize` sugar), `open`/`override`/`sealed`, `trait` + `implements` (no `override` needed), `enum` with payloads, `get`/`set` properties, `public`/`private` (constructors public by default) |
| `hello_io.hlt` | `import std::io` (`print`/`println`/`printInt`/`putChar`) |

```sh
cargo run -p compiler -- examples/basics.hlt && ./examples/basics.out; echo $?
cargo run -p compiler -- examples/abstraction.hlt && ./examples/abstraction.out; echo $?
```

## Language Notes (do not ignore)

- Blocks: `do … end` (not `{}`)
- Decls: `int x = 1` (no `let`)
- Terminators: newline or `;`
- Bodies: `has … end` for struct/class/enum/trait
- Literals: `Type has field = expr end`
- Match: `match expr do pat -> expr end`
- `defer` runs on every scope exit
- `initialize` only on class constructors; `this.field = field` sugar and optional `do … end` block
- Constructors are `public` by default, all other class members are `private` unless `public`

Spec is authoritative: `references/ebnf-0.1.txt`. Phases: `references/phases.md`. LLVM mapping: `references/llvm-mapping.md`.

## Status

Phase 4 (Abstraction) is complete — classes, constructors, inheritance, traits, enums, properties, visibility all lower to LLVM and are exercised by `abstraction.hlt` (exit 233).
