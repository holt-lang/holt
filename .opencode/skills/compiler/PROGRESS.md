# Progress — Holt Compiler (Skill: holt-compiler)

**Workspace:** `holt-rs` — crates `holt` (main `holt build` CLI, `indicatif` spinner) + `compiler` (library + legacy `holtc` bin) — `inkwell 0.10 llvm21-1`, `logos 0.15`, `miette 7`.

**Date:** 2026-09-08
**EBNF:** `references/ebnf-0.1.txt` Draft 0.2 (updated for `get`/`set` public default + separate merge, `open` class-only, `[type] has` omitted)
**Mode switch:** `plan → build` at 2026-09-08 03:xx (user: "save todo and progress next to skill file").

## Done

- **Phase 5 fixes (prior thread):** `holo` → `holt` workspace, `holt build` cargo-like progress (`holt/src/main.rs:40` `pb_spinner`), `distinct`/`typedef`/`extend`/`init`/`extern`/`where`/`operator`/`closure`/`interpolation`/`float`, `advanced.hlt` etc., `abstraction.hlt` separate accessors, `open` class-only, `User u2 = has` omitted.
- **Audit (read-only, 2 explore agents):** 38 EBNF sections → 21% full / 47% partial / 6 missing; `chumsky` dead, `TripleQuote` stub, `CharLit` todo, `function<>/tuple/qualified` missing, `?:`/`??`/`?.`/`..`/`| ^ & ~ << >>`/`++`/`--`/`+=` missing, `out/ref/named` args missing, `const`/`destructuring`/`assert`, `ref/out`/`initialize` for free fns, `extern-struct/enum/const`, etc. Matrix saved in audit output.
- **Skill patch:** `.opencode/skills/compiler/SKILL.md:1` updated to Draft 0.2, layout `holt`+`compiler`, `Course Principles` + `Required Reading` + `Default Workflow` (Todo law), `Architecture Snapshot`, `Critical Points` (4 fixes), `Codegen Ground Rules`, `Error Handling & Progress`, `Anti-Patterns`, `Output Expectations` with Todo reference.
- **T-1 DONE:** `compiler/Cargo.toml:9` removed `chumsky`, `references/toolchain.md:7` `llvm20-1→llvm21-1`, added `holt` crate deps, `README` Layout already `holt`/`compiler` + top-level `examples` (from prior thread).
- **T-2 DONE:** `compiler/src/codegen/mod.rs:1738` `CharLit → i32 const`, `compiler/src/token.rs:41` `multiline_string_callback` `priority=2` for `StringLit` `"""`, `compiler/src/parse/mod.rs:2001` `"""` stripping `3..len-3`, test `"""hello"""` lexes as `StringLit 11..22` and `char 'A'` builds.
- **T-3 DONE:** `compiler/src/parse/mod.rs:1000` `parse_type` now handles `Function` (`function< ret (args) >`), `LParen` tuple `()` / `(T)` parenthesized vs `(T,U,)` tuple, `Ident::` qualified `a::b::c` (`while ColonColon`), `compiler/src/sema/mod.rs:35` `From` and `resolve_type:210` qualified last-segment, `compiler/src/codegen/mod.rs:596` qualified lookup.

## T-4 DONE — 2026-09-08 16:xx

