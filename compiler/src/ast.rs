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
pub struct Attribute {
    pub name: String,
    pub name_span: Span,
    pub args: Vec<AttributeArg>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeArg {
    Expr(Expr),
    Named(String, Span, Expr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    Import(ImportDecl),
    Function(Function),
    Struct(StructDecl),
    Class(ClassDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    Typedef(TypedefDecl),
    Distinct(DistinctDecl),
    Extension(ExtensionDecl),
    Extern(ExternDecl),
    Init(Block),
    Const(ConstDecl),
    Attributed { attrs: Vec<Attribute>, item: Box<Item> },
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
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Option<WhereClause>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamMode {
    None,
    Ref,
    Out,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub mode: ParamMode,
    pub ty: Type,
    pub name: String,
    pub name_span: Span,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenericParam {
    pub name: String,
    pub name_span: Span,
    pub bounds: Vec<Type>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhereClause {
    pub constraints: Vec<WhereConstraint>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhereConstraint {
    pub ty: Type,
    pub bounds: Vec<Type>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Int(Span),
    Bool(Span),
    Void(Span),
    String(Span),
    Char(Span),
    Float(Span),
    Double(Span),
    Named(String, Span),       // Phase 2: struct name
    Generic(String, Vec<Type>, Span), // Phase 5: Box<int>
    FunctionType(Box<Type>, Vec<Type>, Span), // function<Ret(Args)>
    Tuple(Vec<Type>, Span),
    Any(Span),
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
            | Type::Float(s)
            | Type::Double(s)
            | Type::Named(_, s)
            | Type::Generic(_, _, s)
            | Type::FunctionType(_, _, s)
            | Type::Tuple(_, s)
            | Type::Any(s)
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
            Type::Float(_) => "float".into(),
            Type::Double(_) => "double".into(),
            Type::Named(n, _) => n.clone(),
            Type::Generic(n, args, _) => format!("{}<{}>", n, args.iter().map(|a| a.name()).collect::<Vec<_>>().join(", ")),
            Type::FunctionType(ret, args, _) => format!("function<{}({})>", ret.name(), args.iter().map(|a| a.name()).collect::<Vec<_>>().join(", ")),
            Type::Tuple(tys, _) => format!("({})", tys.iter().map(|t| t.name()).collect::<Vec<_>>().join(", ")),
            Type::Any(_) => "any".into(),
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
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Option<WhereClause>,
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
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Option<WhereClause>,
    pub extends: Option<Type>,
    pub implements: Vec<Type>,
    pub fields: Vec<StructField>,
    pub methods: Vec<Function>,
    pub constructors: Vec<ConstructorDecl>,
    pub destructors: Vec<DestructorDecl>,
    pub properties: Vec<PropertyDecl>,
    pub operators: Vec<OperatorDecl>,
    pub conversions: Vec<ConversionDecl>,
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
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Option<WhereClause>,
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
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Option<WhereClause>,
    pub span: Span,
}

// Phase 4: enum (EBNF §28)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumDecl {
    pub name: String,
    pub name_span: Span,
    pub generic_params: Vec<GenericParam>,
    pub where_clause: Option<WhereClause>,
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

// Phase 5: type aliases and distinct
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedefDecl {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub visibility: Visibility,
    pub generic_params: Vec<GenericParam>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistinctDecl {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub visibility: Visibility,
    pub generic_params: Vec<GenericParam>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionDecl {
    pub ty: Type,
    pub members: Vec<ExtensionMember>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionMember {
    Field(StructField),
    Function(Function),
    Operator(OperatorDecl),
    Property(PropertyDecl),
    Conversion(ConversionDecl),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorDecl {
    pub visibility: Visibility,
    pub is_static: bool,
    pub op: String,
    pub op_span: Span,
    pub params: Vec<Param>,
    pub body: Block,
    pub where_clause: Option<WhereClause>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionDecl {
    pub visibility: Visibility,
    pub is_explicit: bool,
    pub from_ty: Type,
    pub to_ty: Type,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternDecl {
    pub lib: String,
    pub lib_span: Span,
    pub file: String,
    pub file_span: Span,
    pub members: Vec<ExternMember>,
    pub visibility: Visibility,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternMember {
    Function { ty: Type, name: String, name_span: Span, params: Vec<ExternParam>, span: Span },
    Struct { name: String, name_span: Span, fields: Vec<ExternField>, span: Span },
    Enum { name: String, name_span: Span, variants: Vec<EnumVariant>, span: Span },
    Const { ty: Type, name: String, name_span: Span, span: Span },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternParam {
    pub ty: Type,
    pub name: String,
    pub name_span: Span,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternField {
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
    Const(ConstDecl),
    Destructure(DestructureStmt),
    Assert(AssertStmt),
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
pub struct DestructureStmt {
    pub targets: Vec<DestructureTarget>,
    pub expr: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DestructureTarget {
    Ident(String, Span),
    Wildcard(Span),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertStmt {
    pub is_debug: bool,
    pub cond: Expr,
    pub message: Option<Expr>,
    pub span: Span,
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
pub struct ConstDecl {
    pub visibility: Visibility,
    pub ty: Option<Type>,
    pub name: String,
    pub name_span: Span,
    pub init: Expr,
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
pub enum InterpolatedPart {
    Literal(String),
    Expr(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosureBody {
    Expr(Box<Expr>),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallArg {
    Expr(Expr),
    Named { name: String, name_span: Span, value: Expr, span: Span },
    Out { ty: Option<Type>, name: String, name_span: Span, span: Span },
    Ref { expr: Box<Expr>, span: Span },
}

impl CallArg {
    pub fn span(&self) -> Span {
        match self {
            CallArg::Expr(e) => e.span,
            CallArg::Named { span, .. } => *span,
            CallArg::Out { span, .. } => *span,
            CallArg::Ref { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExprKind {
    IntLit(i64),
    FloatLit(String),
    BoolLit(bool),
    StringLit(String),
    InterpolatedString(Vec<InterpolatedPart>, Span),
    CharLit(char),
    Ident(String),
    This,
    Super,
    Null,
    Paren(Box<Expr>),
    Tuple(Vec<Expr>),
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
    CompoundAssign {
        op: BinOp,
        lhs: Box<Expr>,
        value: Box<Expr>,
    },
    Conditional {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
    Range {
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        inclusive: bool,
    },
    Postfix {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    NullableMemberAccess {
        object: Box<Expr>,
        field: String,
        field_span: Span,
    },
    Call {
        callee: String,
        callee_span: Span,
        args: Vec<CallArg>,
        type_args: Vec<Type>, // Phase 5: generic args like foo<int>(x)
    },
    MethodCall {
        object: Box<Expr>,
        method: String,
        method_span: Span,
        args: Vec<CallArg>,
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
    Slice {
        object: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        inclusive: bool,
    }, // a[l..r] / a[..r] / a[l..] / a[..] per EBNF §8 index-or-range
    StructLit {
        ty: Type,
        fields: Vec<(String, Span, Expr)>,
    }, // Type has field = expr ... end
    EnumVariant {
        enum_name: Option<String>, // qualified prefix if any, e.g. Option in Option.Some
        variant: String,
        variant_span: Span,
        args: Vec<CallArg>,
    },
    Match(MatchExpr),
    Closure {
        params: Vec<Param>,
        body: Box<ClosureBody>,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg, // -
    Not, // not
    Pos, // + (unary plus, no-op)
    BitNot, // ~
    Inc, // ++ prefix/postfix
    Dec, // -- prefix/postfix
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
    BitAnd, // &
    BitOr, // |
    BitXor, // ^
    Shl, // <<
    Shr, // >>
    NullCoalesce, // ??
    Range, // ..
    RangeInclusive, // ..=
    CompoundAdd, // +=
    CompoundSub, // -=
    CompoundMul, // *=
    CompoundDiv, // /=
    CompoundMod, // %=
    CompoundBitAnd, // &=
    CompoundBitOr, // |=
    CompoundBitXor, // ^=
    CompoundShl, // <<=
    CompoundShr, // >>=
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
