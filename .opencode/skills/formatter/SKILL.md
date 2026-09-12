# Hella Formatter Skill

## Purpose

Implement the `hella fmt` command as Hella's canonical source formatter.

The formatter produces deterministic, idiomatic Hella source code with a strong,
opinionated style.

Formatting is not user-configurable for the initial implementation.

The formatter operates on Hella's parsed syntax rather than regexes or
text-only substitutions.

## Command

The formatter is part of the main Hella application:

    hella fmt [paths...]

Initial behavior:

- Format Hella source files.
- Accept individual files and directories.
- Recursively format Hella source files when given a directory.
- Write formatted source back to the source file.
- Produce deterministic output.
- Be idempotent.

A future `--check` mode may report files that would change without modifying
them.

Do not create a separate `hellafmt` executable.

---

## Canonical Style

### Indentation

- Use 4 spaces per indentation level.
- Never use tabs for indentation.
- Regenerate indentation from syntax rather than preserving source indentation.

### Line Width

- Canonical maximum line width: 100 columns.
- The formatter should avoid exceeding 100 columns whenever a reasonable
  syntactic break exists.
- Prefer readable structural breaks over arbitrary character-based wrapping.

### Blocks

Blocks are always expanded.

The formatter must not preserve compact one-line blocks merely because the
source used a compact form.

For example:

    if active
        do_work()
    end

is the canonical form regardless of whether the original source used a
single-line representation.

The exact block syntax is determined by Hella's grammar; the formatter should
apply the same expansion principle to every block construct.

---

## Blank Lines

Whitespace should provide deliberate visual breathing room.

Rules:

- Never emit multiple consecutive blank lines.
- Normal sections are separated by one blank line.
- There are exactly two blank lines between the final import and the rest of
  the file.
- Top-level declarations are separated by one blank line.
- Logical class sections are separated by one blank line.
- Do not insert decorative blank lines immediately inside blocks.
- A blank line terminates a variable-alignment group.

Example:

    import std::io
    import std::string


    const int MAX_USERS = 100

    struct User has
        string name
    end

---

## Whitespace

Use conventional whitespace unless a Hella-specific rule overrides it.

### Binary Operators

Put one space on each side:

    a + b
    x == y
    value ?? fallback
    flags & mask

### Unary Operators

Do not put a space between unary operators and their operands:

    -x
    !value
    ~flags

### Assignment

Put spaces around assignment operators:

    int age = 26
    age += 1

### Commas

Use conventional comma spacing:

    foo(a, b, c)

Do not put a space before a comma.

### Parentheses

Do not add unnecessary spaces inside parentheses:

    foo(a, b)

### Generic Arguments

Use conventional comma spacing:

    Type<A, B>

---

## Variable Alignment

Alignment is an intentional part of Hella's formatting style.

### Variable Groups

A variable group consists of two or more consecutive variable declarations.

A blank line terminates the group.

Within a group, align:

1. The type column.
2. The variable-name column.
3. The `=` initializer column.

Example:

    string name    = "Hassan"
    int    age     = 26
    bool   active  = true
    double balance = 42.5

A single declaration is not padded for alignment:

    string name = "Hassan"

    int age = 26

If declarations in a group differ in whether they have initializers, do not
invent initializers. Align the existing initializer column for declarations
that have one.

---

## Class and Struct Field Alignment

Class and struct fields use the same alignment principle.

For fields, visibility creates separate alignment groups.

Example:

    class User has
        string name
        int    age
        bool   active

        private string password
        private int    login_count
    end

The default/public field group does not align against the private field group.

If fields have initializers, align the initializer column as well:

    struct Config has
        string name    = "default"
        int    retries = 3
        bool   verbose = false
    end

Visibility is part of the field grouping rule.

---

## Classes

Class members are organized into these categories:

1. Fields
2. Constructors
3. Destructor
4. Methods

The formatter may reorder class members between these categories.

Within each category, preserve source order.

Visibility does not affect ordering.

In particular, methods must never be reordered based on `public` or `private`.

Example:

    class User has
        string name

        initialize(string name)
            this.name = name
        end

        initialize()
            this.name = "Unknown"
        end

        ~User()
            ...
        end

        public void save()
            ...
        end

        private void validate()
            ...
        end

        public void refresh()
            ...
        end
    end

The three methods remain in their original relative order.

Constructors remain in their original relative order.

The destructor occupies the destructor section.

---

## Structs

Structs remain data-only Hella types.

The formatter must not introduce class-style constructs into structs.

Struct fields:

- remain in source order;
- use the field alignment rules;
- use visibility-separated alignment groups;
- use aligned initializer columns when initializers are present.

---

## Line Wrapping

Use a syntax-aware structural wrapping strategy.

General principles:

