---
name: holt-compiler
description: Build or extend a compiler for the Holt programming language in Rust with an LLVM backend (inkwell). Use when the user asks to implement, design, parse, typecheck, or codegen any part of Holt, references the Holt EBNF or language spec, or wants phased compiler construction for this language.
---

# Holt Compiler (Rust + LLVM)

Guide LLMs building a compiler for **Holt** (draft 0.1 EBNF) using Rust and `inkwell`.

## Core Principles

1. **Phase, do not boil the ocean.** Full Holt (classes, traits, generics, match, defer, FFI, interpolation, …) is large. Always implement a working subset that emits native code before expanding.
2. **Spec is authoritative.** The EBNF in `references/ebnf-0.1.txt` is the source of truth for syntax. Do not invent syntax that contradicts it.
3. **Spans everywhere.** Every AST node and diagnostic must carry source spans from day one.
4. **LLVM via inkwell.** Prefer `inkwell` over raw `llvm-sys`. Match the Cargo feature to the installed LLVM (`llvm-config --version`).
5. **Monomorphize generics early.** Do not build a full trait solver or lifetime system before a working monomorphizing backend exists.

## Required Reading

- Full language grammar — `references/ebnf-0.1.txt`
- Phased implementation plan — `references/phases.md`
- LLVM lowering map — `references/llvm-mapping.md`
- Recommended crates and project layout — `references/toolchain.md`

## Default Workflow

When asked to implement any part of the compiler:

1. Identify the smallest phase that contains the requested feature (see `references/phases.md`).
2. Confirm the relevant EBNF productions.
3. Produce or extend:
   - Token / lexer (if new surface syntax)
   - AST nodes
   - Parser rules
   - Semantic checks (names + types at minimum)
   - `inkwell` lowering
4. Prefer a compilable, testable increment over a complete but unrunnable design.
5. Emit either JIT (via `ExecutionEngine`) or object code + system linker for verification.

## Architecture Snapshot

```
source
  → logos lexer
  → parser (chumsky or lalrpop + Pratt for expressions)
  → AST (mirrors EBNF)
  → sema (scopes, types, basic checks)
  → codegen (inkwell Context / Module / Builder)
  → LLVM IR → object / JIT / executable
```

Suggested layout:

Use the already present `compiler` crate in the directory. Add dependencies if you need to.

```
src/
  token.rs, lexer.rs
  ast.rs
  parse/          # expr (Pratt), decl, stmt
  sema/           # types, scope, check
  codegen/        # context, expr, stmt, decl
  error.rs        # spans + ariadne/miette
  main.rs         # clap driver
```

## Critical Language Design Points (do not ignore)

- Blocks are `do` … `end` (not `{}`).
- Variable declarations are type-first — `int x = 1` (no `let`).
- Statement terminators are newline **or** `;`.
- Class/struct/enum/trait bodies use `has` … `end`.
- Struct literals use `Type has field = expr … end`.
- Match is `match expr do arms end`.
- `defer` exists and must run on all exit paths of its scope.
- Strings support interpolation `{ expression }` — handle in the parser, not the lexer.
- Two legal `main` signatures (see EBNF section 37).
- Keywords are reserved (full list in EBNF).

## Codegen Ground Rules

- Locals and parameters → `alloca` in the entry block, then load/store.
- Prefer opaque pointers (`ptr`) on modern LLVM.
- Choose one width for `int` (recommend `i64`) and keep it consistent.
- `defer` → maintain a per-scope stack of deferred code; emit on every exit.
- Generics → monomorphize; do not emit generic LLVM functions.
- Classes → start with static methods + explicit `this`; add vtables only when `open`/`override` is required.
- Never skip `module.verify()` before emission.

## Error Handling

Use span-based diagnostics (`ariadne` or `miette`). Report:

- Unexpected token / incomplete construct
- Undefined name
- Type mismatch
- Missing return on non-void path
- Invalid `break`/`continue` target

## When the User Asks for a Specific Piece

| Request | Start from |
|---------|------------|
| Lexer / tokens | EBNF §2–5, full keyword list |
| Expressions | EBNF §8 (use Pratt / precedence climbing) |
| Statements / blocks | EBNF §13–20 |
| Functions | EBNF §21 |
| Types | EBNF §6–7 |
| Classes / structs / traits | EBNF §22–25 (Phase 4) |
| Match | EBNF §10 + §16 (Phase 2+) |
| Defer | EBNF §19 + control-flow exits |
| FFI | EBNF §36 |
| Full driver | Phase 1 end-to-end |

## Anti-Patterns

- Implementing the entire language before any executable exists.
- Skipping name resolution / type checking and lowering untyped AST.
- Using SSA values for mutable locals instead of allocas (until the user is ready for memory-SSA / phis).
- Inventing `{}` blocks or `let` bindings that are not in the EBNF.
- Ignoring statement-terminator rules (newline vs `;`).
- Matching on string content for keywords instead of a proper keyword set.

## Output Expectations

- Prefer complete, compilable Rust fragments over pseudocode when implementing a phase.
- Always state which phase the work belongs to.
- Cite the relevant EBNF productions when adding syntax.
- Keep test programs in a `tests/` or `examples/` directory as `.hlt` files.
