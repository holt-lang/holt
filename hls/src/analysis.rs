//! Symbol analysis built on the compiler AST.
//!
//! Walks a parsed `Program` into a lightweight symbol table (top-level
//! declarations plus local variables/parameters scoped to their block), then
//! answers the LSP features we support:
//!
//! - `textDocument/hover`
//! - `textDocument/definition`
//! - `textDocument/completion`
//! - `textDocument/documentSymbol`

use compiler::ast::{self, ExprKind, Item, Stmt};
use compiler::token::Span;
use lsp_types::{
    CompletionItem, CompletionItemKind, DocumentSymbol, Hover, HoverContents, Location, MarkupContent,
    MarkupKind, SymbolKind,
};

use crate::document::span_to_range;

/// Kinds of symbols we track (shared between LSP symbol kinds and our
/// internal detail formatting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymKind {
    Function,
    Method,
    Struct,
    Class,
    Enum,
    Trait,
    Typedef,
    Distinct,
    Constant,
    Variable,
    Parameter,
    Field,
    Property,
    Variant,
    Module,
}

impl SymKind {
    fn lsp(self) -> SymbolKind {
        use SymKind::*;
        match self {
            Function => SymbolKind::FUNCTION,
            Method => SymbolKind::METHOD,
            Struct => SymbolKind::STRUCT,
            Class => SymbolKind::CLASS,
            Enum => SymbolKind::ENUM,
            Trait => SymbolKind::INTERFACE,
            Typedef | Distinct => SymbolKind::TYPE_PARAMETER,
            Constant => SymbolKind::CONSTANT,
            Variable | Parameter => SymbolKind::VARIABLE,
            Field => SymbolKind::FIELD,
            Property => SymbolKind::PROPERTY,
            Variant => SymbolKind::ENUM_MEMBER,
            Module => SymbolKind::MODULE,
        }
    }

    fn completion(self) -> CompletionItemKind {
        use SymKind::*;
        match self {
            Function | Method => CompletionItemKind::FUNCTION,
            Struct | Class | Enum | Trait => CompletionItemKind::STRUCT,
            Typedef | Distinct => CompletionItemKind::TYPE_PARAMETER,
            Constant => CompletionItemKind::CONSTANT,
            Variable | Parameter => CompletionItemKind::VARIABLE,
            Field | Property => CompletionItemKind::FIELD,
            Variant => CompletionItemKind::ENUM_MEMBER,
            Module => CompletionItemKind::MODULE,
        }
    }

    fn heading(self) -> &'static str {
        use SymKind::*;
        match self {
            Function | Method => "function/method",
            Struct => "struct",
            Class => "class",
            Enum => "enum",
            Trait => "trait",
            Typedef => "typedef",
            Distinct => "distinct",
            Constant => "constant",
            Variable | Parameter => "variable",
            Field => "field",
            Property => "property",
            Variant => "enum member",
            Module => "module",
        }
    }
}

/// A single named declaration.
#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub kind: SymKind,
    /// Byte span of the identifier (used for goto-def, selection range).
    pub name_span: Span,
    /// Byte span of the whole declaration.
    pub full_span: Span,
    /// Human-readable signature/type shown in hover & completion.
    pub detail: String,
    /// Byte span of the enclosing scope (block/decl). `None` for top-level.
    pub scope: Option<Span>,
    /// Child symbols (struct fields, class members, enum variants).
    pub children: Vec<Symbol>,
}

impl Symbol {
    fn contains(&self, offset: usize) -> bool {
        self.name_span.start <= offset && offset <= self.name_span.end
    }
    fn with_scope(mut self, scope: Span) -> Self {
        self.scope = Some(scope);
        self
    }
    fn kind_heading(&self) -> &'static str {
        self.kind.heading()
    }
}

/// Full-file analysis: top-level symbols plus all lexically scoped locals.
#[derive(Debug, Default)]
pub struct Analysis {
    top: Vec<Symbol>,
    locals: Vec<Symbol>,
}
impl Analysis {
    /// Build a symbol table from a successfully parsed program.
    pub fn from_program(prog: &ast::Program) -> Self {
        let mut a = Analysis::default();
        for item in &prog.items {
            a.collect_item(item);
        }
        a
    }

