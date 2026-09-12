# Hella Type System

## Purpose

This document defines Hella's type-system architecture and the boundary between:

* types provided by the Hella standard library, and
* language-level type forms and syntax understood directly by the compiler.

The Hella standard library is distributed alongside the Hella compiler.

Fundamental types are standard-library types. They are **not user-facing compiler keywords** and do not require explicit imports.

Collection type forms that are intrinsic to Hella's type syntax remain compiler constructs.

---

# 1. Fundamental Types

The following are fundamental Hella types provided by the standard library:

```text
bool
string

i8
i16
i32
i64
i128
int

u8
u16
u32
u64
u128
uint

float
double
```

These names must **not** be implemented as compiler keywords.

They are standard-library definitions that the compiler automatically makes available to every Hella source file.

For example:

```hella
int age = 25
string name = "John"
bool active = true
float ratio = 0.5
double precise = 0.5
```

No import is required to use these types.

The compiler may have intrinsic knowledge of their underlying representations and operations, but that is an implementation detail.

From the programmer's perspective, these are standard-library types.

---

# 2. Integer Types

Hella provides the following signed integer types:

```text
i8
i16
i32
i64
i128
int
```

And the following unsigned integer types:

```text
u8
u16
u32
u64
u128
uint
```

`int` and `uint` are pointer-sized integer types.

The language does **not** provide `isize` or `usize`.

The intended mapping is:

```text
int  → signed pointer-sized integer
uint → unsigned pointer-sized integer
```

The compiler may use platform-specific representations for these types.

---

# 3. Floating-Point Types

Hella provides two floating-point types:

```text
float
double
```

Their underlying representations are:

```text
float  → f32
double → f64
```

`f32` and `f64` are not the user-facing Hella type names.

---

# 4. Boolean Type

The boolean type is:

```text
bool
```

Example:

```hella
bool enabled = true
bool finished = false
```

`bool` is a standard-library type and is automatically available.

---

# 5. String Type

The string type is:

```text
string
```

Example:

```hella
string name = "John"
```

`string` is a standard-library type and is automatically available.

The standard library provides the relevant string operations and methods.

---

# 6. Compiler Keywords vs Standard-Library Types

Hella must maintain a strict distinction between **types** and **language-level type forms**.

## Standard-library types

These include:

```text
bool
string

i8
i16
i32
i64
i128
int

u8
u16
u32
u64
u128
uint

float
double
```

These are ordinary Hella type names.

They must not be reserved as compiler keywords.

The compiler resolves them through the automatically available standard-library environment.

## Compiler type forms

The following are compiler-level collection type forms:

```text
arr
vec
```

These are part of Hella's type grammar and are understood directly by the compiler.

Maps are **not** represented by a `map` keyword.

Map types use the `TYPE:TYPE` syntax described below.

---

# 7. Arrays

`arr` is a compiler keyword used to express fixed-size array types.

The element type precedes `arr`.

## Inferred array size

When an array is initialized with an array literal, its size is inferred from the initializer.

Example:

```hella
int arr numbers = [1, 2, 3, 4]
```

This creates an array of four integers.

A string array:

```hella
string arr names = ["John", "Jane", "Abbas"]
```

creates an array of three strings.

## Explicit array size

An array may specify its size explicitly:

```hella
int arr[5] numbers
```

When no initializer is supplied, the array is zero-initialized:

```text
[0, 0, 0, 0, 0]
```

An explicitly sized array has a fixed size for its lifetime.

Arrays cannot dynamically grow or shrink.

---

# 8. Array Syntax

The supported forms are:

```text
TYPE arr variable = [values...]
```

and:

```text
TYPE arr[SIZE] variable
```

Examples:

```hella
int arr numbers = [1, 2, 3, 4]

int arr[5] numbers

string arr names = ["John", "Jane", "Abbas"]
```

The compiler must verify that:

* all initializer elements are compatible with the declared element type;
* an explicitly specified size is valid;
* the initializer does not violate the declared size.

---

# 9. Vectors

