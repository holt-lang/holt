//! Phase 1 recursive-descent + Pratt parser.
//! Covers EBNF §6-8 §13-21 subset for int/bool/void, functions, blocks, if/while/return/var/expr.

use crate::ast::*;
use crate::token::{Span, SpannedToken, Token};

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} at {}..{}",
            self.message, self.span.start, self.span.end
        )
    }
}

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    source: String,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>, source: String) -> Self {
        Self {
            tokens,
            pos: 0,
            source,
        }
    }

    fn peek(&self) -> Option<&SpannedToken> {
        self.tokens.get(self.pos)
    }
    fn peek_token(&self) -> Option<&Token> {
        self.peek().map(|st| &st.token)
    }
    fn peek_span(&self) -> Span {
        self.peek()
            .map(|st| st.span)
            .unwrap_or(Span::new(self.source.len(), self.source.len()))
    }
    fn is_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn advance(&mut self) -> Option<SpannedToken> {
        if self.is_eof() {
            return None;
        }
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        Some(t)
    }

    fn expect(
        &mut self,
        expected: Token,
        msg: &str,
    ) -> Result<SpannedToken, ParseError> {
        match self.peek() {
            Some(st) if st.token == expected => Ok(self.advance().unwrap()),
            Some(st) => Err(ParseError {
                message: format!(
                    "{msg}: expected `{expected}`, found `{}`",
                    st.token
                ),
                span: st.span,
            }),
            None => Err(ParseError {
                message: format!("{msg}: expected `{expected}`, found EOF"),
                span: Span::new(self.source.len(), self.source.len()),
            }),
        }
    }

    fn consume_if(&mut self, tok: Token) -> bool {
        if self.peek_token() == Some(&tok) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn consume_newlines(&mut self) -> usize {
        let mut n = 0;
        while matches!(
            self.peek_token(),
            Some(Token::Newline) | Some(Token::Semicolon)
        ) {
            self.advance();
            n += 1;
        }
        n
    }

    fn expect_terminator(&mut self, ctx: &str) -> Result<(), ParseError> {
        // Terminator is newline or ;  — allow multiple, but require at least one unless next is End/else/eof
        let n = self.consume_newlines();
        if n > 0 {
            return Ok(());
        }
        // If next is End or Else or EOF, allow missing terminator (implicit before block end)
        if matches!(
            self.peek_token(),
            Some(Token::End) | Some(Token::Else) | None
        ) {
            return Ok(());
        }
        Err(ParseError {
            message: format!("{ctx}: expected newline or `;`"),
            span: self.peek_span(),
        })
    }

    fn slice(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }

    fn parse_int_lit(&self, span: Span) -> Result<i64, ParseError> {
        let s = self.slice(span).replace('_', "");
        if s.starts_with("0x") || s.starts_with("0X") {
            i64::from_str_radix(&s[2..], 16).map_err(|e| ParseError {
                message: format!("invalid hex int: {e}"),
                span,
            })
        } else if s.starts_with("0b") || s.starts_with("0B") {
            i64::from_str_radix(&s[2..], 2).map_err(|e| ParseError {
                message: format!("invalid bin int: {e}"),
                span,
            })
        } else {
            s.parse::<i64>().map_err(|e| ParseError {
                message: format!("invalid int: {e}"),
                span,
            })
        }
    }

    fn unescape_string(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('b') => out.push('\x08'),
                Some('f') => out.push('\x0c'),
                Some('0') => out.push('\0'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('x') => {
                    let h1 = chars.next().unwrap_or('0');
                    let h2 = chars.next().unwrap_or('0');
                    let hex = format!("{}{}", h1, h2);
                    if let Ok(v) = u8::from_str_radix(&hex, 16) {
                        out.push(v as char);
                    } else {
                        out.push_str(&format!("\\x{}", hex));
                    }
                }
                Some('u') => {
                    let mut hex = String::new();
                    for _ in 0..4 {
                        if let Some(h) = chars.next() {
                            hex.push(h);
                        }
                    }
                    if let Ok(v) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(v) {
                            out.push(ch);
                        } else {
                            out.push_str(&format!("\\u{}", hex));
                        }
                    } else {
                        out.push_str(&format!("\\u{}", hex));
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        }
        out
    }
    fn unescape_char(s: &str) -> char {
        if s.len() == 1 {
            return s.chars().next().unwrap();
        }
        if s.starts_with('\\') {
            let un = Self::unescape_string(s);
            un.chars().next().unwrap_or('\0')
        } else {
            s.chars().next().unwrap_or('\0')
        }
    }

    // ── Entry ─────────────────────────────────────────────────────────
    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let start = 0;
        self.consume_newlines();
        let mut items = Vec::new();
        while !self.is_eof() {
            self.consume_newlines();
            if self.is_eof() {
                break;
            }
            // Top-level: struct/class decl vs function decl
            if self.peek_token() == Some(&Token::Struct) {
                let decl = self.parse_struct_decl()?;
                items.push(Item::Struct(decl));
            } else if self.peek_token() == Some(&Token::Class) {
                let decl = self.parse_class_decl()?;
                items.push(Item::Class(decl));
            } else {
                let func = self.parse_function()?;
                items.push(Item::Function(func));
            }
            self.consume_newlines();
        }
        Ok(Program {
            items,
            span: Span::new(start, self.source.len()),
        })
    }

    fn parse_struct_decl(&mut self) -> Result<StructDecl, ParseError> {
        let start = self.expect(Token::Struct, "expected `struct`")?.span.start;
        let (name, name_span) = self.parse_ident()?;
        self.expect(Token::Has, "expected `has` after struct name")?;
        self.consume_newlines();
        let mut fields = Vec::new();
        while !self.is_eof() && self.peek_token() != Some(&Token::End) {
            if matches!(
                self.peek_token(),
                Some(Token::Newline) | Some(Token::Semicolon)
            ) {
                self.advance();
                continue;
            }
            let ty = self.parse_type()?;
            let (fname, fspan) = self.parse_ident()?;
            let fend = fspan.end;
            // optional initializer ignored for Phase 2 (not stored) — but consume if present
            if self.consume_if(Token::Eq) {
                let _ = self.parse_expr()?; // ignore default value for now
            }
            self.expect_terminator("struct field")?;
            let span = Span::new(ty.span().start, fend);
            fields.push(StructField {
                ty,
                name: fname,
                name_span: fspan,
                span,
            });
            self.consume_newlines();
        }
        let end = self
            .expect(Token::End, "expected `end` to close struct")?
            .span
            .end;
        Ok(StructDecl {
            name,
            name_span,
            fields,
            span: Span::new(start, end),
        })
    }

    fn parse_class_decl(&mut self) -> Result<ClassDecl, ParseError> {
        let start = self.expect(Token::Class, "expected `class`")?.span.start;
        // optional `open` before class already handled at top-level? For Phase 4 minimal ignore `open`
        let (name, name_span) = self.parse_ident()?;
        // ignore generics/extends/implements for minimal
        self.expect(Token::Has, "expected `has` after class name")?;
        self.consume_newlines();
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.is_eof() && self.peek_token() != Some(&Token::End) {
            if matches!(self.peek_token(), Some(Token::Newline) | Some(Token::Semicolon)) { self.advance(); continue; }
            // Consume optional visibility
            if matches!(self.peek_token(), Some(Token::Public) | Some(Token::Private)) { self.advance(); }
            // Lookahead: function if type ident '('
            let is_func = {
                let save = self.pos;
                let ty_ok = self.parse_type().is_ok();
                let after_ty = self.peek_token().cloned();
                let is_ident = after_ty == Some(Token::Ident);
                // need '(' after ident
                let mut is_func2 = false;
                if is_ident {
                    // peek after ident
                    if let Some(tok) = self.tokens.get(self.pos+1) {
                        if tok.token == Token::LParen { is_func2 = true; }
                    }
                }
                self.pos = save;
                ty_ok && is_func2
            };
            if is_func {
                // function/method
                let ret_ty = self.parse_type()?;
                let (mname, mspan) = self.parse_ident()?;
                self.expect(Token::LParen, "expected `(` for method params")?;
                let mut params = Vec::new();
                if self.peek_token() != Some(&Token::RParen) {
                    loop {
                        let pty = self.parse_type()?;
                        let (pn, pn_span) = self.parse_ident()?;
                        let pspan = Span::new(pty.span().start, pn_span.end);
                        params.push(Param{ty: pty, name: pn, name_span: pn_span, span: pspan});
                        if self.consume_if(Token::Comma) { continue; } else { break; }
                    }
                }
                self.expect(Token::RParen, "expected `)` after params")?;
                let body = self.parse_block()?;
                let span = Span::new(ret_ty.span().start, body.span.end);
                methods.push(Function{ret_ty, name: mname, name_span: mspan, params, body, span});
            } else {
                // field
                let ty = self.parse_type()?;
                let (fname, fspan) = self.parse_ident()?;
                let fend = fspan.end;
                if self.consume_if(Token::Eq) { let _ = self.parse_expr()?; }
                self.expect_terminator("class field")?;
                let span = Span::new(ty.span().start, fend);
                fields.push(StructField{ty, name: fname, name_span: fspan, span});
            }
            self.consume_newlines();
        }
        let end = self.expect(Token::End, "expected `end` to close class")?.span.end;
        Ok(ClassDecl{name, name_span, fields, methods, span: Span::new(start, end)})
    }

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        // primary-type
        let mut ty: Type = {
            let st = self.peek().cloned().ok_or(ParseError {
                message: "expected type".into(),
                span: Span::new(self.source.len(), self.source.len()),
            })?;
            match st.token {
                Token::Int => {
                    self.advance();
                    Type::Int(st.span)
                }
                Token::Bool => {
                    self.advance();
                    Type::Bool(st.span)
                }
                Token::Void => {
                    self.advance();
                    Type::Void(st.span)
                }
                Token::StringKw => {
                    self.advance();
                    Type::String(st.span)
                }
                Token::CharKw => {
                    self.advance();
                    Type::Char(st.span)
                }
                Token::Ident => {
                    self.advance();
                    Type::Named(self.slice(st.span).to_string(), st.span)
                }
                _ => {
                    return Err(ParseError {
                        message: format!("expected type, found `{}`", st.token),
                        span: st.span,
                    });
                }
            }
        };
        // { type-modifier } : "?" | "*" | "[]"
        loop {
            if self.peek_token() == Some(&Token::Question) {
                let q = self.advance().unwrap();
                let span = Span::new(ty.span().start, q.span.end);
                ty = Type::Optional(Box::new(ty), span);
                continue;
            }
            if self.peek_token() == Some(&Token::Star) {
                let star = self.advance().unwrap();
                let span = Span::new(ty.span().start, star.span.end);
                ty = Type::Pointer(Box::new(ty), span);
                continue;
            }
            if self.peek_token() == Some(&Token::LBracket) {
                let rb = self.tokens.get(self.pos + 1);
                if rb.map(|t| t.token == Token::RBracket).unwrap_or(false) {
                    self.advance();
                    self.advance();
                    let end = self.tokens[self.pos - 1].span.end;
                    let span = Span::new(ty.span().start, end);
                    ty = Type::Array(Box::new(ty), span);
                    continue;
                }
            }
            break;
        }
        Ok(ty)
    }

    fn parse_ident(&mut self) -> Result<(String, Span), ParseError> {
        let st = self.peek().cloned().ok_or(ParseError {
            message: "expected identifier".into(),
            span: Span::new(self.source.len(), self.source.len()),
        })?;
        if st.token == Token::Ident {
            self.advance();
            Ok((self.slice(st.span).to_string(), st.span))
        } else {
            Err(ParseError {
                message: format!("expected identifier, found `{}`", st.token),
                span: st.span,
            })
        }
    }

    fn parse_function(&mut self) -> Result<Function, ParseError> {
        let start_span = self.peek_span();
        let ret_ty = self.parse_type()?;
        let (name, name_span) = self.parse_ident()?;
        self.expect(Token::LParen, "function params")?;
        let mut params = Vec::new();
        // params: [type ident {, type ident}]
        if self.peek_token() != Some(&Token::RParen) {
            loop {
                let ty = self.parse_type()?;
                let (pn, pn_span) = self.parse_ident()?;
                let pspan = Span::new(ty.span().start, pn_span.end);
                params.push(Param {
                    ty,
                    name: pn,
                    name_span: pn_span,
                    span: pspan,
                });
                if self.consume_if(Token::Comma) {
                    continue;
                } else {
                    break;
                }
            }
        }
        self.expect(Token::RParen, "closing `)`")?;
        // function body: block (do ... end)
        // Note EBNF function-body = block ; phase 1 only block
        let body = self.parse_block()?;
        let span = Span::new(start_span.start, body.span.end);
        Ok(Function {
            ret_ty,
            name,
            name_span,
            params,
            body,
            span,
        })
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        let start = self
            .expect(Token::Do, "expected `do` to start block")?
            .span
            .start;
        self.consume_newlines();
        let mut stmts = Vec::new();
        while !self.is_eof() && self.peek_token() != Some(&Token::End) {
            // allow blank lines inside block
            if matches!(
                self.peek_token(),
                Some(Token::Newline) | Some(Token::Semicolon)
            ) {
                self.advance();
                continue;
            }
            let stmt = self.parse_stmt()?;
            stmts.push(stmt);
            // terminators already handled inside parse_stmt; consume extra newlines
            self.consume_newlines();
        }
        let end = self
            .expect(Token::End, "expected `end` to close block")?
            .span
            .end;
        Ok(Block {
            stmts,
            span: Span::new(start, end),
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        // Check for labeled loop/for:  ident ":" (loop|for)
        if self.peek_token() == Some(&Token::Ident) && self.tokens.get(self.pos+1).map(|t| t.token == Token::Colon).unwrap_or(false) {
            let label_tok = self.tokens[self.pos].clone();
            // Look ahead after colon
            let after_colon = self.tokens.get(self.pos+2).map(|t| &t.token);
            if matches!(after_colon, Some(Token::Loop)) {
                let label = self.slice(label_tok.span).to_string();
                let label_span = label_tok.span;
                self.advance(); // ident
                self.advance(); // :
                let l = self.parse_loop(Some((label, label_span)))?;
                return Ok(Stmt::Loop(l));
            } else if matches!(after_colon, Some(Token::For)) {
                let label = self.slice(label_tok.span).to_string();
                let label_span = label_tok.span;
                self.advance(); // ident
                self.advance(); // :
                let f = self.parse_for(Some((label, label_span)))?;
                return Ok(Stmt::For(f));
            }
        }
        match self.peek_token() {
            Some(Token::If) => {
                let s = self.parse_if()?;
                Ok(Stmt::If(s))
            }
            Some(Token::While) => {
                let s = self.parse_while()?;
                Ok(Stmt::While(s))
            }
            Some(Token::Loop) => {
                let l = self.parse_loop(None)?;
                Ok(Stmt::Loop(l))
            }
            Some(Token::For) => {
                let f = self.parse_for(None)?;
                Ok(Stmt::For(f))
            }
            Some(Token::Defer) => {
                let d = self.parse_defer()?;
                Ok(Stmt::Defer(d))
            }
            Some(Token::Return) => {
                let s = self.parse_return()?;
                Ok(Stmt::Return(s))
            }
            Some(Token::Break) => {
                let s = self.parse_break()?;
                Ok(Stmt::Break(s))
            }
            Some(Token::Continue) => {
                let s = self.parse_continue()?;
                Ok(Stmt::Continue(s))
            }
            Some(Token::Do) => {
                let b = self.parse_block()?;
                Ok(Stmt::Block(b))
            }
            _ => {
                // Try var-decl detection: type (int/bool/void or Named Ident) + Ident + (= or terminator)
                if self.is_var_decl_start() {
                    let d = self.parse_var_decl()?;
                    Ok(Stmt::VarDecl(d))
                } else {
                    let e = self.parse_expr_stmt()?;
                    Ok(Stmt::Expr(e))
                }
            }
        }
    }

    fn is_var_decl_start(&mut self) -> bool {
        let save = self.pos;
        // Try parse a type; if fails, not a decl
        let ty = match self.parse_type() {
            Ok(t) => t,
            Err(_) => {
                self.pos = save;
                return false;
            }
        };
        // After type, next token must be Ident (var name)
        let is_ident = matches!(self.peek_token(), Some(Token::Ident));
        self.pos = save;
        // Need to ensure we actually consumed something and next is ident; also avoid mistaking `Point has` as decl
        if !is_ident {
            return false;
        }
        // For Named type followed by Ident, we must ensure that the Ident after type is not "has" (which would be struct literal type usage)
        // But `Point p` vs `Point has ...`: after type `Point`, next token is `p` vs `has`. So `Point has` would have next Token::Has not Ident, so already false.
        // For array types like `int[] arr`, parse_type will consume `int[]` then next is `arr` Ident, so true.
        let _ = ty; // suppress unused
        true
    }

    fn parse_var_decl(&mut self) -> Result<VarDecl, ParseError> {
        let ty = self.parse_type()?;
        let (name, name_span) = self.parse_ident()?;
        let mut init = None;
        let mut end = name_span.end;
        if self.consume_if(Token::Eq) {
            let expr = self.parse_expr()?;
            end = expr.span.end;
            init = Some(expr);
        }
        let term_start = end;
        self.expect_terminator("variable declaration")?;
        // span from ty to term
        let span = Span::new(ty.span().start, term_start);
        // Note: we consumed terminators already; span ends at init/end.
        Ok(VarDecl {
            ty,
            name,
            name_span,
            init,
            span,
        })
    }

    fn parse_if(&mut self) -> Result<IfStmt, ParseError> {
        let start = self.expect(Token::If, "if")?.span.start;
        self.consume_if(Token::LParen);
        let cond = self.parse_expr()?;
        self.consume_if(Token::RParen);
        let then_block = self.parse_block()?;
        // handle else if / else chain
        let else_block = if self.consume_if(Token::Else) {
            if self.peek_token() == Some(&Token::If) {
                // else if -> recursive if parsed as block containing single if stmt?
                // EBNF expands to else if block, we model as else { if }
                let nested_if = self.parse_if()?;
                // Wrap nested_if in a block for uniform else_block
                let span = nested_if.span;
                Some(Block {
                    stmts: vec![Stmt::If(nested_if)],
                    span,
                })
            } else {
                Some(self.parse_block()?)
            }
        } else {
            None
        };
        let end = else_block
            .as_ref()
            .map(|b| b.span.end)
            .unwrap_or(then_block.span.end);
        Ok(IfStmt {
            cond,
            then_block,
            else_block,
            span: Span::new(start, end),
        })
    }

    fn parse_while(&mut self) -> Result<WhileStmt, ParseError> {
        let start = self.expect(Token::While, "while")?.span.start;
        self.consume_if(Token::LParen);
        let cond = self.parse_expr()?;
        self.consume_if(Token::RParen);
        let body = self.parse_block()?;
        let span = Span::new(start, body.span.end);
        Ok(WhileStmt { cond, body, span })
    }

    fn parse_return(&mut self) -> Result<ReturnStmt, ParseError> {
        let start = self.expect(Token::Return, "return")?.span.start;
        // return may have expr or not; peek terminator/end/else
        let needs_semi = matches!(
            self.peek_token(),
            Some(Token::Newline)
                | Some(Token::Semicolon)
                | Some(Token::End)
                | None
        );
        let value = if needs_semi {
            None
        } else {
            Some(self.parse_expr()?)
        };
        let end = value.as_ref().map(|e| e.span.end).unwrap_or(start + 6);
        self.expect_terminator("return")?;
        let span = Span::new(start, end);
        Ok(ReturnStmt { value, span })
    }

    fn parse_break(&mut self) -> Result<BreakStmt, ParseError> {
        let start = self.expect(Token::Break, "break")?.span.start;
        let mut end = start + 5;
        let mut label = None;
        if self.peek_token() == Some(&Token::Ident) {
            let (name, span) = self.parse_ident()?;
            label = Some(name);
            end = span.end;
        }
        self.expect_terminator("break")?;
        Ok(BreakStmt { label, span: Span::new(start, end) })
    }

    fn parse_continue(&mut self) -> Result<ContinueStmt, ParseError> {
        let start = self.expect(Token::Continue, "continue")?.span.start;
        let mut end = start + 8;
        let mut label = None;
        if self.peek_token() == Some(&Token::Ident) {
            let (name, span) = self.parse_ident()?;
            label = Some(name);
            end = span.end;
        }
        self.expect_terminator("continue")?;
        Ok(ContinueStmt { label, span: Span::new(start, end) })
    }

    fn parse_loop(&mut self, label_opt: Option<(String, Span)>) -> Result<LoopStmt, ParseError> {
        let start = if let Some((_, ls)) = &label_opt { ls.start } else { self.peek_span().start };
        self.expect(Token::Loop, "loop")?;
        let body = self.parse_block()?;
        let end = body.span.end;
        let (label, label_span) = match label_opt { Some((n,s)) => (Some(n), Some(s)), None => (None, None) };
        let span = Span::new(start, end);
        Ok(LoopStmt { label, label_span, body, span })
    }

    fn parse_for(&mut self, label_opt: Option<(String, Span)>) -> Result<ForStmt, ParseError> {
        let start = if let Some((_, ls)) = &label_opt { ls.start } else { self.peek_span().start };
        self.expect(Token::For, "for")?;
        let (var, var_span) = self.parse_ident()?;
        self.expect(Token::In, "expected `in` after for variable")?;
        let iter = self.parse_expr()?;
        let body = self.parse_block()?;
        let end = body.span.end;
        let (label, label_span) = match label_opt { Some((n,s)) => (Some(n), Some(s)), None => (None, None) };
        Ok(ForStmt { label, label_span, var, var_span, iter, body, span: Span::new(start, end) })
    }

    fn parse_defer(&mut self) -> Result<DeferStmt, ParseError> {
        let start = self.expect(Token::Defer, "defer")?.span.start;
        if self.peek_token() == Some(&Token::Do) {
            let blk = self.parse_block()?;
            let end = blk.span.end;
            return Ok(DeferStmt { inner: DeferInner::Block(blk), span: Span::new(start, end) });
        }
        let expr = self.parse_expr()?;
        let end = expr.span.end;
        self.expect_terminator("defer")?;
        Ok(DeferStmt { inner: DeferInner::Expr(Box::new(expr)), span: Span::new(start, end) })
    }

    fn parse_expr_stmt(&mut self) -> Result<ExprStmt, ParseError> {
        let expr = self.parse_expr()?;
        let span = expr.span;
        self.expect_terminator("expression statement")?;
        Ok(ExprStmt { expr, span })
    }

    // ── Expressions (Pratt) ────────────────────────────────────────────

    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_or()?;
        if self.peek_token() == Some(&Token::Eq) {
            // Allow Ident, MemberAccess, Index as lvalue (Phase 2)
            let is_lvalue = matches!(
                lhs.kind,
                ExprKind::Ident(_)
                    | ExprKind::MemberAccess { .. }
                    | ExprKind::Index { .. }
                    | ExprKind::Paren(_)
            );
            if !is_lvalue {
                return Err(ParseError {
                    message:
                        "assignment target must be identifier or field access"
                            .into(),
                    span: lhs.span,
                });
            }
            // Normalize paren lvalue? unwrap paren for `(x) = 1` not supported
            if let ExprKind::Paren(_) = lhs.kind {
                return Err(ParseError {
                    message: "cannot assign to parenthesized expression".into(),
                    span: lhs.span,
                });
            }
            let lhs_span = lhs.span;
            self.advance(); // consume =
            let rhs = self.parse_assignment()?; // right-assoc
            let span = Span::new(lhs_span.start, rhs.span.end);
            return Ok(Expr {
                kind: ExprKind::Assign {
                    lhs: Box::new(lhs),
                    value: Box::new(rhs),
                },
                span,
            });
        }
        Ok(lhs)
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_and()?;
        while self.peek_token() == Some(&Token::Or) {
            self.advance();
            let rhs = self.parse_and()?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op: BinOp::Or,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_equality()?;
        while self.peek_token() == Some(&Token::And) {
            self.advance();
            let rhs = self.parse_equality()?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op: BinOp::And,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_relational()?;
        loop {
            let op = match self.peek_token() {
                Some(Token::Is) => {
                    // check for "is not"
                    let is_span = self.peek().unwrap().span;
                    // lookahead
                    let next_is_not = self
                        .tokens
                        .get(self.pos + 1)
                        .map(|st| st.token == Token::Not)
                        .unwrap_or(false)
                        && {
                            // need to ensure slice gap is whitespace? For now treat any adjacent Is Not as IsNot
                            true
                        };
                    if next_is_not {
                        // consume Is + Not
                        self.advance();
                        self.advance();
                        BinOp::IsNot
                    } else {
                        self.advance();
                        BinOp::Is
                    }
                }
                _ => break,
            };
            let rhs = self.parse_relational()?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_relational(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek_token() {
                Some(Token::Lt) => BinOp::Lt,
                Some(Token::LtEq) => BinOp::Le,
                Some(Token::Gt) => BinOp::Gt,
                Some(Token::GtEq) => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_add()?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Ok(lhs)
    }

    // add/sub
    fn parse_add(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek_token() {
                Some(Token::Plus) => BinOp::Add,
                Some(Token::Minus) => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_mul()?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek_token() {
                Some(Token::Star) => BinOp::Mul,
                Some(Token::Slash) => BinOp::Div,
                Some(Token::Percent) => BinOp::Mod,
                _ => break,
            };
            self.advance();
            let rhs = self.parse_unary()?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.peek_token() == Some(&Token::Not) {
            let start = self.advance().unwrap().span.start;
            let expr = self.parse_unary()?;
            let span = Span::new(start, expr.span.end);
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                },
                span,
            });
        }
        if self.peek_token() == Some(&Token::Minus) {
            let start = self.advance().unwrap().span.start;
            let expr = self.parse_unary()?;
            let span = Span::new(start, expr.span.end);
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                },
                span,
            });
        }
        if self.peek_token() == Some(&Token::Plus) {
            let start = self.advance().unwrap().span.start;
            let expr = self.parse_unary()?;
            let span = Span::new(start, expr.span.end);
            return Ok(Expr {
                kind: ExprKind::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(expr),
                },
                span,
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            // call: '(' [args] ')' — handles both free fn `foo()` and method `obj.meth()`
            if self.peek_token() == Some(&Token::LParen) {
                match &expr.kind {
                    ExprKind::Ident(s) => {
                        let callee_name = s.clone();
                        let callee_span = expr.span;
                        self.advance(); // (
                        let mut args = Vec::new();
                        if self.peek_token() != Some(&Token::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if self.consume_if(Token::Comma) { continue; } else { break; }
                            }
                        }
                        let end = self.expect(Token::RParen, "expected `)` after call args")?.span.end;
                        let span = Span::new(callee_span.start, end);
                        expr = Expr{kind: ExprKind::Call{callee: callee_name, callee_span, args}, span};
                        continue;
                    }
                    ExprKind::MemberAccess{object, field, field_span} => {
                        // method call `obj.method(args)` -> MethodCall
                        let obj = object.clone();
                        let meth = field.clone();
                        let meth_span = *field_span;
                        let outer_span_start = expr.span.start;
                        self.advance(); // (
                        let mut args = Vec::new();
                        if self.peek_token() != Some(&Token::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if self.consume_if(Token::Comma) { continue; } else { break; }
                            }
                        }
                        let end = self.expect(Token::RParen, "expected `)` after call args")?.span.end;
                        let span = Span::new(outer_span_start, end);
                        expr = Expr{kind: ExprKind::MethodCall{object: obj, method: meth, method_span: meth_span, args}, span};
                        continue;
                    }
                    _ => break,
                }
            }
            // member access: '.' ident (EBNF §8 postfix member-access)
            if self.peek_token() == Some(&Token::Dot) {
                self.advance(); // consume .
                let (field, fspan) = self.parse_ident()?;
                let span = Span::new(expr.span.start, fspan.end);
                expr = Expr {
                    kind: ExprKind::MemberAccess {
                        object: Box::new(expr),
                        field,
                        field_span: fspan,
                    },
                    span,
                };
                continue;
            }
            // index: '[' expr ']' (EBNF §8 index-expression, Phase 2 simple single expr)
            if self.peek_token() == Some(&Token::LBracket) {
                self.advance(); // [
                let index = self.parse_expr()?;
                if matches!(self.peek_token(), Some(Token::DotDot) | Some(Token::DotDotEq)) {
                    return Err(ParseError{message: "range indexing not supported in Phase 2".into(), span: self.peek_span()});
                }
                let rb = self.expect(Token::RBracket, "expected `]` after index")?;
                let span = Span::new(expr.span.start, rb.span.end);
                expr = Expr{kind: ExprKind::Index{object: Box::new(expr), index: Box::new(index)}, span};
                continue;
            }
            break;
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        // match expression is also primary (EBNF primary-expression includes match-expression)
        if self.peek_token() == Some(&Token::Match) {
            return self.parse_match_expr();
        }
        // Attempt struct literal first: Type has ... end
        if let Some(lit) = self.try_parse_struct_literal()? {
            return Ok(lit);
        }
        let st = self.peek().cloned().ok_or(ParseError {
            message: "expected expression".into(),
            span: Span::new(self.source.len(), self.source.len()),
        })?;
        match st.token {
            Token::IntLit | Token::HexInt | Token::BinInt => {
                self.advance();
                let val = self.parse_int_lit(st.span)?;
                Ok(Expr {
                    kind: ExprKind::IntLit(val),
                    span: st.span,
                })
            }
            Token::True => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::BoolLit(true),
                    span: st.span,
                })
            }
            Token::False => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::BoolLit(false),
                    span: st.span,
                })
            }
            Token::StringLit => {
                self.advance();
                let raw = self.slice(st.span);
                // raw is "\"...\""
                let inner = if raw.len() >= 2 {
                    &raw[1..raw.len() - 1]
                } else {
                    ""
                };
                let decoded = Self::unescape_string(inner);
                Ok(Expr {
                    kind: ExprKind::StringLit(decoded),
                    span: st.span,
                })
            }
            Token::RawStringLit => {
                self.advance();
                let raw = self.slice(st.span);
                // r"..."
                let inner = if raw.len() >= 3 {
                    &raw[2..raw.len() - 1]
                } else {
                    ""
                };
                Ok(Expr {
                    kind: ExprKind::StringLit(inner.to_string()),
                    span: st.span,
                })
            }
            Token::CharLit => {
                self.advance();
                let raw = self.slice(st.span);
                let inner = if raw.len() >= 2 {
                    &raw[1..raw.len() - 1]
                } else {
                    ""
                };
                let ch = Self::unescape_char(inner);
                Ok(Expr {
                    kind: ExprKind::CharLit(ch),
                    span: st.span,
                })
            }
            Token::This => {
                self.advance();
                Ok(Expr{kind: ExprKind::This, span: st.span})
            }
            Token::Ident => {
                self.advance();
                Ok(Expr {
                    kind: ExprKind::Ident(self.slice(st.span).to_string()),
                    span: st.span,
                })
            }
            Token::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                let end = self.expect(Token::RParen, "expected `)`")?.span.end;
                let span = Span::new(st.span.start, end);
                Ok(Expr {
                    kind: ExprKind::Paren(Box::new(inner)),
                    span,
                })
            }
            _ => Err(ParseError {
                message: format!("expected expression, found `{}`", st.token),
                span: st.span,
            }),
        }
    }

    fn parse_match_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.expect(Token::Match, "expected `match`")?.span.start;
        let scrutinee = self.parse_expr()?;
        self.expect(Token::Do, "expected `do` after `match` scrutinee")?;
        self.consume_newlines();
        let mut arms = Vec::new();
        while !self.is_eof() && self.peek_token() != Some(&Token::End) {
            if matches!(
                self.peek_token(),
                Some(Token::Newline) | Some(Token::Semicolon)
            ) {
                self.advance();
                continue;
            }
            let pat_start = self.peek_span().start;
            let pattern = self.parse_pattern()?;
            // optional guard `if expr`
            let guard = if self.peek_token() == Some(&Token::If) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            self.expect(Token::Arrow, "expected `->` after match pattern")?;
            // body: block or expr
            let body = if self.peek_token() == Some(&Token::Do) {
                let blk = self.parse_block()?;
                MatchArmBody::Block(blk)
            } else {
                // expression body — consume without requiring terminator; newlines will separate arms
                let e = self.parse_expr()?;
                MatchArmBody::Expr(Box::new(e))
            };
            let arm_span = Span::new(
                pat_start,
                match &body {
                    MatchArmBody::Expr(e) => e.span.end,
                    MatchArmBody::Block(b) => b.span.end,
                },
            );
            arms.push(MatchArm {
                pattern,
                guard,
                body,
                span: arm_span,
            });
            self.consume_newlines();
        }
        let end = self
            .expect(Token::End, "expected `end` to close `match`")?
            .span
            .end;
        let span = Span::new(start, end);
        Ok(Expr {
            kind: ExprKind::Match(MatchExpr {
                scrutinee: Box::new(scrutinee),
                arms,
                span,
            }),
            span,
        })
    }

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        // Simple Phase 2: `_`, int lit, bool lit
        // Handle alternative `|`? For now handle single primary, but consume `|` chains by taking first only
        let st = self.peek().cloned().ok_or(ParseError {
            message: "expected pattern".into(),
            span: Span::new(self.source.len(), self.source.len()),
        })?;
        match st.token {
            Token::Ident => {
                let s = self.slice(st.span).to_string();
                if s == "_" {
                    self.advance();
                    Ok(Pattern::Wildcard(st.span))
                } else {
                    // For Phase 2 we don't support variable binding patterns; treat as wildcard error?
                    // But allow `_` only; any other ident we treat as error for now to keep simple
                    Err(ParseError {
                        message: format!(
                            "unsupported pattern `{s}`; expected `_` or literal"
                        ),
                        span: st.span,
                    })
                }
            }
            Token::IntLit | Token::HexInt | Token::BinInt => {
                self.advance();
                let v = self.parse_int_lit(st.span)?;
                Ok(Pattern::LitInt(v, st.span))
            }
            Token::True => {
                self.advance();
                Ok(Pattern::LitBool(true, st.span))
            }
            Token::False => {
                self.advance();
                Ok(Pattern::LitBool(false, st.span))
            }
            _ => Err(ParseError {
                message: format!("expected pattern, found `{}`", st.token),
                span: st.span,
            }),
        }
    }

    fn try_parse_struct_literal(&mut self) -> Result<Option<Expr>, ParseError> {
        // Save position to backtrack if not a struct literal
        let save = self.pos;
        // Try parse Type (int/bool/void/named) — but struct literal expects struct Named type
        // We'll peek: Int/Bool/Ident could be start of Type
        let is_type_start = matches!(
            self.peek_token(),
            Some(Token::Int)
                | Some(Token::Bool)
                | Some(Token::Void)
                | Some(Token::Ident)
        );
        if !is_type_start {
            return Ok(None);
        }
        // Lookahead: need Type then `has`
        // We attempt to parse type, then check for Has; if not Has, revert
        let ty = match self.parse_type() {
            Ok(t) => t,
            Err(_) => {
                self.pos = save;
                return Ok(None);
            }
        };
        if self.peek_token() != Some(&Token::Has) {
            self.pos = save;
            return Ok(None);
        }
        // It is a struct literal
        let has_tok = self.advance().unwrap(); // consume has
        self.consume_newlines();
        let mut fields = Vec::new();
        while !self.is_eof() && self.peek_token() != Some(&Token::End) {
            if matches!(
                self.peek_token(),
                Some(Token::Newline) | Some(Token::Semicolon)
            ) {
                self.advance();
                continue;
            }
            let (fname, fspan) = self.parse_ident()?;
            self.expect(Token::Eq, "expected `=` in struct literal")?;
            let expr = self.parse_expr()?;
            self.expect_terminator("struct field initializer")?;
            fields.push((fname, fspan, expr));
            self.consume_newlines();
        }
        let end = self
            .expect(Token::End, "expected `end` to close struct literal")?
            .span
            .end;
        let span = Span::new(ty.span().start, end);
        Ok(Some(Expr {
            kind: ExprKind::StructLit { ty, fields },
            span,
        }))
    }
}

pub fn parse(
    tokens: Vec<SpannedToken>,
    source: String,
) -> Result<Program, ParseError> {
    let mut p = Parser::new(tokens, source);
    p.parse_program()
}