    fn push_top(&mut self, sym: Symbol, name_span: Span) {
        // Avoid duplicate imports colliding; keep first occurrence by name.
        if !self.top.iter().any(|s| s.name_span == name_span) {
            self.top.push(sym);
        }
    }
fn collect_item(&mut self, item: &Item) {
        match item {
            Item::Function(f) => {
                let detail = format!(
                    "fn {}({}) -> {}",
                    f.name,
                    format_params(&f.params),
                    f.ret_ty.name()
                );
                let sym = make_symbol(
                    &f.name,
                    SymKind::Function,
                    f.name_span,
                    f.span,
                    detail,
                    Vec::new(),
                );
                self.push_top(sym, f.name_span);
                self.collect_block_locals(&f.body, f.span, &f.params);
            }
            Item::Struct(s) => {
                let children = s
                    .fields
                    .iter()
                    .map(|f| {
                        make_symbol(
                            &f.name,
                            SymKind::Field,
                            f.name_span,
                            f.span,
                            format!("{} {}", f.ty.name(), f.name),
                            Vec::new(),
                        )
                    })
                    .collect();
                let sym = make_symbol(
                    &s.name,
                    SymKind::Struct,
                    s.name_span,
                    s.span,
                    format!("struct {}", s.name),
                    children,
                );
                self.push_top(sym, s.name_span);
            }
            Item::Class(c) => {
                let mut children = Vec::new();
                for f in &c.fields {
                    children.push(make_symbol(
                        &f.name,
                        SymKind::Field,
                        f.name_span,
                        f.span,
                        format!("{} {}", f.ty.name(), f.name),
                        Vec::new(),
                    ));
                }
                for m in &c.methods {
                    let detail = format!(
                        "fn {}({}) -> {}",
                        m.name,
                        format_params(&m.params),
                        m.ret_ty.name()
                    );
                    children.push(make_symbol(
                        &m.name,
                        SymKind::Method,
                        m.name_span,
                        m.span,
                        detail,
                        Vec::new(),
                    ));
                    self.collect_block_locals(&m.body, m.span, &m.params);
                }
                let sym = make_symbol(
                    &c.name,
                    SymKind::Class,
                    c.name_span,
                    c.span,
                    format!("class {}", c.name),
                    children,
                );
                self.push_top(sym, c.name_span);
            }
            Item::Enum(e) => {
                let children = e
                    .variants
                    .iter()
                    .map(|v| {
                        make_symbol(
                            &v.name,
                            SymKind::Variant,
                            v.name_span,
                            v.span,
                            format!("variant {}", v.name),
                            Vec::new(),
                        )
                    })
                    .collect();
                let sym = make_symbol(
                    &e.name,
                    SymKind::Enum,
                    e.name_span,
                    e.span,
                    format!("enum {}", e.name),
                    children,
                );
                self.push_top(sym, e.name_span);
            }
Item::Trait(t) => {
                let sym = make_symbol(
                    &t.name,
                    SymKind::Trait,
                    t.name_span,
                    t.span,
                    format!("trait {}", t.name),
                    Vec::new(),
                );
                self.push_top(sym, t.name_span);
                for m in &t.methods {
                    self.collect_params(&m.params, m.span);
                }
            }
            Item::Typedef(t) => {
                let sym = make_symbol(
                    &t.name,
                    SymKind::Typedef,
                    t.name_span,
                    t.span,
                    format!("typedef {} = {}", t.name, t.ty.name()),
                    Vec::new(),
                );
                self.push_top(sym, t.name_span);
            }
            Item::Distinct(d) => {
                let sym = make_symbol(
                    &d.name,
                    SymKind::Distinct,
                    d.name_span,
                    d.span,
                    format!("distinct {} = {}", d.name, d.ty.name()),
                    Vec::new(),
                );
                self.push_top(sym, d.name_span);
            }
            Item::Const(c) => {
                let ty = c.ty.as_ref().map(|t| t.name()).unwrap_or_else(|| "*".into());
                let sym = make_symbol(
                    &c.name,
                    SymKind::Constant,
                    c.name_span,
                    c.span,
                    format!("const {}: {}", c.name, ty),
                    Vec::new(),
                );
                self.push_top(sym, c.name_span);
            }
            Item::Var(v) => {
                let sym = make_symbol(
                    &v.name,
                    SymKind::Variable,
                    v.name_span,
                    v.span,
                    format!("var {}: {}", v.name, v.ty.name()),
                    Vec::new(),
                );
                self.push_top(sym, v.name_span);
            }
            Item::Import(imp) => {
                let name = imp.path.join("::");
                let sym = make_symbol(
                    &name,
                    SymKind::Module,
                    imp.path_span,
                    imp.span,
                    format!("module {}", name),
                    Vec::new(),
                );
                self.push_top(sym, imp.path_span);
            }
            Item::Extern(e) => {
                for m in &e.members {
                    match m {
                        ast::ExternMember::Function { name, name_span, span, .. }
                        | ast::ExternMember::Struct { name, name_span, span, .. }
                        | ast::ExternMember::Enum { name, name_span, span, .. }
                        | ast::ExternMember::Const { name, name_span, span, .. } => {
                            let sym = make_symbol(
                                name,
                                SymKind::Function,
                                *name_span,
                                *span,
                                format!("extern {}", name),
                                Vec::new(),
                            );
                            self.push_top(sym, *name_span);
                        }
                    }
                }
            }
            Item::Init(b) => self.collect_block_locals(b, b.span, &[]),
            Item::Extension(_) => {}
            Item::Attributed { item, .. } => self.collect_item(item),
        }
    }

