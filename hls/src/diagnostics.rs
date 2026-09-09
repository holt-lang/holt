//! Convert compiler lex/parse/sema errors into LSP `Diagnostic`s so a client
//! renders them inline as the user edits, mirroring `holt check`.
//!
//! Two project-model rules live here:
//!
//! - Imports are expanded through the shared [`compiler::modules`] resolver
//!   (project root around `main.hlt`, then `~/.hella/lib`), so names from
//!   imported namespaces resolve instead of raising `undefined …`.
//! - `missing `main` function` is only reported for files actually named
//!   `main` — library modules next to `main.hlt` are entry-less by design.

use std::path::Path;

use compiler::sema;
use compiler::token::Span;
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use crate::document::span_to_range;

/// Produce diagnostics for a document by running the same lex → parse →
/// resolve → sema pipeline the CLI uses, and mapping every error to an LSP
/// `Diagnostic` with a proper source span.
///
/// `path` is the document's filesystem path when known (`None` for
/// unsaved/untitled buffers): it drives import resolution and the
/// `main`-file gate. With `None`, imports are skipped and `main` is
/// required, matching the pre-project-model behavior.
pub fn diagnostics(source: &str, path: Option<&Path>) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    // Lex
    let lexed = compiler::lexer::lex(source);
    for e in &lexed.errors {
        out.push(diag(
            source,
            e.span.start,
            e.span.end,
            format!("unexpected token `{}`", e.slice),
        ));
    }
    // Parse (best-effort even with lex errors, so we catch more).
    let parse = compiler::parse::parse(lexed.tokens.clone(), source.to_string());
    let parsed = match parse {
        Ok(p) => Some(p),
        Err(e) => {
            out.push(diag(source, e.span.start, e.span.end, e.message));
            None
        }
    };
    // Resolve imports, then sema.
    if let Some(prog) = parsed {
        // Only entry points must define `main`.
        let require_main = path.map(is_main_file).unwrap_or(true);
        let expanded = match path {
            Some(p) => compiler::modules::expand_imports(prog, p),
            None => compiler::modules::Expanded {
                program: prog,
                errors: Vec::new(),
                files: Vec::new(),
            },
        };
        let import_errors = expanded.errors;
        let expanded = expanded.program;
        // Surface only failures from the open document itself: nested
        // failures belong to the imported file and appear when it is opened.
        if let Some(open) = path {
            for e in import_errors.iter().filter(|e| e.file.as_path() == open) {
                out.push(diag(source, e.span.start, e.span.end, e.message.clone()));
            }
        }
        for e in sema::check_with_options(
            &expanded,
            sema::CheckOptions { require_main },
        ) {
            out.push(diag(source, e.span.start, e.span.end, e.message));
        }
    }

    out
}

/// Entry-point gate: only files named `main` (e.g. `main.hlt`) must define
/// a `main` function. Library modules are checked without one.
fn is_main_file(path: &Path) -> bool {
    path.file_stem().is_some_and(|s| s == "main")
}

fn diag(source: &str, start: usize, end: usize, message: String) -> Diagnostic {
    Diagnostic {
        range: span_to_range(source, Span::new(start, end)),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String("holt".to_string())),
        code_description: None,
        source: Some("hls".to_string()),
        message,
        related_information: None,
        tags: None,
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Scratch project: `<dir>/main.hlt` + `<dir>/util.hlt` next to it.
    fn scratch_project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "hls-diag-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("util.hlt"),
            "int twice(int x) do\n    return x * 2\nend\n",
        )
        .unwrap();
        dir
    }

    fn messages(diags: &[Diagnostic]) -> Vec<&str> {
        diags.iter().map(|d| d.message.as_str()).collect()
    }

    #[test]
    fn library_file_does_not_require_main() {
        let dir = scratch_project();
        let util = dir.join("util.hlt");
        let src = std::fs::read_to_string(&util).unwrap();
        let diags = diagnostics(&src, Some(&util));
        assert!(
            !messages(&diags).iter().any(|m| m.contains("main")),
            "unexpected main complaint: {diags:?}"
        );
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn main_file_still_requires_main() {
        let dir = scratch_project();
        let main = dir.join("main.hlt");
        std::fs::write(&main, "int helper() do\n    return 1\nend\n").unwrap();
        let src = std::fs::read_to_string(&main).unwrap();
        let diags = diagnostics(&src, Some(&main));
        assert!(
            messages(&diags).iter().any(|m| m.contains("missing `main`")),
            "expected missing-main diagnostic: {diags:?}"
        );
    }

    #[test]
    fn imported_namespace_resolves() {
        let dir = scratch_project();
        let main = dir.join("main.hlt");
        let src = "import util\n\nvoid main() do\n    int y = twice(21)\nend\n";
        std::fs::write(&main, src).unwrap();
        let diags = diagnostics(src, Some(&main));
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
    }

    #[test]
    fn unresolvable_import_is_reported() {
        let dir = scratch_project();
        let main = dir.join("main.hlt");
        let src = "import nope::missing\n\nvoid main() do\nend\n";
        std::fs::write(&main, src).unwrap();
        let diags = diagnostics(src, Some(&main));
        assert!(
            messages(&diags)
                .iter()
                .any(|m| m.contains("cannot resolve import `nope::missing`")),
            "expected unresolvable-import diagnostic: {diags:?}"
        );
    }
}