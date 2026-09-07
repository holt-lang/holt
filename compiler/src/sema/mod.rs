//! Phase 2 semantic checks: structs, field access, literals.

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::token::Span;

#[derive(Debug, Clone)]
pub struct SemError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Bool,
    Void,
    Struct(String),
}

impl From<&Type> for Ty {
    fn from(t: &Type) -> Self {
        match t {
            Type::Int(_) => Ty::Int,
            Type::Bool(_) => Ty::Bool,
            Type::Void(_) => Ty::Void,
            Type::Named(n, _) => Ty::Struct(n.clone()),
        }
    }
}
impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Bool => write!(f, "bool"),
            Ty::Void => write!(f, "void"),
            Ty::Struct(n) => write!(f, "{}", n),
        }
    }
}

#[derive(Clone, Debug)]
struct FuncSig {
    ret: Ty,
    params: Vec<Ty>,
    span: Span,
}

#[derive(Clone, Debug)]
struct StructInfo {
    name: String,
    fields: Vec<(String, Ty)>, // ordered
    field_map: HashMap<String, (usize, Ty)>,
    span: Span,
}

pub struct Checker {
    funcs: HashMap<String, FuncSig>,
    structs: HashMap<String, StructInfo>,
    scopes: Vec<HashMap<String, Ty>>,
    errors: Vec<SemError>,
    cur_ret: Option<Ty>,
}

impl Checker {
    pub fn new() -> Self {
        Self { funcs: HashMap::new(), structs: HashMap::new(), scopes: Vec::new(), errors: Vec::new(), cur_ret: None }
    }

    fn push_scope(&mut self) { self.scopes.push(HashMap::new()); }
    fn pop_scope(&mut self) { self.scopes.pop(); }