`vec` is a compiler keyword used to express dynamically sized, owning vectors.

The element type precedes `vec`.

Example:

```hella
int vec numbers = [1, 2, 3, 4]
```

This creates a vector whose element type is `int`.

Vectors own their storage and may dynamically grow.

The standard library provides the vector implementation and its methods, while the compiler recognizes the `vec` type form.

---

# 10. Empty Vectors

An empty vector may be created using:

```hella
any names = vec[]
```

An empty vector initially has no established element type.

The first compatible insertion establishes its element type.

Example:

```hella
any names = vec[]

names.push("John")
```

After the first insertion, the vector is a `string` vector.

Its element type is permanently established and cannot subsequently change.

Conceptually:

```text
empty vector
     ↓
first insertion
     ↓
element type established
     ↓
element type remains fixed
```

Another example:

```hella
any values = vec[]

values.push(42)
values.push(100)
```

creates an integer vector.

The following is invalid:

```hella
any values = vec[]

values.push(42)
values.push("John")
```

because the vector's element type was established as `int`.

---

# 11. `any` with Empty Vectors

For empty vectors, `any` represents an initially undetermined element type.

It does **not** mean that the vector becomes a permanently heterogeneous collection.

For example:

```hella
any values = vec[]

values.push(42)
```

establishes the vector's element type as `int`.

After that point, the vector behaves as an integer vector.

The compiler must reject subsequent insertions whose types are incompatible with the established element type.

The exact internal mechanism for deferred element-type inference is an implementation detail.

The language-level rule is that the element type becomes fixed after it is established.

---

# 12. Maps

Hella does **not** have a `map` keyword.

Map types are expressed using the `TYPE:TYPE` syntax:

```text
KEY_TYPE:VALUE_TYPE
```

The type before `:` is the key type.

The type after `:` is the value type.

For example:

```hella
string:int ages
```

represents a map from `string` keys to `int` values.

Another example:

```hella
string:bool admins
```

represents a map from `string` keys to `bool` values.

The compiler must recognize `TYPE:TYPE` as a map type form.

---

# 13. Map Construction

Map literals use the `has ... end` construction syntax.

A multiline map:

```hella
string:int ages = has
    "John": 25
    "Jane": 30
end
```

A compact map:

```hella
string:bool admins = has "John": false, "Jane": true end
```

Each entry consists of:

```text
key: value
```

The key must be compatible with the map's key type.

The value must be compatible with the map's value type.

For example:

```hella
string:int ages = has
    "John": 25
    "Jane": 30
end
```

has:

```text
key type   → string
value type → int
```

The compiler must type-check both sides of every map entry.

---

# 14. Map Semantics

Maps are associative collections of keys and values.

The map implementation and its operations belong to the standard library.

The compiler is responsible for understanding the `TYPE:TYPE` type form and validating its types.

The standard library provides the relevant map methods and operations.

There is no user-facing `map` type keyword.

The following distinction is intentional:

```text
int arr
int vec
string:int
```

These are different language-level collection type forms.

---

# 15. Slices

Slices are non-owning views over contiguous storage.

A slice does not own the elements it references.

The collection model is:

```text
array  → fixed-size, owning storage
vector → dynamically-sized, owning storage
slice  → non-owning contiguous view
map    → associative key/value collection
```

Slices may reference compatible contiguous storage such as arrays or vectors.

The compiler must enforce the type and lifetime rules required by Hella's memory-management model.

The standard library provides relevant slice operations and methods.

---

# 16. Standard-Library Responsibility

All fundamental Hella types are defined by the standard library:

```text
bool
string

i8
i16
i32
i64
i128
int

u8
u16
u32
u64
u128
uint

float
double
```

Their APIs, methods, and higher-level behavior belong to the standard library.

The compiler may provide intrinsic implementation support where necessary for:

* primitive representation;
* arithmetic;
* comparisons;
* ABI integration;
* LLVM lowering;
* memory operations;
* runtime integration;
* other operations that cannot reasonably be implemented entirely in Hella.

Compiler support does not make these types compiler keywords.

---

