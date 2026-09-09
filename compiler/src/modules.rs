//! Module resolution shared by the CLI (`holt build/check/run`) and the
//! language server (`hls`).
//!
//! Two roots, in order:
//!
//! 1. **Project root** — the nearest ancestor of the entry file containing a
//!    `main.hlt` (falling back to the entry file's own directory). Every
//!    other `.hlt` file next to it is a module (`import foo` → `foo.hlt`),
//!    and directories are traversed by qualified paths
//!    (`import net::http` → `net/http.hlt`). A bare directory is also
//!    importable through its entry file (`import net` → `net.hlt`, else
//!    `net/mod.hlt`).
//! 2. **Standard-library roots** — a dev-checkout `stdlib/` found by ancestor
//!    search from the entry (so working in the repo uses live sources),
//!    then `~/.hella/lib` on UNIX-like systems (populated by `holt setup`).
//!
//! Resolution is textual inlining before sema (see `expand_imports`).
//! Cycles (`a` ↔ `b`) are cut by tracking visited files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::ast::{Item, Program};
use crate::parse::ParseError;
use crate::token::Span;

/// A failed import. `span` is in the coordinates of `file` (the document
/// containing the failing `import` statement), so CLI and LSP can each
/// decide whether and where to render it.
#[derive(Debug, Clone)]
pub struct ImportError {
    pub message: String,
    pub span: Span,
    pub file: PathBuf,
}

impl From<ImportError> for ParseError {
    fn from(e: ImportError) -> Self {
        ParseError {
            message: e.message,
            span: e.span,
        }
    }
}

/// Standard-library home on UNIX-like systems (`~/.hella/lib`).
/// Populated by `holt setup`. `None` elsewhere / when `$HOME` is missing.
pub fn hella_lib_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    {
        std::env::var("HOME")
            .ok()
            .map(|h| PathBuf::from(h).join(".hella").join("lib"))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Dev-checkout fallback: nearest ancestor directory (of the entry file)
/// that contains a `stdlib/` directory, e.g. the repo root when working
/// inside it. `None` for entries outside any checkout.
fn workspace_stdlib_root(start: &Path) -> Option<PathBuf> {
    let mut dir = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };
    loop {
        let candidate = dir.join("stdlib");
        if candidate.is_dir() {
            return Some(candidate);
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => break,
        }
    }
    None
}

/// Project root for an entry file: nearest ancestor (starting with the
/// entry's own directory) containing a `main.hlt`; otherwise the entry's
/// own directory.
pub fn project_root(entry: &Path) -> PathBuf {
    let mut dir = if entry.is_dir() {
        entry.to_path_buf()
    } else {
        entry
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };
    loop {
        if dir.join("main.hlt").is_file() {
            return dir;
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => break,
        }
    }
    if entry.is_dir() {
        entry.to_path_buf()
    } else {
        entry
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// Ordered search bases for an entry file: project root, then stdlib roots
/// (dev checkout first so repo work uses live sources, then `~/.hella/lib`).
pub fn search_bases(entry: &Path) -> Vec<PathBuf> {
    let mut bases = vec![project_root(entry)];
    if let Some(ws) = workspace_stdlib_root(entry) {
        if !bases.contains(&ws) {
            bases.push(ws);
        }
    }
    if let Some(home) = hella_lib_dir() {
        if !bases.contains(&home) {
            bases.push(home);
        }
    }
    bases
}

/// Resolve a qualified import path (`["std", "io"]`) against ordered bases.
/// Tries `<base>/std/io.hlt`, then the directory-module entry
/// `<base>/std/io/mod.hlt`.
pub fn resolve_import(path: &[String], bases: &[PathBuf]) -> Option<PathBuf> {
    let rel = path.join("/");
    for base in bases {
        let file = base.join(format!("{rel}.hlt"));
        if file.is_file() {
            return Some(file);
        }
        let mod_file = base.join(&rel).join("mod.hlt");
        if mod_file.is_file() {
            return Some(mod_file);
        }
    }
    None
}

struct Ctx {
    bases: Vec<PathBuf>,
    visited: HashSet<PathBuf>,
    errors: Vec<ImportError>,
}

/// Inline `import` items recursively (textual inclusion before sema).
/// Never fails fatally: unresolvable imports are collected in the returned
/// error list (spans in the importing file's coordinates) and skipped, so
/// the LSP can still check the rest of the document. The CLI treats the
/// first error as fatal, preserving `holt build` behavior.
pub fn expand_imports(
    program: Program,
    importer: &Path,
) -> (Program, Vec<ImportError>) {
    let mut ctx = Ctx {
        bases: search_bases(importer),
        visited: HashSet::new(),
        errors: Vec::new(),
    };
    let span = program.span;
    let items = expand_items(program.items, importer, &mut ctx);
    (Program { items, span }, ctx.errors)
}

fn expand_items(
    items: Vec<Item>,
    importer: &Path,
    ctx: &mut Ctx,
) -> Vec<Item> {
    let mut out_items: Vec<Item> = Vec::new();
    for item in items {
        let Item::Import(imp) = item else {
            out_items.push(item);
            continue;
        };
        let Some(path) = resolve_import(&imp.path, &ctx.bases) else {
            let searched = ctx
                .bases
                .iter()
                .map(|b| b.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            ctx.errors.push(ImportError {
                message: format!(
                    "cannot resolve import `{}` (looked in {searched})",
                    imp.path.join("::")
                ),
                span: imp.span,
                file: importer.to_path_buf(),
            });
            continue;
        };
        // Cycle guard: same file reached twice (directly or transitively).
        if !ctx.visited.insert(path.clone()) {
            continue;
        }
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                ctx.errors.push(ImportError {
                    message: format!("failed to read import {}: {e}", path.display()),
                    span: imp.span,
                    file: importer.to_path_buf(),
                });
                continue;
            }
        };
        let toks = crate::lexer::lex(&src);
        if !toks.errors.is_empty() {
            ctx.errors.push(ImportError {
                message: format!("lex error in import {}", path.display()),
                span: imp.span,
                file: importer.to_path_buf(),
            });
            continue;
        }
        let sub = match crate::parse::parse(toks.tokens, src.clone()) {
            Ok(p) => p,
            Err(e) => {
                ctx.errors.push(ImportError {
                    message: format!("parse error in {}: {}", path.display(), e.message),
                    span: imp.span,
                    file: importer.to_path_buf(),
                });
                continue;
            }
        };
        let sub_items = expand_items(sub.items, &path, ctx);
        if let Some(ref syms) = imp.symbols {
            let wanted: HashSet<String> = syms.iter().map(|(s, _)| s.clone()).collect();
            for it in sub_items {
                match &it {
                    Item::Function(f) if wanted.contains(&f.name) => out_items.push(it),
                    Item::Struct(s) if wanted.contains(&s.name) => out_items.push(it),
                    Item::Class(c) if wanted.contains(&c.name) => out_items.push(it),
                    Item::Enum(e) if wanted.contains(&e.name) => out_items.push(it),
                    Item::Import(_) => {}
                    // `extern` blocks are linkage requirements, not
                    // selectable symbols: a selective import like
                    // `import std::io::{print}` still needs the libc
                    // declarations its wrappers call into.
                    Item::Extern(_) => out_items.push(it),
                    _ => {}
                }
            }
        } else {
            for it in sub_items.into_iter().filter(|i| !matches!(i, Item::Import(_))) {
                out_items.push(it);
            }
        }
    }
    out_items
}
