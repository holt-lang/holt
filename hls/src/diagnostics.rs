//! Convert compiler lex/parse/sema errors into LSP `Diagnostic`s so a client
//! renders them inline as the user edits, mirroring `holt check`.

use compiler::sema;
use compiler::token::Span;
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString};

use crate::document::span_to_range;

/// Produce diagnostics for a document by running the same lex → parse →
/// sema pipeline the CLI uses, and mapping every error to an LSP
/// `Diagnostic` with a proper source span.
pub fn diagnostics(source: &str) -> Vec<Diagnostic> {
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
    // Sema
    if let Some(prog) = &parsed {
        for e in sema::check(prog) {
            out.push(diag(source, e.span.start, e.span.end, e.message));
        }
    }

    out
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