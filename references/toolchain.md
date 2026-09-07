# Toolchain & Project Layout

## Crates

```toml
[dependencies]
inkwell = { version = "0.10", features = ["llvm20-1"] }  # adjust to llvm-config --version
logos = "0.15"
chumsky = "0.10"          # alternative: lalrpop
ariadne = "0.5"           # or miette
thiserror = "2"
clap = { version = "4", features = ["derive"] }
```

Optional later:

- `salsa` or hand-rolled query system (incremental)
- `codespan-reporting` as alternative diagnostics

## LLVM Setup

1. Install LLVM **development** packages (headers + `llvm-config`).
2. Confirm version: `llvm-config --version`.
3. Set the matching `inkwell` feature (`llvm18-1`, `llvm19-1`, `llvm20-1`, …).
4. If discovery fails: `export LLVM_SYS_XXX_PREFIX=/path/to/llvm`.

## Suggested Directory Layout

```
compiler/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── token.rs
│   ├── lexer.rs
│   ├── ast.rs
│   ├── parse/
│   │   ├── mod.rs
│   │   ├── expr.rs      # Pratt / precedence climbing
│   │   ├── stmt.rs
│   │   └── decl.rs
│   ├── sema/
│   │   ├── mod.rs
│   │   ├── scope.rs
│   │   ├── types.rs
│   │   └── check.rs
│   ├── codegen/
│   │   ├── mod.rs
│   │   ├── context.rs
│   │   ├── expr.rs
│   │   ├── stmt.rs
│   │   └── decl.rs
│   └── error.rs
├── examples/            # .hlt programs
└── tests/
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

1. Parse CLI (`clap`).
2. Read source.
3. Lex → Parse → Sema → Codegen.
4. On success: JIT-execute or write object file and link.
5. On failure: print span-based diagnostics and exit non-zero.

## Testing Strategy

- Unit tests for lexer and pure parser functions.
- End-to-end `.hlt` files under `examples/` or `tests/` that are compiled and run.
- Prefer checking observable behavior (return code, printed output) over snapshotting raw LLVM IR early on.
