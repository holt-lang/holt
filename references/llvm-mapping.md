# Hella → LLVM Mapping

## Primitive Types

| Hella     | Recommended LLVM | Notes |
|----------|------------------|-------|
| `int`    | `i64`            | Fix width early; do not change later |
| `bool`   | `i1`             | |
| `float`  | `float`          | |
| `double` | `double`         | |
| `char`   | `i32` (Unicode scalar) or `i8` | Decide and document |
| `void`   | `void`           | |
| `string` | `{ ptr, i64 }` or runtime type | Start simple |
| `any`    | boxed / fat pointer | Phase 5 |

## Type Modifiers

| Hella | LLVM approach |
|------|----------------|
| `T?` | Optional: `{ T, i1 }` or nullable pointer |
| `T*` | `ptr` (opaque pointers) |
| `T[]`| Fat pointer `{ ptr, i64 }` + runtime bounds checks optional |

## Variables & Parameters

- Emit all locals and parameters as `alloca` in the function entry block.
- Store incoming parameter values into their allocas.
- Subsequent reads = `load`, writes = `store`.
- This avoids early phi complexity.

## Control Flow

- `if` / `else if` / `else` → then/else/merge basic blocks + `cond_br`
- `while` → header / body / exit; condition in header
- `loop` → header / body / exit (condition always true)
- `return` → emit pending `defer`s then `ret`
- `break` / `continue` → branch to the appropriate loop exit/header; run defers for scopes being exited
- Keep a stack of `LoopContext { header, exit, label }` and a stack of deferred actions

## Functions

```text
ret_ty @name(param_tys...) {
entry:
  %p0 = alloca ...
  store param0, %p0
  ...
  ; body
}
```

- Nested functions / closures later become separate LLVM functions + environment pointer.
- `static` methods = ordinary functions; instance methods = extra `this` parameter.

## Structs & Classes

- Struct → `llvm.struct` with field types in declaration order.
- Class instance → same, plus optional vtable pointer in slot 0 when dynamic dispatch is required.
- Field access → `getelementptr` + load/store.
- Methods → functions taking `ptr` to the instance as first argument (`this`).

## Enums

- Tag + union of payloads, or separate LLVM structs per variant + a discriminant.
- Match → switch on discriminant + basic blocks per arm.

## Defer

Per scope:

```text
defer_stack: Vec<DeferredCode>
```

On any exit (end of block, `return`, `break` that leaves the scope):

1. Pop and emit deferred code in reverse order.
2. Then perform the exit branch / ret.

Defers are **not** executed on `continue` that stays inside the same scope.

## Generics

- Monomorphize at the semantic / codegen boundary.
- Key = (generic item, concrete type arguments).
- Cache specialized LLVM functions/types to avoid duplicate emission.

## Strings & Interpolation

- Runtime representation: pointer + length (and optional capacity).
- Interpolation `"{expr}"` → sequence of runtime calls that append the stringified value of `expr`.
- Do not attempt to lower interpolation inside the lexer.

## FFI (`extern`)

- Each `extern-function` becomes an LLVM function declaration (no body).
- Calling convention defaults to C; adjust if the EBNF later gains annotations.
- Library names are recorded for the linker / JIT symbol resolver.

## Verification & Emission

Always:

```rust
if module.verify().is_err() { /* report and abort */ }
```

Then either:

- JIT via `module.create_jit_execution_engine(...)`, or
- `TargetMachine::write_to_file` (object) + invoke `clang`/`lld`.