# 17. Automatic Availability

The fundamental standard-library types are part of Hella's implicit initial type environment.

A source file must be able to use:

```hella
int
uint
i32
u64
float
double
bool
string
```

without importing anything.

For example:

```hella
int main() {
    string message = "Hello"
    bool valid = true

    return 0
}
```

must be valid without an explicit standard-library import.

This automatic availability applies to the designated fundamental types.

It does not imply that every standard-library type or module is automatically imported.

Other standard-library facilities may require explicit imports according to Hella's module system.

---

# 18. No Built-In Fundamental Type Keywords

The compiler must not expose the fundamental types as reserved language keywords.

In particular, the following must remain identifiers resolved through the standard library:

```text
int
uint

i8
i16
i32
i64
i128

u8
u16
u32
u64
u128

float
double

bool
string
```

The compiler may recognize their definitions specially after resolving them through the standard-library environment.

The source-level architecture must nevertheless treat them as standard-library types.

This means the compiler should not require a separate hard-coded keyword/token category merely for these type names.

---

# 19. Collection Type Forms

The current collection-related language forms are:

```text
TYPE arr
TYPE arr[SIZE]

TYPE vec
vec[]

KEY_TYPE:VALUE_TYPE
```

Examples:

```hella
int arr numbers = [1, 2, 3]

int arr[5] numbers

int vec numbers = [1, 2, 3]

any values = vec[]

string:int ages = has
    "John": 25
    "Jane": 30
end
```

`arr` and `vec` are compiler keywords.

`map` is not a keyword and must not be introduced.

Map types are expressed through the `TYPE:TYPE` syntax.

---

# 20. Design Principle

Hella should avoid unnecessary compiler magic in its user-facing type system.

The guiding rule is:

> If something is a type, it should be a standard-library type whenever possible. If something defines or transforms type syntax, it belongs to the language/compiler.

Therefore:

```text
int       → standard-library type
uint      → standard-library type
i32       → standard-library type
u64       → standard-library type
float     → standard-library type
double    → standard-library type
bool      → standard-library type
string    → standard-library type

arr       → compiler type form
vec       → compiler type form
TYPE:TYPE → compiler map type form
```

The standard library remains responsible for the actual implementations and APIs of these types.

The compiler remains responsible for the language syntax and semantics that cannot simply be represented as ordinary library declarations.

The standard library and compiler are distributed together, so fundamental types feel like built-in parts of Hella to the programmer while remaining architecturally separated from the compiler's language keywords.

---

# 21. Collection and String Methods

The compiler validates and lowers these methods directly (they operate on compiler-managed storage that Hella source cannot address):

```text
vectors: len(), is_empty(), push(x), pop(), clear(),
         contains(x), first(), last()
arrays:  len(), is_empty(), contains(x), first(), last()
maps:    len(), is_empty(), contains(k), remove(k),
         clear(), get_or(k, default)
strings: len(), is_empty()
```

Notes:

* `pop`, `first`, and `last` on an empty vector trap via `abort`.
  `first`/`last` on an empty fixed array likewise trap.
* `push` past vector capacity (16 entries, MVP) traps via `abort`, as
  does map insert past capacity.
* Map reads of missing keys yield the zero value; `get_or(k, default)`
  supplies an explicit fallback instead. `remove` reports whether the
  key was present (swap-with-last, order not preserved).
* `get` is a reserved property keyword, hence `get_or`.

Because these lowercase libc/runtime symbols are referenced by generated
code, user functions may not be named `strlen`, `strcmp`, `abort`,
`puts`, `printf`, `putchar`, `strcat`, or `sprintf`.

---

# 22. Pair Iteration

`for` binds one variable by default and optionally a second:

```hella
for name in names do
    println(name)
end

for name, index in names do
    println(index)
    println(name)
end
```

Over arrays, vectors, and strings the second variable is the `int`
index. Over maps the first variable binds keys and the second binds
values:

```hella
string:int users = has
    "John": 1
    "Jane": 2
    "Jack": 3
end

for key, value in users do
    ...
end
```