    fn declare_var(&mut self, name: &str, ty: Ty, span: Span) -> bool {
        if let Some(scope) = self.scopes.last_mut() {
            if scope.contains_key(name) {
                self.errors.push(SemError{message: format!("redefinition of `{name}`"), span});
                return false;
            }
            scope.insert(name.to_string(), ty);
            true
        } else { false }
    }
    fn lookup_var(&self, name: &str) -> Option<Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) { return Some(ty.clone()); }
        }
        None
    }

    fn resolve_type(&mut self, ty: &Type) -> Ty {
        let t = Ty::from(ty);
        if let Ty::Struct(ref n) = t {
            if !self.structs.contains_key(n) {
                self.errors.push(SemError{message: format!("unknown type `{n}`"), span: ty.span()});
            }
        }
        // void check for variable usage handled elsewhere
        t
    }

    pub fn check_program(&mut self, prog: &Program) -> Vec<SemError> {
        // First pass: collect struct definitions
        for item in &prog.items {
            if let Item::Struct(s) = item {
                if self.structs.contains_key(&s.name) {
                    self.errors.push(SemError{message: format!("duplicate struct `{}`", s.name), span: s.name_span});
                } else if self.funcs.contains_key(&s.name) {
                    self.errors.push(SemError{message: format!("struct name `{}` conflicts with function", s.name), span: s.name_span});
                } else {
                    let mut seen = HashSet::new();
                    let mut fields = Vec::new();
                    let mut fmap = HashMap::new();
                    for (idx, f) in s.fields.iter().enumerate() {
                        if !seen.insert(&f.name) {
                            self.errors.push(SemError{message: format!("duplicate field `{}` in struct `{}`", f.name, s.name), span: f.name_span});
                        }
                        let fty = self.resolve_type(&f.ty);
                        if fty == Ty::Void {
                            self.errors.push(SemError{message: format!("field `{}` cannot be `void`", f.name), span: f.span});
                        }
                        fmap.insert(f.name.clone(), (idx, fty.clone()));
                        fields.push((f.name.clone(), fty));
                    }
                    self.structs.insert(s.name.clone(), StructInfo{name: s.name.clone(), fields, field_map: fmap, span: s.span});
                }
            }
        }
        // Second pass: collect function signatures
        for item in &prog.items {
            if let Item::Function(f) = item {
                if self.funcs.contains_key(&f.name) {
                    self.errors.push(SemError{message: format!("duplicate function `{}`", f.name), span: f.name_span});
                } else if self.structs.contains_key(&f.name) {
                    self.errors.push(SemError{message: format!("function name `{}` conflicts with struct", f.name), span: f.name_span});
                } else {
                    let param_tys: Vec<Ty> = f.params.iter().map(|p| {
                        let t = self.resolve_type(&p.ty);
                        if t == Ty::Void { self.errors.push(SemError{message: format!("parameter `{}` cannot be `void`", p.name), span: p.span}); }
                        t
                    }).collect();
                    let ret_ty = self.resolve_type(&f.ret_ty);
                    let mut seen = HashSet::new();
                    for p in &f.params {
                        if !seen.insert(&p.name) {
                            self.errors.push(SemError{message: format!("duplicate parameter `{}`", p.name), span: p.name_span});
                        }
                    }
                    self.funcs.insert(f.name.clone(), FuncSig{ret: ret_ty, params: param_tys, span: f.name_span});
                }
            }
        }
        // Validate main
        if let Some(main) = self.funcs.get("main").cloned() {
            if !( (main.ret == Ty::Void && main.params.is_empty()) || (main.ret == Ty::Int && main.params.is_empty()) ) {
                self.errors.push(SemError{message: format!("invalid `main` signature: expected `void main()` or `int main()`, found `{} main({})`", main.ret, main.params.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")), span: main.span});
            }
        } else {
            self.errors.push(SemError{message: "missing `main` function".into(), span: prog.span});
        }

        // Third pass: check function bodies
        for item in &prog.items {
            if let Item::Function(f) = item {
                self.check_function(f);
            }
        }
        std::mem::take(&mut self.errors)
    }

    fn check_function(&mut self, f: &Function) {
        let ret_ty = self.resolve_type(&f.ret_ty);
        self.cur_ret = Some(ret_ty.clone());
        self.push_scope();
        for p in &f.params {
            let ty = self.resolve_type(&p.ty);
            self.declare_var(&p.name, ty, p.name_span);
        }
        let always_returns = self.check_block(&f.body, &ret_ty);
        if ret_ty != Ty::Void && !always_returns {
            self.errors.push(SemError{message: format!("function `{}` missing return on some path (returns `{ret_ty}`)", f.name), span: f.span});
        }
        self.pop_scope();
        self.cur_ret = None;
    }

    fn check_block(&mut self, block: &Block, ret_ty: &Ty) -> bool {
        self.push_scope();
        let mut always_returns = false;
        for stmt in &block.stmts {
            let stmt_returns = self.check_stmt(stmt, ret_ty);
            if stmt_returns { always_returns = true; }
        }
        self.pop_scope();
        always_returns
    }

    fn check_stmt(&mut self, stmt: &Stmt, ret_ty: &Ty) -> bool {
        match stmt {
            Stmt::VarDecl(d) => {
                let decl_ty = self.resolve_type(&d.ty);
                if decl_ty == Ty::Void {
                    self.errors.push(SemError{message: "variable cannot have `void` type".into(), span: d.span});
                }
                if let Some(init) = &d.init {
                    let init_ty = self.check_expr(init);
                    if init_ty != decl_ty && decl_ty != Ty::Void {
                        self.errors.push(SemError{message: format!("type mismatch in initializer: expected `{decl_ty}`, found `{init_ty}`"), span: init.span});
                    }
                }
                self.declare_var(&d.name, decl_ty, d.name_span);
                false
            }
            Stmt::Expr(e) => { let _ = self.check_expr(&e.expr); false }
            Stmt::Block(b) => self.check_block(b, ret_ty),
            Stmt::Return(r) => {
                let cur = self.cur_ret.clone().unwrap();
                match (&r.value, &cur) {
                    (None, Ty::Void) => {},
                    (Some(_), Ty::Void) => self.errors.push(SemError{message: "return with value in `void` function".into(), span: r.span}),
                    (None, ty) => self.errors.push(SemError{message: format!("missing return value: expected `{ty}`"), span: r.span}),
                    (Some(expr), ty) => {
                        let got = self.check_expr(expr);
                        if &got != ty {
                            self.errors.push(SemError{message: format!("return type mismatch: expected `{ty}`, found `{got}`"), span: expr.span});
                        }
                    }
                }
                true
            }
            Stmt::If(s) => {
                let cond_ty = self.check_expr(&s.cond);
                if cond_ty != Ty::Bool {
                    self.errors.push(SemError{message: format!("`if` condition must be `bool`, found `{cond_ty}`"), span: s.cond.span});
                }
                let then_ret = self.check_block(&s.then_block, ret_ty);
                let else_ret = if let Some(else_b) = &s.else_block { self.check_block(else_b, ret_ty) } else { false };
                then_ret && else_ret
            }
            Stmt::While(s) => {
                let cond_ty = self.check_expr(&s.cond);
                if cond_ty != Ty::Bool {
                    self.errors.push(SemError{message: format!("`while` condition must be `bool`, found `{cond_ty}`"), span: s.cond.span});
                }
                let _ = self.check_block(&s.body, ret_ty);
                false
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::IntLit(_) => Ty::Int,
            ExprKind::BoolLit(_) => Ty::Bool,
            ExprKind::Ident(name) => {
                if let Some(ty) = self.lookup_var(name) { ty } else {
                    self.errors.push(SemError{message: format!("undefined variable `{name}`"), span: expr.span});
                    Ty::Int
                }
            }
            ExprKind::Paren(inner) => self.check_expr(inner),
            ExprKind::Unary{op, expr: inner} => {
                let t = self.check_expr(inner);
                match op {
                    UnaryOp::Not => { if t != Ty::Bool { self.errors.push(SemError{message: format!("`not` requires `bool`, found `{t}`"), span: expr.span}); } Ty::Bool }
                    UnaryOp::Neg | UnaryOp::Pos => { if t != Ty::Int { self.errors.push(SemError{message: format!("unary `{op:?}` requires `int`, found `{t}`"), span: expr.span}); } Ty::Int }
                }
            }
            ExprKind::Binary{op, lhs, rhs} => {
                let lt = self.check_expr(lhs);
                let rt = self.check_expr(rhs);
                match op {
                    BinOp::Add|BinOp::Sub|BinOp::Mul|BinOp::Div|BinOp::Mod => {
                        if lt != Ty::Int || rt != Ty::Int {
                            self.errors.push(SemError{message: format!("arithmetic `{op:?}` requires `int`, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Int
                    }
                    BinOp::Lt|BinOp::Le|BinOp::Gt|BinOp::Ge => {
                        if lt != Ty::Int || rt != Ty::Int {
                            self.errors.push(SemError{message: format!("comparison `{op:?}` requires `int`, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Bool
                    }
                    BinOp::Is|BinOp::IsNot => {
                        if lt != rt {
                            self.errors.push(SemError{message: format!("`is` requires matching types, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Bool
                    }
                    BinOp::And|BinOp::Or => {
                        if lt != Ty::Bool || rt != Ty::Bool {
                            self.errors.push(SemError{message: format!("logical `{op:?}` requires `bool`, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Bool
                    }
                }
            }
            ExprKind::Assign{lhs, value} => {
                let lhs_ty = self.check_lvalue(lhs);
                let rhs_ty = self.check_expr(value);
                if lhs_ty != rhs_ty {
                    self.errors.push(SemError{message: format!("assignment type mismatch: expected `{lhs_ty}`, found `{rhs_ty}`"), span: expr.span});
                }
                lhs_ty
            }
            ExprKind::Call{callee, callee_span, args} => {
                let sig = self.funcs.get(callee).cloned();
                if let Some(sig) = sig {
                    if sig.params.len() != args.len() {
                        self.errors.push(SemError{message: format!("`{callee}` expects {} args, found {}", sig.params.len(), args.len()), span: *callee_span});
                    }
                    for (i, arg) in args.iter().enumerate() {
                        let aty = self.check_expr(arg);
                        if let Some(param_ty) = sig.params.get(i) {
                            if &aty != param_ty {
                                self.errors.push(SemError{message: format!("argument {} of `{callee}`: expected `{}`, found `{aty}`", i+1, param_ty), span: arg.span});
                            }
                        }
                    }
                    sig.ret
                } else {
                    self.errors.push(SemError{message: format!("undefined function `{callee}`"), span: *callee_span});
                    for arg in args { let _ = self.check_expr(arg); }
                    Ty::Int
                }
            }
            ExprKind::MemberAccess{object, field, field_span} => {
                let obj_ty = self.check_expr(object);
                if let Ty::Struct(ref sname) = obj_ty {
                    if let Some(sinfo) = self.structs.get(sname) {
                        if let Some((_, fty)) = sinfo.field_map.get(field) {
                            fty.clone()
                        } else {
                            self.errors.push(SemError{message: format!("struct `{sname}` has no field `{field}`"), span: *field_span});
                            Ty::Int
                        }
                    } else {
                        self.errors.push(SemError{message: format!("unknown struct `{sname}`"), span: object.span});
                        Ty::Int
                    }
                } else {
                    self.errors.push(SemError{message: format!("field access on non-struct `{}`, field `{}`", obj_ty, field), span: *field_span});
                    Ty::Int
                }
            }
            ExprKind::StructLit{ty, fields} => {
                let lit_ty = self.resolve_type(ty);
                let sname = match lit_ty {
                    Ty::Struct(ref n) => n.clone(),
                    _ => {
                        self.errors.push(SemError{message: format!("struct literal requires struct type, found `{lit_ty}`"), span: expr.span});
                        return lit_ty;
                    }
                };
                let sinfo = match self.structs.get(&sname).cloned() {
                    Some(s) => s,
                    None => {
                        self.errors.push(SemError{message: format!("unknown struct `{sname}`"), span: expr.span});
                        return Ty::Struct(sname);
                    }
                };
                let mut seen = HashSet::new();
                for (fname, fspan, fexpr) in fields {
                    if !seen.insert(fname) {
                        self.errors.push(SemError{message: format!("duplicate field `{fname}` in struct literal"), span: *fspan});
                    }
                    if let Some((_, expected_ty)) = sinfo.field_map.get(fname) {
                        let got = self.check_expr(fexpr);
                        if &got != expected_ty {
                            self.errors.push(SemError{message: format!("field `{fname}`: expected `{expected_ty}`, found `{got}`"), span: fexpr.span});
                        }
                    } else {
                        self.errors.push(SemError{message: format!("unknown field `{fname}` for struct `{sname}`"), span: *fspan});
                        let _ = self.check_expr(fexpr);
                    }
                }
                // Check missing fields
                for (fname, _) in &sinfo.fields {
                    if !seen.contains(fname) {
                        self.errors.push(SemError{message: format!("missing field `{fname}` in `{sname}` literal"), span: expr.span});
                    }
                }
                Ty::Struct(sname)
            }
        }
    }

    fn check_lvalue(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::Ident(name) => {
                if let Some(ty) = self.lookup_var(name) { ty } else {
                    self.errors.push(SemError{message: format!("undefined variable `{name}`"), span: expr.span});
                    Ty::Int
                }
            }
            ExprKind::MemberAccess{object, field, field_span} => {
                // reuse field check but treat as lvalue
                let obj_ty = self.check_expr(object);
                if let Ty::Struct(ref sname) = obj_ty {
                    if let Some(sinfo) = self.structs.get(sname) {
                        if let Some((_, fty)) = sinfo.field_map.get(field) {
                            fty.clone()
                        } else {
                            self.errors.push(SemError{message: format!("struct `{sname}` has no field `{field}`"), span: *field_span});
                            Ty::Int
                        }
                    } else {
                        self.errors.push(SemError{message: format!("unknown struct `{sname}`"), span: expr.span});
                        Ty::Int
                    }
                } else {
                    self.errors.push(SemError{message: format!("assignment to non-struct field `{field}`"), span: *field_span});
                    Ty::Int
                }
            }
            _ => {
                self.errors.push(SemError{message: "invalid assignment target".into(), span: expr.span});
                Ty::Int
            }
        }
    }
}

pub fn check(prog: &Program) -> Vec<SemError> {
    let mut c = Checker::new();
    c.check_program(prog)
}
