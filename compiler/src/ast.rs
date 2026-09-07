//! Holt AST — mirrors EBNF §6-8, §13-21 for Phase 1 subset.
//! Every node carries a `Span` (see SKILL.md: Spans everywhere).

use crate::token::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub items: Vec<Item>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Function(Function),
    Struct(StructDecl),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub ret_ty: Type,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub ty: Type,
    pub name: String,
    pub name_span: Span,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int(Span),
    Bool(Span),
    Void(Span),
    Named(String, Span), // Phase 2: struct name
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Int(s) | Type::Bool(s) | Type::Void(s) | Type::Named(_, s) => *s,
        }
    }
    pub fn name(&self) -> String {
        match self {
            Type::Int(_) => "int".into(),
            Type::Bool(_) => "bool".into(),
            Type::Void(_) => "void".into(),
            Type::Named(n, _) => n.clone(),
        }
    }
    pub fn is_void(&self) -> bool {
        matches!(self, Type::Void(_))
    }
}

// Phase 2: struct declaration + member
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructDecl {
    pub name: String,
    pub name_span: Span,
    pub fields: Vec<StructField>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructField {
    pub ty: Type,
    pub name: String,
    pub name_span: Span,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    VarDecl(VarDecl),
    If(IfStmt),
    While(WhileStmt),
    Return(ReturnStmt),
    Expr(ExprStmt),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarDecl {
    pub ty: Type,
    pub name: String,
    pub name_span: Span,
    pub init: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IfStmt {
    pub cond: Expr,
    pub then_block: Block,
    pub else_block: Option<Block>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhileStmt {
    pub cond: Expr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnStmt {
    pub value: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprStmt {
    pub expr: Expr,
    pub span: Span,
}

// ── Expressions (§8) ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    IntLit(i64),
    BoolLit(bool),
    Ident(String),
    Paren(Box<Expr>),
    Unary { op: UnaryOp, expr: Box<Expr> },
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    Assign { lhs: Box<Expr>, value: Box<Expr> },
    Call { callee: String, callee_span: Span, args: Vec<Expr> },
    MemberAccess { object: Box<Expr>, field: String, field_span: Span },
    StructLit { ty: Type, fields: Vec<(String, Span, Expr)> }, // Type has field = expr ... end
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,      // -
    Not,      // not
    Pos,      // + (unary plus, no-op)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Lt, Le, Gt, Ge,
    Is, IsNot, // "is" / "is not"
    And, Or,
}