    fn collect_params(&mut self, params: &[ast::Param], scope: Span) {
        for p in params {
            let detail = format!("{} {}", p.ty.name(), p.name);
            let sym = make_symbol(
                &p.name,
                SymKind::Parameter,
                p.name_span,
                p.span,
                detail,
                Vec::new(),
            );
            self.locals.push(sym.with_scope(scope));
        }
    }

    fn collect_block_locals(&mut self, block: &ast::Block, scope: Span, params: &[ast::Param]) {
        self.collect_params(params, scope);
        self.collect_block(block, scope);
    }

    fn collect_block(&mut self, block: &ast::Block, scope: Span) {
        for stmt in &block.stmts {
            self.collect_stmt(stmt, scope);
        }
    }
fn collect_stmt(&mut self, stmt: &Stmt, scope: Span) {
        match stmt {
            Stmt::VarDecl(v) => {
                let detail = format!("var {}: {}", v.name, v.ty.name());
                let sym = make_symbol(
                    &v.name,
                    SymKind::Variable,
                    v.name_span,
                    v.span,
                    detail,
                    Vec::new(),
                )
                .with_scope(scope);
                self.locals.push(sym);
                if let Some(init) = &v.init {
                    self.collect_expr(init);
                }
            }
            Stmt::Const(c) => {
                let ty = c.ty.as_ref().map(|t| t.name()).unwrap_or_else(|| "*".into());
                let detail = format!("const {}: {}", c.name, ty);
                let sym = make_symbol(
                    &c.name,
                    SymKind::Constant,
                    c.name_span,
                    c.span,
                    detail,
                    Vec::new(),
                )
                .with_scope(scope);
                self.locals.push(sym);
                self.collect_expr(&c.init);
            }
            Stmt::Destructure(d) => {
                for t in &d.targets {
                    if let ast::DestructureTarget::Ident(name, nspan) = t {
                        let sym = make_symbol(
                            name,
                            SymKind::Variable,
                            *nspan,
                            *nspan,
                            name.clone(),
                            Vec::new(),
                        )
                        .with_scope(scope);
                        self.locals.push(sym);
                    }
                }
                self.collect_expr(&d.expr);
            }
            Stmt::If(s) => {
                self.collect_expr(&s.cond);
                self.collect_block(&s.then_block, scope);
                if let Some(b) = &s.else_block {
                    self.collect_block(b, scope);
                }
            }
            Stmt::While(s) => {
                self.collect_expr(&s.cond);
                self.collect_block(&s.body, scope);
            }
            Stmt::Loop(s) => self.collect_block(&s.body, scope),
            Stmt::For(s) => {
                let sym = make_symbol(
                    &s.var,
                    SymKind::Variable,
                    s.var_span,
                    s.span,
                    format!("for {}", s.var),
                    Vec::new(),
                )
                .with_scope(scope);
                self.locals.push(sym);
                if let Some((v2, s2)) = &s.var2 {
                    let sym2 = make_symbol(
                        v2,
                        SymKind::Variable,
                        *s2,
                        s.span,
                        format!("for {}", v2),
                        Vec::new(),
                    )
                    .with_scope(scope);
                    self.locals.push(sym2);
                }
                self.collect_expr(&s.iter);
                self.collect_block(&s.body, scope);
            }
            Stmt::Return(s) => {
                if let Some(v) = &s.value {
                    self.collect_expr(v);
                }
            }
            Stmt::Expr(e) => self.collect_expr(&e.expr),
            Stmt::Block(b) => self.collect_block(b, scope),
            Stmt::Assert(s) => {
                self.collect_expr(&s.cond);
                if let Some(m) = &s.message {
                    self.collect_expr(m);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Defer(d) => match &d.inner {
                ast::DeferInner::Expr(e) => self.collect_expr(e),
                ast::DeferInner::Block(b) => self.collect_block(b, scope),
            },
        }
    }

    fn collect_expr(&mut self, expr: &ast::Expr) {
        match &expr.kind {
            ExprKind::IntLit(_)
            | ExprKind::FloatLit(_)
            | ExprKind::BoolLit(_)
            | ExprKind::StringLit(_)
            | ExprKind::CharLit(_)
            | ExprKind::Ident(_)
            | ExprKind::This
            | ExprKind::Super
            | ExprKind::Null => {}
            ExprKind::InterpolatedString(parts, _) => {
                for p in parts {
                    if let ast::InterpolatedPart::Expr(e) = p {
                        self.collect_expr(e);
                    }
                }
            }
            ExprKind::Paren(e)
            | ExprKind::Unary { expr: e, .. }
            | ExprKind::Postfix { expr: e, .. } => self.collect_expr(e),
            ExprKind::Tuple(es) | ExprKind::ArrayLit(es) => {
                for e in es {
                    self.collect_expr(e);
                }
            }
            ExprKind::VecEmpty(_) => {}
            ExprKind::MapLit { entries, .. } => {
                for (k, v) in entries {
                    self.collect_expr(k);
                    self.collect_expr(v);
                }
            }
            ExprKind::Binary { lhs, rhs, .. } => {
                self.collect_expr(lhs);
                self.collect_expr(rhs);
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.collect_expr(s);
                }
                if let Some(e) = end {
                    self.collect_expr(e);
                }
            }
            ExprKind::Assign { lhs, value } | ExprKind::CompoundAssign { lhs, value, .. } => {
                self.collect_expr(lhs);
                self.collect_expr(value);
            }
            ExprKind::Conditional {
                cond,
                then_branch,
                else_branch,
            } => {
                self.collect_expr(cond);
                self.collect_expr(then_branch);
                self.collect_expr(else_branch);
            }
            ExprKind::NullableMemberAccess { object, .. } | ExprKind::MemberAccess { object, .. } => {
                self.collect_expr(object);
            }
            ExprKind::MethodCall { object, args, .. } => {
                self.collect_expr(object);
                self.collect_call_args(args);
            }
            ExprKind::Call { args, .. } => self.collect_call_args(args),
            ExprKind::Index { object, index } => {
                self.collect_expr(object);
                self.collect_expr(index);
            }
            ExprKind::Slice { object, start, end, .. } => {
                self.collect_expr(object);
                if let Some(s) = start {
                    self.collect_expr(s);
                }
                if let Some(e) = end {
                    self.collect_expr(e);
                }
            }
            ExprKind::StructLit { fields, .. } => {
                for (_, _, e) in fields {
                    self.collect_expr(e);
                }
            }
            ExprKind::EnumVariant { args, .. } => self.collect_call_args(args),
            ExprKind::Match(m) => {
                self.collect_expr(&m.scrutinee);
                for arm in &m.arms {
                    if let Some(g) = &arm.guard {
                        self.collect_expr(g);
                    }
                    match &arm.body {
                        ast::MatchArmBody::Expr(e) => self.collect_expr(e),
                        ast::MatchArmBody::Block(b) => self.collect_block(b, arm.span),
                    }
                }
            }
            ExprKind::Closure { body, .. } => match body.as_ref() {
                ast::ClosureBody::Expr(e) => self.collect_expr(e),
                ast::ClosureBody::Block(b) => self.collect_block(b, b.span),
            },
        }
    }

    fn collect_call_args(&mut self, args: &[ast::CallArg]) {
        for a in args {
            match a {
                ast::CallArg::Expr(e) => self.collect_expr(e),
                ast::CallArg::Named { value, .. } => self.collect_expr(value),
                ast::CallArg::Ref { expr, .. } => self.collect_expr(expr),
                ast::CallArg::Out { .. } => {}
            }
        }
    }
// ── Queries ───────────────────────────────────────────────────────

    /// All top-level symbols (including imported modules, useful for
    /// completion but filtered out of document symbols).
    pub fn top_symbols(&self) -> &[Symbol] {
        &self.top
    }

    /// Symbols whose identifier span covers `offset` (innermost first):
    /// top-level/children plus any local in that scope.
    pub fn symbols_at(&self, offset: usize) -> Vec<&Symbol> {
        let mut out = Vec::new();
        collect_containing(&self.top, offset, &mut out);
        out.extend(
            self.locals
                .iter()
                .filter(|l| l.contains(offset) && scope_covers(l, offset)),
        );
        out.sort_by_key(|s| s.name_span.end - s.name_span.start);
        out
    }

    /// The innermost symbol at `offset`, if any.
    pub fn symbol_at(&self, offset: usize) -> Option<&Symbol> {
        self.symbols_at(offset).into_iter().next()
    }

    fn top_recursive(&self) -> Vec<&Symbol> {
        fn walk<'a>(syms: &'a [Symbol], out: &mut Vec<&'a Symbol>) {
            for s in syms {
                out.push(s);
                walk(&s.children, out);
            }
        }
        let mut v = Vec::new();
        walk(&self.top, &mut v);
        v
    }

