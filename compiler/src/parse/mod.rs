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
        write!(f, "{} at {}..{}", self.message, self.span.start, self.span.end)
    }
}

pub struct Parser {
    tokens: Vec<SpannedToken>,
    pos: usize,
    source: String,
}

impl Parser {
    pub fn new(tokens: Vec<SpannedToken>, source: String) -> Self {
        Self { tokens, pos: 0, source }
    }

    fn peek(&self) -> Option<&SpannedToken> {
        self.tokens.get(self.pos)
    }
    fn peek_token(&self) -> Option<&Token> {
        self.peek().map(|st| &st.token)
    }
    fn peek_span(&self) -> Span {
        self.peek().map(|st| st.span).unwrap_or(Span::new(self.source.len(), self.source.len()))
    }
    fn is_eof(&self) -> bool { self.pos >= self.tokens.len() }

    fn advance(&mut self) -> Option<SpannedToken> {
        if self.is_eof() { return None; }
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        Some(t)
    }

    fn expect(&mut self, expected: Token, msg: &str) -> Result<SpannedToken, ParseError> {
        match self.peek() {
            Some(st) if st.token == expected => Ok(self.advance().unwrap()),
            Some(st) => Err(ParseError { message: format!("{msg}: expected `{expected}`, found `{}`", st.token), span: st.span }),
            None => Err(ParseError { message: format!("{msg}: expected `{expected}`, found EOF"), span: Span::new(self.source.len(), self.source.len()) }),
        }
    }

    fn consume_if(&mut self, tok: Token) -> bool {
        if self.peek_token() == Some(&tok) { self.advance(); true } else { false }
    }

    fn consume_newlines(&mut self) -> usize {
        let mut n = 0;
        while matches!(self.peek_token(), Some(Token::Newline) | Some(Token::Semicolon)) { self.advance(); n+=1; }
        n
    }

    fn expect_terminator(&mut self, ctx: &str) -> Result<(), ParseError> {
        // Terminator is newline or ;  — allow multiple, but require at least one unless next is End/else/eof
        let n = self.consume_newlines();
        if n > 0 { return Ok(()); }
        // If next is End or Else or EOF, allow missing terminator (implicit before block end)
        if matches!(self.peek_token(), Some(Token::End) | Some(Token::Else) | None) {
            return Ok(());
        }
        Err(ParseError { message: format!("{ctx}: expected newline or `;`"), span: self.peek_span() })
    }

    fn slice(&self, span: Span) -> &str { &self.source[span.start..span.end] }

    fn parse_int_lit(&self, span: Span) -> Result<i64, ParseError> {
        let s = self.slice(span).replace('_', "");
        if s.starts_with("0x") || s.starts_with("0X") {
            i64::from_str_radix(&s[2..], 16).map_err(|e| ParseError { message: format!("invalid hex int: {e}"), span })
        } else if s.starts_with("0b") || s.starts_with("0B") {
            i64::from_str_radix(&s[2..], 2).map_err(|e| ParseError { message: format!("invalid bin int: {e}"), span })
        } else {
            s.parse::<i64>().map_err(|e| ParseError { message: format!("invalid int: {e}"), span })
        }
    }

    // ── Entry ─────────────────────────────────────────────────────────
    pub fn parse_program(&mut self) -> Result<Program, ParseError> {
        let start = 0;
        self.consume_newlines();
        let mut items = Vec::new();
        while !self.is_eof() {
            // skip stray terminators between top-level decls
            self.consume_newlines();
            if self.is_eof() { break; }
            let func = self.parse_function()?;
            items.push(Item::Function(func));
            self.consume_newlines();
        }
        Ok(Program { items, span: Span::new(start, self.source.len()) })
    }

    fn parse_type(&mut self) -> Result<Type, ParseError> {
        let st = self.peek().cloned().ok_or(ParseError{message:"expected type".into(), span: Span::new(self.source.len(), self.source.len())})?;
        match st.token {
            Token::Int => { self.advance(); Ok(Type::Int(st.span)) }
            Token::Bool => { self.advance(); Ok(Type::Bool(st.span)) }
            Token::Void => { self.advance(); Ok(Type::Void(st.span)) }
            _ => Err(ParseError{message: format!("expected type `int`/`bool`/`void`, found `{}`", st.token), span: st.span}),
        }
    }