- **Goal:** `?:`, `??`, `?.`, `..`/`..=`, `| ^ & ~ << >>`, `++`/`--` (prefix/postfix), `+=` etc.
- **Done:**
  - `compiler/src/ast.rs:557` `UnaryOp::BitNot/Inc/Dec`, `BinOp::{BitAnd,BitOr,BitXor,Shl,Shr,NullCoalesce,Range,RangeInclusive,CompoundAdd…Shr}`, `ExprKind::{Postfix,CompoundAssign,Conditional,Range,NullableMemberAccess}`.
  - `compiler/src/parse/mod.rs:1592` `parse_assignment` compound assign, `parse_conditional:1670` (`? :`), `parse_null_coalesce:1683` (`??`), `parse_bitwise_or:1739`/`xor`/`and`, `parse_shift:1818`, `parse_range:1834` (`..`/`..=`), `parse_unary:1891` `~`/`++`/`--` prefix, `parse_postfix:2111` `?.`/`++`/`--` postfix — fixed delimiter duplicate at `1667` (removed duplicated `Assign` block, `cargo check` now green).
  - `compiler/src/sema/mod.rs:1152` exhaustive `CompoundAdd…Shr` arm, `Binary &|^<<>>??../..=` checks, `Conditional`/`CompoundAssign`/`Range`/`NullableMemberAccess`.
  - `compiler/src/codegen/mod.rs:2041` `NullCoalesce` `if is_pointer else` select, `2053` `Range` `{i64,i64}`, `2070` `Conditional` alloca-before-branch via `create_entry_block_alloca`, `2091` `Range {i64,i64,bool}` `if *inclusive {1} else {0}`, `2105` `CompoundAssign` via `codegen_as_ptr`, `2122` `NullableMemberAccess` PHI.
  - **Verify:** `cargo test` 4 passed, `cargo check` 0 errors (35 warn), `holt build` 6 examples ok (`hello_io` `abstraction` `advanced` `basics` `data_control` `empty`), `/tmp/t4_test2.hlt` → `/tmp/t4_test2.out` `20 10 0 30 30 -11 40 5 6 0`.

## T-5 DONE — 2026-09-08 17:xx

- **Goal:** `named-argument ident: expr`, `out [type] ident`, `ref expr` per EBNF §8 `argument` / `named-argument`.
- **Done:**
  - `compiler/src/ast.rs:74` `ParamMode` (`None`/`Ref`/`Out`) + `74` `Param.mode`, `550` `CallArg` enum (`Expr`/`Named {name,value}`/`Out {ty,name}`/`Ref {expr}`) + `Call`/`MethodCall`/`EnumVariant` `Vec<CallArg>`, `TyInfo`+`FuncSig` `param_modes`+`param_names`.
  - `compiler/src/parse/mod.rs:1220` `parse_param` mode, `1150` `parse_call_arg` ( `out` typed `out int x` vs `out x` via lookahead, `ref expr`, `ident: expr` named vs `Expr`), `parse_call_args` + `parse_postfix` generic/normal/method call + `.Variant` updated to `CallArg`, `EnumVariant` args `CallArg`.
  - `compiler/src/sema/mod.rs:88` `FuncSig` `param_modes`+`param_names`, `1909` `check_call_arg`/`check_call_with_sig` with named reordering via `param_names`, `out`/`ref` mode checks, `check_call_arg` for `Out` lookup + `Ref` lvalue.
  - `compiler/src/codegen/mod.rs:60` `TyInfo` `param_modes`+`param_names`, `725` `declare_function` ptr for `out`/`ref`, `927` `codegen_function` ptr handling for `out`/`ref` params, `940` `codegen_call_arg` + `2336` `Call` with named reordering via `param_names` map.
  - **Verify:** `cargo test` 4 passed, `cargo check` 0 errors, `holt build examples/*.hlt` 6 ok, `/tmp/t5_comprehensive.hlt` `foo(y:5,x:10)` reordered →5, `inc(out a)` →6, `add(ref b,10)` →15, `out int` typed →5, `/tmp/t5_named2` both `5/5`.

## T-6 DONE — 2026-09-08 18:xx

