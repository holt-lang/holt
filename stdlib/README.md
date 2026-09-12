# Hella Standard Library

Pure Hella sources. See `.opencode/skills/stdlib/SKILL.md` for import semantics (EBNF §32),
`.opencode/skills/stdlib/REAL_STDLIB.md` for the bare-minimum compiler contract, and roadmap.

The compiler knows no user-facing IO names. Every symbol below is an ordinary Hella
function defined in `stdlib/` on top of `extern "c"` libc declarations. Calling one
without its `import` is a sema error (`undefined function`) by design.

## Modules

- `std::io` — `stdlib/std/io.hll`
  - `void print(string s)` — no newline (`printf("%s", s)`)
  - `void println(string s)` — with newline (`puts(s)`)
  - `void printInt(int n)` — decimal with newline (`printf("%ld\n", n)`)
  - `void putChar(char c)` — single char, no newline (`putchar(c)`)
  - `void eprint(string s)` / `void eprintln(string s)` — stderr, without/with
    newline (`write(2, …)`; no `FILE*` global needed)
  - `string readLine()` — one stdin line sans newline, 255-byte cap, `""` on EOF
    (`calloc` + `scanf("%255[^\n]%*c")`)
  - `int readInt()` — one stdin integer, `0` on EOF (`scanf("%ld%*c", out n)`)
  - extern linkage (not imported selectively — always carried along):
    `i32 puts(string s)`, `i32 printf(string fmt, ...)`, `i32 putchar(char c)`,
    `int write(int fd, string buf, int count)`, `string calloc(int n, int size)`,
    `int scanf(string fmt, ...)`
- `std::types` — `stdlib/std/types.hll` (doc-only manifest of the implicit
  environment: `bool string i8…u128 int uint float double`; importing is a no-op)

Import examples:

```
import std::io
import std::io::{print, println}
```

Selective imports keep the module's `extern` blocks automatically (linkage
requirements, not selectable symbols).

## Build

`./target/debug/hella build examples/stdlib_io.hll` inlines `stdlib/std/io.hll` and links against libc. The input half needs piped stdin:

```
printf 'Ada\n42\n' | ./stdlib_io
```

## Not yet

File IO, formatting/interpolation helpers, buffering.