    // ── LSP feature handlers ──────────────────────────────────────────

    pub fn hover(&self, source: &str, offset: usize) -> Option<Hover> {
        let sym = self.resolve_symbol_at(source, offset)?;
        let value = format!("**{}**\n\n```hlt\n{}\n```", sym.kind_heading(), sym.detail);
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: Some(span_to_range(source, sym.name_span)),
        })
    }

    pub fn definition(
        &self,
        source: &str,
        uri: &lsp_types::Uri,
        offset: usize,
    ) -> Option<Location> {
        let span = self.resolve_definition_span(source, offset)?;
        Some(Location {
            uri: uri.clone(),
            range: span_to_range(source, span),
        })
    }

    /// Resolve the symbol the cursor is on: either the declaration itself
    /// (a symbol whose name covers `offset`) or a *reference* to some
    /// top-level/local symbol whose name matches the word under the cursor.
    /// Powers hover (shows the referenced symbol's info) and goto-definition.
    fn resolve_symbol_at(&self, source: &str, offset: usize) -> Option<&Symbol> {
        // 1. Cursor on a declaration name.
        if let Some(sym) = self.symbol_at(offset) {
            return Some(sym);
        }
        // 2. Cursor on a reference: look up the word by name.
        let word = ident_at(source, offset)?;
        let local = self
            .locals
            .iter()
            .filter(|l| l.name == word && scope_covers(l, offset))
            .min_by_key(|l| l.name_span.end - l.name_span.start);
        if let Some(l) = local {
            return Some(l);
        }
        self.top_recursive()
            .into_iter()
            .find(|s| s.name == word)
    }

    /// Span of the declaration the identifier at `offset` refers to.
    fn resolve_definition_span(&self, source: &str, offset: usize) -> Option<Span> {
        let word = ident_at(source, offset)?;
        if let Some(l) = self
            .locals
            .iter()
            .filter(|l| l.name == word && scope_covers(l, offset))
            .min_by_key(|l| l.name_span.end - l.name_span.start)
        {
            return Some(l.name_span);
        }
        self.top_recursive()
            .into_iter()
            .find(|s| s.name == word)
            .map(|s| s.name_span)
    }

    pub fn document_symbols(&self, source: &str) -> Vec<DocumentSymbol> {
        self.top
            .iter()
            .filter(|s| s.kind != SymKind::Module)
            .map(|s| to_document_symbol(source, s))
            .collect()
    }

    pub fn completions(&self, source: &str, offset: usize) -> Vec<CompletionItem> {
        let prefix = word_prefix_at(source, offset).unwrap_or_default();
        let mut items = Vec::new();
        let matches = |name: &str| prefix.is_empty() || name.to_lowercase().starts_with(&prefix);
        // Locals in scope, declared before the cursor.
        for l in self
            .locals
            .iter()
            .filter(|l| scope_covers_prefix(l, offset))
        {
            if matches(&l.name) {
                items.push(completion_item(l.name.clone(), l.kind.completion(), &l.detail));
            }
        }
        // Top-level + nested symbols.
        for s in self.top_recursive() {
            if matches(&s.name) {
                items.push(completion_item(s.name.clone(), s.kind.completion(), &s.detail));
            }
        }
        // Keywords.
        for kw in KEYWORDS {
            if matches(kw) {
                items.push(completion_item(
                    (*kw).to_string(),
                    CompletionItemKind::KEYWORD,
                    &format!("keyword {kw}"),
                ));
            }
        }
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);
        items
    }
}
// ── Helpers ──────────────────────────────────────────────────────────

