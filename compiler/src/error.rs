//! Span-based diagnostics via `miette` (chosen over `ariadne` for
//! richer structured reports + `thiserror` integration — both are
//! suggested in `references/toolchain.md:10`, but `miette` gives
//! `#[source_code]` + `#[label]` and `fancy` rendering with no
//! manual `Report` building).

use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

use crate::token::Span;

/// A single lex error rendered with source context.
#[derive(Debug, Error, Diagnostic)]
#[error("unexpected token `{slice}`")]
pub struct LexDiagnostic {
    #[source_code]
    pub src: NamedSource<String>,
    #[label("here")]
    pub span: SourceSpan,
    pub slice: String,
}

impl LexDiagnostic {
    pub fn new(filename: String, source: String, span: Span, slice: String) -> Self {
        Self {
            src: NamedSource::new(filename, source),
            span: span.to_source_span(),
            slice,
        }
    }
}

/// Wrapper for multiple diagnostics (miette wants `Diagnostic` per error,
/// but Phase 0 aggregates lex errors before exiting).
#[derive(Debug, Error, Diagnostic)]
#[error("lexing failed with {count} error(s)")]
pub struct MultiLexError {
    #[source_code]
    pub src: NamedSource<String>,
    #[related]
    pub errors: Vec<LexDiagnostic>,
    count: usize,
}

impl MultiLexError {
    pub fn from_parts(filename: String, source: String, errors: Vec<(Span, String)>) -> Self {
        let diagnostics = errors
            .into_iter()
            .map(|(span, slice)| LexDiagnostic {
                src: NamedSource::new(filename.clone(), source.clone()),
                span: span.to_source_span(),
                slice,
            })
            .collect::<Vec<_>>();
        let count = diagnostics.len();
        // miette's `#[related]` expects each error to own its source, but we also
        // keep a top-level source for the aggregate. Deduplication is okay for Phase 0.
        Self {
            src: NamedSource::new(filename, source),
            errors: diagnostics,
            count,
        }
    }
}
