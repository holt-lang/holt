# Toolchain & Project Layout

## Crates

```toml
# compiler crate (library + legacy bin)
[dependencies]
inkwell = { version = "0.10", features = ["llvm21-1"] }  # matches llvm-config --version (21.x)
logos = "0.15"
miette = { version = "7", features = ["fancy"] }
thiserror = "2"
clap = { version = "4", features = ["derive"] }

# holt crate (main `holt build` CLI)
[dependencies]
compiler = { path = "../compiler" }
clap = { version = "4", features = ["derive"] }
miette = { version = "7", features = ["fancy"] }
indicatif = "0.17"
console = "0.15"
inkwell = { version = "0.10", features = ["llvm21-1"] }
```

Optional later:

- `salsa` or hand-rolled query system (incremental)
- `codespan-reporting` as alternative diagnostics (not used — `miette` chosen)
- Parser is hand-rolled recursive-descent + Pratt (`compiler/src/parse/mod.rs:1`), not `chumsky`/`lalrpop` (removed)

## LLVM Setup

1. Install LLVM **development** packages (headers + `llvm-config`).
2. Confirm version: `llvm-config --version`.
3. Set the matching `inkwell` feature (`llvm18-1`, `llvm19-1`, `llvm20-1`, …).
4. If discovery fails: `export LLVM_SYS_XXX_PREFIX=/path/to/llvm`.

## Suggested Directory Layout (actual workspace as of Phase 5 DONE)

```
holt-rs/
├── holt/                 # main binary `holt build` (clap Build subcommand, indicatif spinner, console styling)
│   ├── Cargo.toml        # compiler = {path="../compiler"}, indicatif, console, inkwell, miette, clap
│   └── src/main.rs       # Commands::Build, pb_spinner ("{spinner:.green} {msg}"), Compiling/Lexing/Parsing/Resolving/Checking/Codegen/Linking/Finished with Instant timing
├── compiler/             # library + legacy bin `compiler`/`holtc`
│   ├── Cargo.toml        # lib + bin, inkwell llvm21-1, logos, miette, thiserror, clap (no chumsky)
│   └── src/
│       ├── lib.rs        # pub mod ast/codegen/error/lexer/parse/sema/token (re-export for holt)
│       ├── main.rs       # legacy Args (lex/show_spans/emit_llvm/keep_obj/output/print_ast) — use `holt build` instead
│       ├── token.rs      # 93 keywords + EqEq diagnostic, From/Extend
│       ├── lexer.rs      # logos Logos, Newline/Semicolon terminators
│       ├── ast.rs        # mirrors EBNF Draft 0.2, Span on every node, Type::__inferred__ for omitted has
│       ├── parse/mod.rs  # 2919 lines, parse_program, try_parse_struct_literal (omitted has), parse_class_decl (operator/convert), parse_extension_decl
│       ├── sema/mod.rs   # 1861 lines, ClassInfo{operators,conversions}, prop_map merging, open-on-method rejection
│       ├── codegen/mod.rs# 2919 lines, llvm_ty_for, compile_program, class_operators dispatch, closure_count, defer stacks, holt.init
│       └── error.rs      # miette Single/MultiDiagnostic
├── examples/             # top-level .hlt (empty, basics, data_control with User omitted has, abstraction with separate accessors, advanced with all Phase 5, hello_io)
├── stdlib/std/io.hlt     # pure-Holt stdlib (import std::io)
├── references/           # ebnf-0.1.txt Draft 0.2, phases.md DONE, llvm-mapping.md, toolchain.md
└── holt-syntax.nvim/
```

## Parser Guidance

- **Expressions** — Pratt parser or precedence climbing. The EBNF already defines a clear precedence ladder (assignment → conditional → … → unary → postfix → primary).
- **Declarations & statements** — recursive descent.
- Statement terminators (newline or `;`) require either:
  - a token filter that inserts virtual semicolons, or
  - parser rules that accept both `Newline` and `;`.
- Keep comments and pure whitespace out of the token stream that the parser sees (or mark them trivia).

## AST Conventions

- Every node carries a `Span`.
- Use owned `String` or interned symbols for names.
- Prefer enums that mirror EBNF non-terminals (`Expr`, `Stmt`, `Item`, `Type`, …).
- Avoid premature optimization (arenas, interning) until the compiler works.

## Driver Responsibilities

1. Parse CLI (`clap` `Commands::Build` in `holt`, `Args` in legacy `compiler`).
2. Read source.
3. Lex → Parse → Sema → Codegen with progress like `cargo` (`holt/src/main.rs:40` `pb_spinner` `indicatif` + `console` `{:>12}` green bold `Compiling/Lexing/Parsing/Resolving/Checking/Codegen/Linking/Finished` + `Instant::now` timing).
4. On success: write object via `inkwell::targets::TargetMachine` + `clang` link to `*.out` (or `--emit-llvm` to stdout/file, `--keep-obj`, `-o`).
5. On failure: print span-based `miette` diagnostics and exit non-zero.
6. Legacy `compiler` bin (`compiler/src/main.rs:20` `holtc`) still works but `holt build` is canonical.

## Testing Strategy

- Unit tests for lexer and pure parser functions.
- End-to-end `.hlt` files under `examples/` or `tests/` that are compiled and run.
- Prefer checking observable behavior (return code, printed output) over snapshotting raw LLVM IR early on.