fn make_symbol(
    name: &str,
    kind: SymKind,
    name_span: Span,
    full_span: Span,
    detail: String,
    children: Vec<Symbol>,
) -> Symbol {
    Symbol {
        name: name.to_string(),
        kind,
        name_span,
        full_span,
        detail,
        scope: None,
        children,
    }
}

fn format_params(params: &[ast::Param]) -> String {
    params
        .iter()
        .map(|p| {
            let mut s = String::new();
            if p.is_variadic {
                s.push_str("...");
            }
            s.push_str(&p.ty.name());
            s.push(' ');
            s.push_str(&p.name);
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn collect_containing<'a>(syms: &'a [Symbol], offset: usize, out: &mut Vec<&'a Symbol>) {
    for s in syms {
        // Descend only into symbols whose *body* could contain the offset;
        // a symbol "matches" when the offset is on its name.
        if s.full_span.start <= offset && offset <= s.full_span.end {
            if s.name_span.start <= offset && offset <= s.name_span.end {
                out.push(s);
            }
            collect_containing(&s.children, offset, out);
        }
    }
}

/// Scope containment for symbol/hover/definition lookup: a local is visible
/// anywhere inside its enclosing scope (cursor may sit exactly on the decl).
fn scope_covers(l: &Symbol, offset: usize) -> bool {
    match l.scope {
        Some(scope) => scope.start <= offset && offset <= scope.end,
        None => true,
    }
}

/// Completion variant: additionally require the declaration to appear before
/// the cursor, so we don't offer a symbol in the middle of typing it.
fn scope_covers_prefix(l: &Symbol, offset: usize) -> bool {
    match l.scope {
        Some(scope) => scope.start <= offset && offset <= scope.end && l.name_span.end <= offset,
        None => true,
    }
}

fn to_document_symbol(source: &str, sym: &Symbol) -> DocumentSymbol {
    #[allow(deprecated)]
    DocumentSymbol {
        name: sym.name.clone(),
        detail: Some(sym.detail.clone()),
        kind: sym.kind.lsp(),
        tags: None,
        deprecated: None,
        range: span_to_range(source, sym.full_span),
        selection_range: span_to_range(source, sym.name_span),
        children: Some(
            sym.children
                .iter()
                .map(|c| to_document_symbol(source, c))
                .collect(),
        ),
    }
}

fn completion_item(label: String, kind: CompletionItemKind, detail: &str) -> CompletionItem {
    CompletionItem {
        label,
        kind: Some(kind),
        detail: Some(detail.to_string()),
        ..Default::default()
    }
}

/// The identifier/keyword at `offset`, if any. Works for a cursor anywhere
/// on/inside/at-either-edge-of a word by expanding over identifier
/// characters, rather than relying on token span boundaries (which are
/// contiguous, so an inclusive end-match on the previous token would
/// otherwise shadow the word's first character).
fn ident_at(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    if offset > bytes.len() {
        return None;
    }
    let mut lo = offset;
    while lo > 0 && (bytes[lo - 1].is_ascii_alphanumeric() || bytes[lo - 1] == b'_') {
        lo -= 1;
    }
    let mut hi = offset;
    while hi < bytes.len() && (bytes[hi].is_ascii_alphanumeric() || bytes[hi] == b'_') {
        hi += 1;
    }
    if lo == hi {
        return None;
    }
    Some(source[lo..hi].to_string())
}

/// Lowercased word prefix ending at `offset`, or `None` when not in a word.
fn word_prefix_at(source: &str, offset: usize) -> Option<String> {
    let bytes = source.as_bytes();
    let mut lo = offset;
    while lo > 0 && (bytes[lo - 1].is_ascii_alphanumeric() || bytes[lo - 1] == b'_') {
        lo -= 1;
    }
    let prefix = source[lo..offset].to_lowercase();
    if prefix.is_empty() {
        None
    } else {
        Some(prefix)
    }
}

/// Holt keywords offered in completion.
const KEYWORDS: &[&str] = &[
    "and", "any", "assert", "bool", "break", "char", "class", "const", "continue",
    "debug_assert", "defer", "distinct", "do", "double", "else", "end", "enum",
    "extends", "extend", "explicit", "extern", "float", "for", "function", "from",
    "get", "has", "if", "import", "implements", "in", "init", "initialize", "int",
    "is", "loop", "match", "not", "null", "open", "operator", "or", "out",
    "override", "private", "public", "ref", "return", "sealed", "set", "static",
    "string", "struct", "super", "this", "to", "trait", "typedef", "var", "void",
    "where", "while",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze(src: &str) -> Analysis {
        let out = compiler::lexer::lex(src);
        let prog = compiler::parse::parse(out.tokens, src.to_string()).unwrap();
        Analysis::from_program(&prog)
    }

    #[test]
    fn collects_top_level() {
        let a = analyze("void main() do\n  int x = 1\nend\nint foo(int a) do\n  return a\nend");
        let names: Vec<_> = a.top_symbols().iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"foo"));
    }

    #[test]
    fn collects_locals() {
        let a = analyze("void main() do\n  int x = 1\n  print(x)\nend");
        assert!(a.locals.iter().any(|l| l.name == "x"));
        let a = analyze("int foo(int a) do\n  bool b = true\n  return a\nend");
        assert!(a.locals.iter().any(|l| l.name == "a"));
        assert!(a.locals.iter().any(|l| l.name == "b"));
    }

    #[test]
    fn finds_symbol_at() {
        let src = "void main() do\n  int x = 1\nend\nint foo(int a) do\n  return a\nend";
        let a = analyze(src);
        let x_pos = src.find("int x").unwrap() + 4;
        assert_eq!(a.symbol_at(x_pos).map(|s| s.name.as_str()), Some("x"));
        let foo_name = src.find("foo").unwrap();
        assert_eq!(a.symbol_at(foo_name).map(|s| s.name.as_str()), Some("foo"));
    }

    #[test]
    fn definition_resolves_local_and_toppub() {
        let src = "void main() do\n  int x = 1\n  print(x)\nend\nint foo() do\n  return 1\nend\nvoid bar() do\n  foo()\nend";
        let a = analyze(src);
        // `print(x)` — the `x` argument is 6 bytes into the match ("print(").
        let x_off = src.find("print(x)").unwrap() + 6;
        assert_eq!(src.as_bytes()[x_off], b'x');
        assert_eq!(
            a.resolve_definition_span(src, x_off),
            Some(Span::new(
                src.find("int x").unwrap() + 4,
                src.find("int x").unwrap() + 5,
            ))
        );
        // `foo()` inside bar resolves to foo's declaration name_span.
        let foo_use = src.rfind("foo()").unwrap(); // the call inside bar
        assert_eq!(a.resolve_definition_span(src, foo_use).map(|s| &src[s.start..s.end]), Some("foo"));
    }
}
