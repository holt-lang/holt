# Holt Standard Library

Pure Holt sources. See `.opencode/skills/stdlib/SKILL.md` for import semantics (EBNF §32) and roadmap.

## Modules

- `std::io` — `stdlib/std/io.hlt`
  - `void print(string s)` — no newline
  - `void println(string s)` — with newline
  - `void printInt(int n)` — decimal
  - `void putChar(char c)` — single char

Import examples:

```
import std::io
import std::io::{print, println}
```

## Build

`cargo run -p compiler -- examples/stdlib_io.hlt` inlines `stdlib/std/io.hlt` and links against libc (`puts`/`printf`/`putchar`).

Minimal IO is intrinsified in `compiler/src/codegen/mod.rs` until `extern` §36 lands; no `runtime/` C needed this iteration.