    fn parse_ident(&mut self) -> Result<(String, Span), ParseError> {
        let st = self.peek().cloned().ok_or(ParseError{message:"expected identifier".into(), span: Span::new(self.source.len(), self.source.len())})?;
        if st.token == Token::Ident {
            self.advance();
            Ok((self.slice(st.span).to_string(), st.span))
        } else {
            Err(ParseError{message: format!("expected identifier, found `{}`", st.token), span: st.span})
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
                params.push(Param{ty, name: pn, name_span: pn_span, span: pspan});
                if self.consume_if(Token::Comma) { continue; } else { break; }
            }
        }
        self.expect(Token::RParen, "closing `)`")?;
        // function body: block (do ... end)
        // Note EBNF function-body = block ; phase 1 only block
        let body = self.parse_block()?;
        let span = Span::new(start_span.start, body.span.end);
        Ok(Function{ret_ty, name, name_span, params, body, span})
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        let start = self.expect(Token::Do, "expected `do` to start block")?.span.start;
        self.consume_newlines();
        let mut stmts = Vec::new();
        while !self.is_eof() && self.peek_token() != Some(&Token::End) {
            // allow blank lines inside block
            if matches!(self.peek_token(), Some(Token::Newline) | Some(Token::Semicolon)) { self.advance(); continue; }
            let stmt = self.parse_stmt()?;
            stmts.push(stmt);
            // terminators already handled inside parse_stmt; consume extra newlines
            self.consume_newlines();
        }
        let end = self.expect(Token::End, "expected `end` to close block")?.span.end;
        Ok(Block{stmts, span: Span::new(start, end)})
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        match self.peek_token() {
            Some(Token::If) => { let s = self.parse_if()?; Ok(Stmt::If(s)) }
            Some(Token::While) => { let s = self.parse_while()?; Ok(Stmt::While(s)) }
            Some(Token::Return) => { let s = self.parse_return()?; Ok(Stmt::Return(s)) }
            Some(Token::Do) => { let b = self.parse_block()?; Ok(Stmt::Block(b)) }
            Some(Token::Int) | Some(Token::Bool) | Some(Token::Void) => {
                // Lookahead: if type ident -> var decl, else expr stmt starting with type? but type as expr not valid, so decl
                // Peek next non-type token is ident => var decl
                let is_decl = self.tokens.get(self.pos+1).map(|st| st.token == Token::Ident).unwrap_or(false);
                if is_decl {
                    let d = self.parse_var_decl()?;
                    Ok(Stmt::VarDecl(d))
                } else {
                    let e = self.parse_expr_stmt()?;
                    Ok(Stmt::Expr(e))
                }
            }
            _ => {
                // expression statement
                let e = self.parse_expr_stmt()?;
                Ok(Stmt::Expr(e))
            }
        }
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
        Ok(VarDecl{ty, name, name_span, init, span})
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
                Some(Block{stmts: vec![Stmt::If(nested_if)], span})
            } else {
                Some(self.parse_block()?)
            }
        } else { None };
        let end = else_block.as_ref().map(|b| b.span.end).unwrap_or(then_block.span.end);
        Ok(IfStmt{cond, then_block, else_block, span: Span::new(start, end)})
    }

    fn parse_while(&mut self) -> Result<WhileStmt, ParseError> {
        let start = self.expect(Token::While, "while")?.span.start;
        self.consume_if(Token::LParen);
        let cond = self.parse_expr()?;
        self.consume_if(Token::RParen);
        let body = self.parse_block()?;
        let span = Span::new(start, body.span.end);
        Ok(WhileStmt{cond, body, span})
    }

    fn parse_return(&mut self) -> Result<ReturnStmt, ParseError> {
        let start = self.expect(Token::Return, "return")?.span.start;
        // return may have expr or not; peek terminator/end/else
        let needs_semi = matches!(self.peek_token(), Some(Token::Newline) | Some(Token::Semicolon) | Some(Token::End) | None);
        let value = if needs_semi { None } else { Some(self.parse_expr()?) };
        let end = value.as_ref().map(|e| e.span.end).unwrap_or(start+6);
        self.expect_terminator("return")?;
        let span = Span::new(start, end);
        Ok(ReturnStmt{value, span})
    }

    fn parse_expr_stmt(&mut self) -> Result<ExprStmt, ParseError> {
        let expr = self.parse_expr()?;
        let span = expr.span;
        self.expect_terminator("expression statement")?;
        Ok(ExprStmt{expr, span})
    }

    // ── Expressions (Pratt) ────────────────────────────────────────────

    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        let lhs = self.parse_or()?;
        if self.peek_token() == Some(&Token::Eq) {
            // Only allow ident as assignment target for Phase 1
            let target_ident = match lhs.kind {
                ExprKind::Ident(ref s) => s.clone(),
                _ => return Err(ParseError{message:"assignment target must be identifier".into(), span: lhs.span}),
            };
            let target_span = lhs.span;
            self.advance(); // consume =
            let rhs = self.parse_assignment()?; // right-assoc
            let span = Span::new(target_span.start, rhs.span.end);
            return Ok(Expr{kind: ExprKind::Assign{target: target_ident, target_span, value: Box::new(rhs)}, span});
        }
        Ok(lhs)
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_and()?;
        while self.peek_token() == Some(&Token::Or) {
            self.advance();
            let rhs = self.parse_and()?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr{kind: ExprKind::Binary{op: BinOp::Or, lhs: Box::new(lhs), rhs: Box::new(rhs)}, span};
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut lhs = self.parse_equality()?;
        while self.peek_token() == Some(&Token::And) {
            self.advance();
            let rhs = self.parse_equality()?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr{kind: ExprKind::Binary{op: BinOp::And, lhs: Box::new(lhs), rhs: Box::new(rhs)}, span};
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
                    let next_is_not = self.tokens.get(self.pos+1).map(|st| st.token == Token::Not).unwrap_or(false)
                        && {
                            // need to ensure slice gap is whitespace? For now treat any adjacent Is Not as IsNot
                            true
                        };
                    if next_is_not {
                        // consume Is + Not
                        self.advance(); self.advance();
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
            lhs = Expr{kind: ExprKind::Binary{op, lhs: Box::new(lhs), rhs: Box::new(rhs)}, span};
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
            lhs = Expr{kind: ExprKind::Binary{op, lhs: Box::new(lhs), rhs: Box::new(rhs)}, span};
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
            lhs = Expr{kind: ExprKind::Binary{op, lhs: Box::new(lhs), rhs: Box::new(rhs)}, span};
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
            lhs = Expr{kind: ExprKind::Binary{op, lhs: Box::new(lhs), rhs: Box::new(rhs)}, span};
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.peek_token() == Some(&Token::Not) {
            let start = self.advance().unwrap().span.start;
            let expr = self.parse_unary()?;
            let span = Span::new(start, expr.span.end);
            return Ok(Expr{kind: ExprKind::Unary{op: UnaryOp::Not, expr: Box::new(expr)}, span});
        }
        if self.peek_token() == Some(&Token::Minus) {
            let start = self.advance().unwrap().span.start;
            let expr = self.parse_unary()?;
            let span = Span::new(start, expr.span.end);
            return Ok(Expr{kind: ExprKind::Unary{op: UnaryOp::Neg, expr: Box::new(expr)}, span});
        }
        if self.peek_token() == Some(&Token::Plus) {
            let start = self.advance().unwrap().span.start;
            let expr = self.parse_unary()?;
            let span = Span::new(start, expr.span.end);
            return Ok(Expr{kind: ExprKind::Unary{op: UnaryOp::Pos, expr: Box::new(expr)}, span});
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            // call: '(' [args] ')'
            if self.peek_token() == Some(&Token::LParen) {
                // only Ident callee for Phase 1
                let callee_name = match &expr.kind {
                    ExprKind::Ident(s) => s.clone(),
                    _ => break, // non-ident call not handled Phase 1
                };
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
            break;
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let st = self.peek().cloned().ok_or(ParseError{message:"expected expression".into(), span: Span::new(self.source.len(), self.source.len())})?;
        match st.token {
            Token::IntLit | Token::HexInt | Token::BinInt => {
                self.advance();
                let val = self.parse_int_lit(st.span)?;
                Ok(Expr{kind: ExprKind::IntLit(val), span: st.span})
            }
            Token::True => { self.advance(); Ok(Expr{kind: ExprKind::BoolLit(true), span: st.span}) }
            Token::False => { self.advance(); Ok(Expr{kind: ExprKind::BoolLit(false), span: st.span}) }
            Token::Ident => { self.advance(); Ok(Expr{kind: ExprKind::Ident(self.slice(st.span).to_string()), span: st.span}) }
            Token::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                let end = self.expect(Token::RParen, "expected `)`")?.span.end;
                let span = Span::new(st.span.start, end);
                Ok(Expr{kind: ExprKind::Paren(Box::new(inner)), span})
            }
            _ => Err(ParseError{message: format!("expected expression, found `{}`", st.token), span: st.span}),
        }
    }
}

pub fn parse(tokens: Vec<SpannedToken>, source: String) -> Result<Program, ParseError> {
    let mut p = Parser::new(tokens, source);
    p.parse_program()
}