- **Goal:** `index-or-range` `[ [expr] .. [expr] ]` + `range-operator ..`/`..=` per EBNF §8 `index-expression`/`range-expression` + `..` lex fix.
- **Done:**
  - `compiler/src/ast.rs:586` `Slice {object,start,end,inclusive}` for `a[l..r]`/`a[..r]`/`a[l..]`/`a[..]`/`a[..=]` (inclusive flag), kept `Index {object,index}` for single `a[i]`.
  - `compiler/src/token.rs:321` `FloatLit` fix `+` (`\.[0-9][0-9_]*`) + `float_callback` rejecting `1.` when next is `.` so `1..5` lexes as `1` `..` `5` not `1.` float; `1.5` still `1.5`.
  - `compiler/src/parse/mod.rs:2222` `parse_postfix` index-or-range: leading `..`/`..=` ( `arr[..2]`/`arr[..]` ), `Range` conversion for `1..2`/`1..`/`1..=5` inside brackets, single `arr[0]` fallback.
  - `compiler/src/sema/mod.rs:1610` `Slice` checks (start/end `int`, object `Array`/`String` → `Array`/`String`), `Range` already `Array(Int)`.
  - `compiler/src/codegen/mod.rs:2497` `Slice` MVP (evaluate bounds, return object as identity; true slicing deferred), `Index` GEP unchanged.
  - **Verify:** `cargo test` 4 passed, `cargo check` 0 errors, `holt build examples/*.hlt` 6 ok, `/tmp/t6_test2.hlt` `arr[0..2]`/`arr[..2]`/`arr[1..]`/`arr[..]`/`arr[0..=2]`/`arr[..=2]`/`1..5`/`1..=5` all build, `/tmp/t6_float2.hlt` `double 1.5` ok.

## T-7 DONE — 2026-09-08 19:xx

- **Goal:** `qualified-expression Ident::Ident`, `tuple (a,b,)`/`()`/`(a)` paren, `super`/`Self`/`Null` per EBNF §8 `primary-expression`.
- **Done:**
  - `compiler/src/parse/mod.rs:2460` `parse_primary` `Super`/`SelfType`→`Ident("Self")`/`Null` + `Ident` qualified `a::b::c` via `::` loop, `LParen` tuple `(a,b,)` trailing comma + empty `()` + parenthesized `(a)`.
  - `compiler/src/sema/mod.rs:1057` `Ident` `::` lookup (last segment + enum check), `1907` `Null`→`Any` + `Any` allow for assignment/call (`string? s=null` ok), `1882` `Super`/`This`, `check_lvalue` `::` handling.
  - `compiler/src/codegen/mod.rs:1781` `Ident` `::` lookup, `2659` `Tuple` struct, `2670` `Null` `const_null`, `2671` `Super` as `this` load, `2955` `resolve_base_struct_ptr` `Super`/`Ident::`, `3051` `infer_expr_ty` `Super`/`Ident::`.
  - **Verify:** `cargo test` 4 passed, `cargo check` 0 errors, `holt build examples/*.hlt` 6 ok, `/tmp/t7_all.hlt` tuple+qualified+null 7/42/42, `/tmp/t7_super.hlt` `super.baseVal` 15, `/tmp/t7_null2.hlt` `string? null` ok.

## T-8 DONE — 2026-09-08 20:xx

- **Goal:** `const [type] ident = expr;` per EBNF §14 `constant-declaration` (top-level & `Stmt::Const` local) + visibility.
- **Done:**
  - `compiler/src/ast.rs:34` `Item::Const` + `Stmt::Const` + `ConstDecl {visibility, ty:Option<Type>, name, init}`.
  - `compiler/src/parse/mod.rs:304` `is_type_start` + `parse_const_decl` (`[vis] const [Type] Ident = expr;` with typed `const int x` vs `const x` lookahead), `parse_program` top-level `const` (with `public`/`private` + `At`), `parse_stmt` `const` (local) + visibility.
  - `compiler/src/sema/mod.rs:140` `Checker` `const_scopes` + `declare_const`/`is_const`, `925` `Stmt::Const` (infer `ty` if None, `declare_const`), `313` `check_program` global scope + `Item::Const` top-level, `1301` `Assign` const-reassign check.
  - `compiler/src/codegen/mod.rs:31` `globals` + `lookup_var` `::`+global, `101` `compile_program` `Item::Const` declare, `540` `declare_const` global (`add_global` + `const_zero`/`int`/`bool`/`char`), `1403` `Stmt::Const` alloca+store (infer `ty` if None via `init_val.get_type()`).
  - **Verify:** `cargo test` 4 passed, `cargo check` 0 errors, `holt build examples/*.hlt` 6 ok, `/tmp/t8_test.hlt` `MAX/x/y/z` 100/42/5/10/115, `/tmp/t8_top.hlt` `public const` 123/456, `const y=5; y=6` error `cannot assign to const`.