1. Prefer compact representation when it fits within 100 columns.
2. Avoid exceeding 100 columns when a reasonable syntactic break exists.
3. Break at syntactic boundaries.
4. Indent continuations according to their syntactic structure.
5. Keep related expressions together where reasonably possible.
6. Avoid arbitrary character-based wrapping.
7. Avoid ugly partially wrapped expressions.
8. Do not put every argument on its own line unless structural wrapping requires
   it.

The formatter should choose the layout based on the parsed syntax tree.

---

## Function Calls

Keep calls compact when they fit:

    result = create_user(name, age, active)

When wrapping is required:

    result = create_user(
        name,
        age,
        active
    )

The opening delimiter establishes the continuation structure.

Do not retain a compact call merely because the original source was compact if
the canonical result exceeds the line-width limit.

---

## Function Declarations

Keep function declarations compact when they fit.

When a parameter list must wrap, use structural indentation.

Example:

    function<Result<User, Error>(
        string name,
        int age,
        bool active
    )> create_user(
        string name,
        int age,
        bool active
    )
        ...
    end

Do not use arbitrary character wrapping.

---

## Collections

Keep short collections compact:

    int arr values = [1, 2, 3, 4, 5]

When wrapping is required, use one element per line:

    int arr values = [
        100,
        200,
        300,
        400,
        500,
    ]

Multiline delimited collections use trailing commas.

Apply the same general structural wrapping principle to arrays, vectors, and
other delimited collection literals.

---

## Maps

Hella maps use `has ... end` syntax.

Format multiline maps as blocks:

    string:int ages = has
        "John": 25
        "Jane": 30
        "Alice": 28
    end

Do not introduce commas between map entries merely for formatting.

---

## Imports

Imports form their own top-level section.

Keep imports together at the beginning of the file.

After the final import, insert exactly two blank lines before the next
top-level declaration.

Example:

    import std::io
    import std::string


    const int MAX_USERS = 100

Import ordering is not currently specified.

Do not invent import-sorting rules until they are explicitly defined.

---

## Top-Level Declarations

Separate normal top-level declarations with one blank line.

Example:

    const int MAX_USERS = 100

    struct User has
        string name
    end

    function<void()> main()
        ...
    end

Do not introduce additional blank lines between ordinary top-level
declarations.

---

## Comments

Comments must be preserved.

The formatter must never silently delete comments.

Comments should remain associated with the syntactic construct they document
or annotate.

`///` documentation comments are documentation and must be preserved.

Comment reflow is not part of the initial formatter specification.

Until a dedicated comment-formatting specification exists, formatting should
be conservative about changing comment text.

---

## Semantic Safety

Formatting must not change program semantics.

The formatter may perform the explicitly defined class-member categorization:

    fields
    constructors
    destructor
    methods

Within each category, declaration order must remain unchanged.

The formatter must not:

- reorder methods based on visibility;
- reorder executable statements;
- reorder expressions;
- reorder struct fields;
- reorder top-level declarations;
- perform semantic rewrites merely for formatting.

---

## Parser and AST Requirements

The formatter must operate from Hella's parsed representation.

Do not implement formatting as regex replacement or text-only substitution.

The formatter needs enough syntax information to distinguish at minimum:

- imports;
- top-level declarations;
- variable declarations;
- classes;
- structs;
- fields;
- constructors;
- destructors;
- methods;
- visibility;
- parameters;
- generic parameters;
- expressions;
- function calls;
- collections;
- maps;
- blocks;
- comments.

The formatter should have a dedicated pretty-printing/layout layer rather than
embedding formatting logic throughout the parser.

---

## Idempotency

Formatting must be idempotent.

For every valid source file:

    format(format(source)) == format(source)

A second formatting pass must produce exactly the same output as the first
pass.

This is a hard requirement.

---

## Initial Scope

The first implementation should cover:

- 4-space indentation;
- 100-column line width;
- block expansion;
- whitespace normalization;
- blank-line normalization;
- variable alignment;
- class-field alignment;
- struct-field alignment;
- initializer alignment;
- class member categorization;
- imports;
- top-level declarations;
- structural line wrapping;
- function calls;
- function declarations;
- arrays and collections;
- maps;
- comment preservation;
- deterministic output;
- idempotency.

Do not add additional stylistic transformations without defining them first.

---

## Future Extensions

Potential future features include:

- `hella fmt --check`;
- stdin/stdout formatting;
- editor/LSP integration;
- dedicated comment reflow;
- import sorting;
- more detailed formatting rules for every Hella construct;
- more sophisticated line-breaking heuristics;
- formatter diagnostics;
- formatter configuration, if Hella ever decides to permit configuration.

The initial formatter should remain intentionally opinionated and
configuration-light.
