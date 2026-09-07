//! Holt AST — mirrors EBNF §6-8, §13-21 for Phase 1 subset.
//! Every node carries a `Span` (see SKILL.md: Spans everywhere).

use crate::token::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub items: Vec<Item>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportDecl {
    pub path: Vec<String>,
    pub path_span: Span,
    pub symbols: Option<Vec<(String, Span)>>, // None = whole module
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Import(ImportDecl),
    Function(Function),
    Struct(StructDecl),
    Class(ClassDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum Visibility {
    Public,
    Private,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub ret_ty: Type,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub body: Block,
    pub visibility: Visibility,
    pub is_static: bool,
    pub is_sealed: bool,
    pub is_override: bool,
    pub is_open: bool,
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
    String(Span),
    Char(Span),
    Named(String, Span),       // Phase 2: struct name
    Array(Box<Type>, Span),    // T[]  (Phase 2)
    Pointer(Box<Type>, Span),  // T*  (Phase 2)
    Optional(Box<Type>, Span), // T? (Phase 2 future)
}

impl Type {
    pub fn span(&self) -> Span {
        match self {
            Type::Int(s)
            | Type::Bool(s)
            | Type::Void(s)
            | Type::String(s)
            | Type::Char(s)
            | Type::Named(_, s)
            | Type::Array(_, s)
            | Type::Pointer(_, s)
            | Type::Optional(_, s) => *s,
        }
    }
    pub fn name(&self) -> String {
        match self {
            Type::Int(_) => "int".into(),
            Type::Bool(_) => "bool".into(),
            Type::Void(_) => "void".into(),
            Type::String(_) => "string".into(),
            Type::Char(_) => "char".into(),
            Type::Named(n, _) => n.clone(),
            Type::Array(el, _) => format!("{}[]", el.name()),
            Type::Pointer(el, _) => format!("{}*", el.name()),
            Type::Optional(el, _) => format!("{}?", el.name()),
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
    pub visibility: Visibility,
    pub span: Span,
}

// Phase 4: class
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDecl {
    pub name: String,
    pub name_span: Span,
    pub is_open: bool,
    pub is_sealed: bool,
    pub extends: Option<Type>,
    pub implements: Vec<Type>,
    pub fields: Vec<StructField>,
    pub methods: Vec<Function>,
    pub constructors: Vec<ConstructorDecl>,
    pub destructors: Vec<DestructorDecl>,
    pub properties: Vec<PropertyDecl>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructorDecl {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub body: Option<Block>,
    pub visibility: Visibility,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestructorDecl {
    pub name: String,
    pub name_span: Span,
    pub body: Block,
    pub visibility: Visibility,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyDecl {
    pub ty: Option<Type>,
    pub name: String,
    pub name_span: Span,
    pub visibility: Visibility,
    pub getter: Option<Block>,
    pub setter: Option<(Param, Block)>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitDecl {
    pub name: String,
    pub name_span: Span,
    pub methods: Vec<TraitMethod>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitMethod {
    pub ret_ty: Type,
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub is_sealed: bool,
    pub span: Span,
}

// Phase 4: enum (EBNF §28)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDecl {
    pub name: String,
    pub name_span: Span,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub name: String,
    pub name_span: Span,
    pub discriminant: Option<i64>,
    pub payload_ty: Option<Type>, // minimal single payload type e.g. Some(int)
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
    Loop(LoopStmt),
    For(ForStmt),
    Return(ReturnStmt),
    Expr(ExprStmt),
    Block(Block),
    Break(BreakStmt),
    Continue(ContinueStmt),
    Defer(DeferStmt),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakStmt {
    pub label: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueStmt {
    pub label: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopStmt {
    pub label: Option<String>,
    pub label_span: Option<Span>,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForStmt {
    pub label: Option<String>,
    pub label_span: Option<Span>,
    pub var: String,
    pub var_span: Span,
    pub iter: Expr,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferStmt {
    pub inner: DeferInner,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeferInner {
    Expr(Box<Expr>),
    Block(Block),
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
    StringLit(String),
    CharLit(char),
    Ident(String),
    This,
    Paren(Box<Expr>),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Assign {
        lhs: Box<Expr>,
        value: Box<Expr>,
    },
    Call {
        callee: String,
        callee_span: Span,
        args: Vec<Expr>,
    },
    MethodCall {
        object: Box<Expr>,
        method: String,
        method_span: Span,
        args: Vec<Expr>,
    },
    MemberAccess {
        object: Box<Expr>,
        field: String,
        field_span: Span,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    }, // a[i] Phase 2
    StructLit {
        ty: Type,
        fields: Vec<(String, Span, Expr)>,
    }, // Type has field = expr ... end
    EnumVariant {
        enum_name: Option<String>, // qualified prefix if any, e.g. Option in Option.Some
        variant: String,
        variant_span: Span,
        args: Vec<Expr>,
    },
    Match(MatchExpr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg, // -
    Not, // not
    Pos, // + (unary plus, no-op)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Le,
    Gt,
    Ge,
    Is,
    IsNot, // "is" / "is not"
    And,
    Or,
}

// Phase 2: simple match (EBNF §10, §16)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchExpr {
    pub scrutinee: Box<Expr>,
    pub arms: Vec<MatchArm>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: MatchArmBody,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    Wildcard(Span), // _
    Var(String, Span),
    LitInt(i64, Span),
    LitBool(bool, Span),
    Enum {
        variant: String,
        variant_span: Span,
        payload: Option<Box<Pattern>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchArmBody {
    Expr(Box<Expr>),
    Block(Block), // `do ... end` – evaluated for value via last expr? For now treat as block with return
}
