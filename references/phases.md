# Holt Compiler Phases

Always advance one phase at a time. Each phase must produce runnable code (JIT or object file) before the next begins.

## Phase 0 — Skeleton - DONE

- Cargo project, `inkwell` linked, `llvm-config` working
- Token enum covering keywords + operators needed later
- Empty driver that reads a file and prints tokens

**Exit criteria:** `cargo run -- examples/empty.hlt` tokenizes without crash.

## Phase 1 — MVP Executable (highest priority) - DONE

**Syntax subset**
- Types: `int`, `bool`, `void` (optionally `float`/`double`)
- Literals: integer, `true`/`false`
- Expressions: arithmetic (`+ - * / %`), comparisons, `and`/`or`/`not`, parentheses, assignment
- Statements: expression-stmt, variable declaration (`Type name = expr`), `if`/`else`, `while`, `return`, `do`…`end` blocks
- Functions: free functions, parameters, return type
- `main` — either `void main()` or `int main()`

**Sema**
- Nested scopes
- Name resolution for variables and functions
- Basic type checking
- Every non-void path returns

**Codegen**
- `alloca` for locals/params
- Basic blocks for `if`/`while`
- LLVM IR → JIT or `.o` + link

**Exit criteria:** Programs such as factorial or Fibonacci compile and run correctly.

## Phase 2 — Data & Simple Control - DONE

- Struct declarations + field access + struct literals (`Type has … end`)
- Arrays (`T[]`) or fixed-size arrays; indexing
- Pointers (`T*`) and address-of / dereference (decide exact surface from EBNF modifiers)
- Simple `match` on integers / bools / simple enums
- `string` type (pointer + length) without interpolation first
- `break` / `continue` (unlabeled)

## Phase 3 — Defer & Loops - DONE

- `defer` (expression or block) — must execute on all scope exits
- `loop`, labeled loops, labeled `break`/`continue`
- `for name in expr`
- Nested defers and interaction with `return`/`break`

## Phase 4 — Abstraction - DONE

- Classes, fields, methods, `this`
- Constructors (`initialize`)
- `open` / `override` / `sealed` (start without vtables; add when needed)
- Traits + `implements` (fat pointer or monomorph)
- Enums with optional payloads
- Properties (get/set)
- Visibility (`public`/`private`)

## Phase 5 — Advanced - DONE

- Generics + monomorphization + `where` clauses
- Closures (`|params| => expr` / `|params| do … end`, indirect calls)
- String interpolation (`"hello {expr}"` via `sprintf`/`strcat`/`strdup`)
- Operator overloading (`operator +` etc. via `__op_*` dispatch)
- Conversions (`convert … to …`)
- `extern` FFI blocks (`extern "c" from "m"`)
- Modules / `import` (qualified `::` + `{a,b}` lists)
- `init` blocks (`holt.init`)
- Attributes (`@inline` etc.)
- Distinct types / typedefs (`distinct` wraps struct, `typedef` alias)
- Extensions (`extend Type do … end`)
- `float`/`double` literals and `any`/`function` types

## Out of Scope for Early Phases

- Full borrow/ownership system
- Async
- Macros
- Incremental compilation
- Custom LLVM passes beyond standard optimization levels