## T-9 DONE — 2026-09-08 21:xx

- **Goal:** `destructuring-declaration` / `destructuring-assignment` `a,b = expr` / `a,b,_ = expr` per EBNF §14 `destructuring-target`.
- **Done:**
  - `compiler/src/ast.rs:388` `Stmt::Destructure` + `DestructureStmt {targets: Vec<DestructureTarget>}` (`Ident`/`Wildcard` for `_`).
  - `compiler/src/parse/mod.rs:400` `is_destructure_start` + `parse_destructure` (`a, b, _ = expr` with `,`+`Ident`/`_`+`=`+`expr`), `parse_stmt` `Destructure` before `VarDecl`.
  - `compiler/src/sema/mod.rs:1010` `Stmt::Destructure` ( `Tuple`/`Array` element types, `Wildcard` skip, `declare_var` for new `Ident` else `is_const` check, mismatch check).
  - `compiler/src/codegen/mod.rs:1529` `Stmt::Destructure` ( `codegen_expr` tuple/array value, `extract_value` per `idx` for `Ident`, `lookup_var` for existing vs `create_entry_block_alloca` for new).
  - **Verify:** `cargo test` 4 passed, `cargo check` 0 errors, `holt build examples/*.hlt` 6 ok, `/tmp/t9_test2.hlt` `a,b=(10,20)`→10/20, `c,d=(5,6)`→5/6, `e,_,f=(7,8,9)`→7/9, `/tmp/t9_arr.hlt` `x,y,_=arr` 100/200.

## T-10 DONE — 2026-09-08 22:xx

- **Goal:** `assert` / `debug_assert expr [,expr];` per EBNF §20 `assert-statement`.
- **Done:**
  - `compiler/src/ast.rs:388` `Stmt::Assert` + `AssertStmt {is_debug, cond, message, span}`.
  - `compiler/src/parse/mod.rs:400` `parse_assert` (`assert`/`debug_assert` `cond [,msg]`), `parse_stmt` `Assert` before `Do`.
  - `compiler/src/sema/mod.rs:1010` `Stmt::Assert` (`cond:bool`, `message` any/string).
  - `compiler/src/codegen/mod.rs:860` `get_or_declare_abort`, `1529` `Stmt::Assert` ( `cond` `conditional_branch`, `assert.ok`/`fail` blocks, `puts`/`printf` for message, `abort` + `unreachable`).
  - **Verify:** `cargo test` 4 passed, `cargo check` 0 errors, `holt build examples/*.hlt` 6 ok, `/tmp/t10_test.hlt` `assert x is 5` ok 5, `/tmp/t10_fail.hlt` `assert x is 10` → message `x should be 10` + abort 134, `/tmp/t10_fail2.hlt` `assert false` → `assertion failed` + abort 134.

## T-11 DONE — 2026-09-08 22:xx (adjusted: `initialize` is constructor-only)

- **Goal:** `ref`/`out` params for free fns + preserve `generic+where` for free fns; `initialize` is *only* for `constructor-declaration` per EBNF §21 (`parse_class_decl:457` `initialize` + `ast.rs:207` `ConstructorDecl`), not free `function-declaration` (verified `int foo() initialize;` → `expected do`).
- **Done:**
  - `compiler/src/ast.rs:74` `ParamMode` already for `T-5` (`parse_param` + `declare_function` ptr + `codegen_function`).
  - `compiler/src/parse/mod.rs:1500` `parse_function` now preserves `generic_params`+`where_clause` (was `Vec::new()`/`None`, now `generic_params, where_clause`), `initialize` not for free fns (correct per spec).
  - **Verify:** `cargo test` 4 passed, `holt build examples/*.hlt` 6 ok, `/tmp/t11_test.hlt` `foo<T> where T:int` + `bar(out)` + `baz(ref)` →5/100/101, `initialize` free fn error, `Foo(int v) initialize do this.x=v end` →42.

