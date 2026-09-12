//! Lexer wrapper around `logos` that produces `SpannedToken`s and
//! miette-friendly diagnostics for invalid input.

use logos::Logos;

use crate::token::{Span, SpannedToken, Token};

/// Result of lexing: tokens (including `Newline`/`;` terminators) and raw
/// lex errors surfaced for diagnostics. Trivia (whitespace/comments) is
/// already skipped by the `logos` `skip` attributes in `token.rs`.
pub struct LexOutput {
    pub tokens: Vec<SpannedToken>,
    pub errors: Vec<LexError>,
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub span: Span,
    pub slice: String,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unexpected token `{}` at {}..{}",
            self.slice, self.span.start, self.span.end
        )
    }
}

/// Lex the entire source string into spanned tokens.
///
/// Newlines are preserved as `Token::Newline` because `statement-terminator`
/// in EBNF §13 is `newline | ";"` (see `SKILL.md: Statement terminators`).
pub fn lex(source: &str) -> LexOutput {
    let mut lexer = Token::lexer(source);
    let mut tokens = Vec::new();
    let mut errors = Vec::new();

    while let Some(result) = lexer.next() {
        let span = Span::new(lexer.span().start, lexer.span().end);
        let slice = lexer.slice().to_string();
        match result {
            Ok(tok) => tokens.push(SpannedToken { token: tok, span }),
            Err(_) => {
                errors.push(LexError { span, slice });
            }
        }
    }

    LexOutput { tokens, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty() {
        let out = lex("");
        assert!(out.tokens.is_empty());
        assert!(out.errors.is_empty());
    }

    #[test]
    fn newlines_preserved() {
        let out = lex("int x\nint y;");
        assert!(out.errors.is_empty());
        // int, x, newline, int, y, ;
        let kinds: Vec<_> =
            out.tokens.iter().map(|t| format!("{}", t.token)).collect();
        assert!(kinds.contains(&"<newline>".to_string()));
    }

    #[test]
    fn comments_skipped() {
        let src = r#"
            // line comment
            int x = 1 /* block comment */ ;
        "#;
        let out = lex(src);
        assert!(out.errors.is_empty());
        assert!(out.tokens.iter().any(|t| t.token == Token::Int));
    }

    #[test]
    fn keywords_not_ident() {
        let out = lex("int if else while do end has");
        assert_eq!(
            out.tokens
                .iter()
                .filter(|t| t.token != Token::Newline)
                .map(|t| t.token.clone())
                .collect::<Vec<_>>(),
            vec![
                Token::Int,
                Token::If,
                Token::Else,
                Token::While,
                Token::Do,
                Token::End,
                Token::Has
            ] // Note: `Has` is not a reserved keyword in EBNF §2, but `has` is a structural
              // delimiter. We keep it as a keyword for phase 1+; fallback is ident if not added.
              // If this fails, you'll need to extend Token with Has.
        );
    }
}
