# Toolchain & Project Layout

## Crates

```toml
# crates/hella-compiler (library)
[dependencies]
inkwell = { version = "0.10", features = ["llvm21-1"] }  # matches llvm-config --version (21.x)
logos = "0.16.1"
miette = { version = "7", features = ["fancy"] }
thiserror = "2"
clap = { version = "4", features = ["derive"] }

# crates/hella-cli (package `hella`: main `hella build` / `hella run` CLI)
[dependencies]
hella-compiler = { path = "../hella-compiler" }
hella-lsp = { path = "../hella-lsp" }
clap = { version = "4", features = ["derive"] }
miette = { version = "7", features = ["fancy"] }
console = "0.16"
indicatif = "0.18.6"
inkwell = { version = "0.10", features = ["llvm21-1"] }

# crates/hella-lsp (library `hella_lsp`, LSP over stdio)
[dependencies]
hella-compiler = { path = "../hella-compiler" }
lsp-server = "0.10.0"
lsp-types = "0.97"
```

Optional later:

- `salsa` or hand-rolled query system (incremental)
- `codespan-reporting` as alternative diagnostics (not used — `miette` chosen)
- Parser is hand-rolled recursive-descent + Pratt (`crates/hella-compiler/src/parse/mod.rs:1`), not `chumsky`/`lalrpop` (removed)

## LLVM Setup

1. Install LLVM **development** packages (headers + `llvm-config`).
2. Confirm version: `llvm-config --version`.
3. Set the matching `inkwell` feature (`llvm18-1`, `llvm19-1`, `llvm20-1`, …).
4. If discovery fails: `export LLVM_SYS_XXX_PREFIX=/path/to/llvm`.

## Suggested Directory Layout (actual workspace as of Phase 5 DONE)

```
hella/
├── crates/
│   ├── hella-cli/           # `hella build`/`hella run` binary (clap subcommands, brand-green output, single progress bar)
│   │   ├── Cargo.toml      # package `hella`; hella-compiler/hella-lsp paths, console 0.16, indicatif, inkwell, miette, clap
│   │   ├── build.rs        # embeds `stdlib/**/*.hll` for `hella setup`
│   │   └── src/main.rs     # Commands::Build/Run/Check/Lsp/Setup/New, status() lines (brand #00A693, {:>11}) + one ProgressBar, Compiling/Checking/Checked/Compiling/Compiled/Running
│   ├── hella-compiler/      # compiler library (`hella_compiler`)
│   │   ├── Cargo.toml      # lib only, inkwell llvm21-1, logos, miette, thiserror, clap (no chumsky)
│   │   └── src/
│   │       ├── lib.rs        # pub mod ast/codegen/error/lexer/modules/parse/sema/token
│   │       ├── token.rs      # 93 keywords + EqEq diagnostic, From/Extend
│   │       ├── lexer.rs      # logos Logos, Newline/Semicolon terminators
│   │       ├── ast.rs        # mirrors EBNF Draft 0.2, Span on every node, Type::__inferred__ for omitted has
│   │       ├── parse/mod.rs  # 2919 lines, parse_program, try_parse_struct_literal (omitted has), parse_class_decl (operator/convert), parse_extension_decl
│   │       ├── sema/mod.rs   # 1861 lines, ClassInfo{operators,conversions}, prop_map merging, open-on-method rejection
│   │       ├── codegen/mod.rs# 2919 lines, llvm_ty_for, compile_program, class_operators dispatch, closure_count, defer stacks, hella.init
│   │       ├── modules.rs    # import resolution (`hella.toml` / `main.hll` project root, `~/.hella/lib`)
│   │       └── error.rs      # miette Single/MultiDiagnostic
│   └── hella-lsp/           # language server library (`hella_lsp`, LSP over stdio)
│       ├── Cargo.toml      # hella-compiler path, lsp-server, lsp-types, serde
│       └── src/            # server.rs, analysis.rs, diagnostics.rs, document.rs, lib.rs
├── examples/             # top-level .hll (basics, data_control with User omitted has, abstraction with separate accessors, advanced with all Phase 5, hello_io)
├── stdlib/std/io.hll     # pure-Hella stdlib (import std::io)
└── references/           # ebnf-0.1.txt Draft 0.2, phases.md DONE, llvm-mapping.md, toolchain.md
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

1. Parse CLI (`clap` `Commands::Build`/`Run`/`Check`/`Lsp`/`Setup`/`New` in `hella-cli`).
2. Read source.
3. Lex → Parse → Sema → Codegen with a single progress bar covering the whole pipeline plus static status lines on stderr (`crates/hella-cli/src/main.rs` `new_progress_bar()` + `status()` in brand green #00A693 `{:>11}` `Compiling/Checking/Checked/Compiling/Compiled/Running`; `--verbose` per-phase, `--quiet` silence).
4. On success: write object via `inkwell::targets::TargetMachine` + `clang` link to a proper binary (extension stripped; `run` rebuilds only when sources changed and keeps the binary) (or `--emit-llvm` to stdout/file, `--keep-obj`, `-o`).
5. On failure: print span-based `miette` diagnostics via the single `fail()` sink on stderr and exit non-zero.

## Testing Strategy

- Unit tests for lexer and pure parser functions.
- End-to-end `.hll` files under `examples/` or `tests/` that are compiled and run.
- Prefer checking observable behavior (return code, printed output) over snapshotting raw LLVM IR early on.