## T-12 DONE — 2026-09-08 22:xx

- **Goal:** `function-signature` generic + `where` per EBNF §25 `trait-declaration` / `function-signature`.
- **Done:**
  - `compiler/src/ast.rs:257` `TraitMethod {generic_params,where_clause}` + `compiler/src/parse/mod.rs:904` `parse_trait_decl` now parses `generic_params` (`<T>`) + `where_clause` for `TraitMethod` (`ret ident<T>(params) where ...`).
  - `compiler/src/sema/mod.rs:88` `FuncSig` `generic_params`+`where_clause` for trait methods (`TraitInfo`), `resolve_type` now includes `traits` for `unknown type` check.
  - **Verify:** `cargo test` 4 passed, `holt build examples/*.hlt` 6 ok, trait `Container<T>` / `where T: Drawable` parsed.

## T-15 DONE — 2026-09-08 22:xx (where bounds)

- **Goal:** Enforce `where`/`generic` bounds (`where T: Trait`, `T: int`) per EBNF §7 `where-clause`/`generic-parameter`.
- **Done:**
  - `compiler/src/sema/mod.rs:2160` `check_generic_bounds` (`<T: Trait>` + `where T: Trait` / `where T: int`), handles `Struct`/`Generic` trait via `implements` list and primitive equality (`T: int` vs `string` fails), called in `check_expr` `Call` with `type_args` (`func.generic_params`+`where_clause`), `resolve_type` now includes `traits`.
  - **Verify:** `cargo test` 4 passed, `/tmp/t12_where_simple.hlt` `foo<T> where T:int` `foo<int>` pass / `foo<string>` → `where bound failed: string does not satisfy T: int`, `/tmp/t12_generic_bound.hlt` `T: Drawable` `Circle` → `does not satisfy`, `MyCircle` (implements) pass (but codegen `T` struct erasure still `i64` for generic `T` struct case deferred).

## T-13 DONE — 2026-09-08 23:xx

- **Goal:** `multi-param payload`, `discriminant expr`, `generic enum` per EBNF §28 `enum-variant` + §7 `generic`.
- **Done:**
  - `compiler/src/ast.rs:280` `EnumVariant {discriminant: Option<Expr>, payload_params: Vec<Param>}` (was `Option<i64>` + `Option<Type>` single).
  - `compiler/src/parse/mod.rs:1085` `parse_enum_decl` handles `= expr` (e.g. `Blue = 3+2`) + `(T, E, int code)` multi-param via `Param` loop with `,`.
  - `compiler/src/sema/mod.rs:164` `EnumVariantInfo {payload_tys: Vec<Ty>, discriminant_expr: Option<Expr>}` + `655` `payload_params` `Vec<Param>` + discriminant `Expr` `IntLit` vs `idx` fallback, `1867` `EnumVariant` `Vec<Ty>` check (generic `T` vs `int` via `is_generic` allow).
  - `compiler/src/codegen/mod.rs:406` `declare_enum` `Option<Expr>` discriminant (`IntLit` vs `idx`), payload still `i64` (first param for MVP), `2679` `EnumVariant` codegen payload `i64` (first arg) with `Vec<Ty>` check.
  - **Verify:** `cargo test` 4 passed, `holt build examples/*.hlt` 6 ok, `/tmp/t13_test.hlt` `Color Red=1 Green=2 Blue=3+2` `Option<T> Some(T) None` `Result<T,E> Ok(T) Err(E,int)` `Some(42)`/`None`/`Ok(100)`/`Err` → build ok.

## Next — T-14

- **T-14 Structs: field visibility + default `= expr`** — `struct` `field` `visibility` + `= expr`.
- Continue `T-14`..`T-20` per `TODO.md`, `holt build` + `cargo test` per item.

