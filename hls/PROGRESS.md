# hls — Holt Language Server

Implementation tracking for the `hls` LSP binary.

## Goal

A Language Server for the Holt language, speaking the Language Server
Protocol over stdio, reusing the `compiler` crate for lex/parse/sema.

## Stack / rationale

| Crate | Version | Why |
|-------|---------|-----|
| `lsp-server` | 0.7.8 | Rust-analyzer's generic LSP scaffold: stdio `Connection`, message framing, crossbeam channels. All deps cached locally (offline-safe). |
| `lsp-types` | 0.97 | Typed LSP structures (requests, notifications, diagnostics, etc.). |
| `serde` / `serde_json` | 1 | Wire format (the LSP is JSON-RPC 2.0). |
| `crossbeam-channel` | 0.5 | Transitive from `lsp-server`. |
| `compiler` | path | Lex/parse/sema/AST reuse. |

## Progress log

- [x] Inspect workspace, `compiler` public API (`lex`, `parse`, `sema::check`), and AST.
- [x] Confirm offline dependency availability for `lsp-server` + `lsp-types`.
- [x] Scaffold `hls` crate (`Cargo.toml` with deps, module layout).
- [ ] Implement LSP server loop (introspect main, lifecycle).
- [ ] Document manager: open/change/close, byte &ndash; line/col utils.
- [ ] Diagnostics: lex/parse/sema errors -> `textDocument/publishDiagnostics`.
- [ ] Symbol table from AST (functions, structs, classes, enums, consts, locals, params).
- [ ] Features: hover, goto-definition, completion, document symbols.
- [ ] Tests + manual verification with a driver script.