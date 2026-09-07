//! Phase 1 semantic checks: scopes, name resolution, type checking,
//! non-void return guarantee. Mirrors `references/phases.md:23`.

use std::collections::HashMap;

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
}

impl From<&Type> for Ty {
    fn from(t: &Type) -> Self {
        match t {
            Type::Int(_) => Ty::Int,
            Type::Bool(_) => Ty::Bool,
            Type::Void(_) => Ty::Void,
        }
    }
}
impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Bool => write!(f, "bool"),
            Ty::Void => write!(f, "void"),
        }
    }
}

#[derive(Clone, Debug)]
struct FuncSig {
    ret: Ty,
    params: Vec<Ty>,
    span: Span,
}

pub struct Checker {
    funcs: HashMap<String, FuncSig>,
    scopes: Vec<HashMap<String, Ty>>, // stack; 0 is global (unused for locals), function scope pushed
    errors: Vec<SemError>,
    cur_ret: Option<Ty>,
}

impl Checker {
    pub fn new() -> Self {
        Self { funcs: HashMap::new(), scopes: Vec::new(), errors: Vec::new(), cur_ret: None }
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

    pub fn check_program(&mut self, prog: &Program) -> Vec<SemError> {
        // First pass: collect function signatures, detect duplicate definitions, check main
        for item in &prog.items {
            if let Item::Function(f) = item {
                if self.funcs.contains_key(&f.name) {
                    self.errors.push(SemError{message: format!("duplicate function `{}`", f.name), span: f.name_span});
                } else {
                    let param_tys: Vec<Ty> = f.params.iter().map(|p| Ty::from(&p.ty)).collect();
                    // duplicate param names?
                    let mut seen = std::collections::HashSet::new();
                    for p in &f.params {
                        if !seen.insert(&p.name) {
                            self.errors.push(SemError{message: format!("duplicate parameter `{}`", p.name), span: p.name_span});
                        }
                    }
                    self.funcs.insert(f.name.clone(), FuncSig{ret: Ty::from(&f.ret_ty), params: param_tys, span: f.name_span});
                }
            }
        }
        // Validate main exists and signature (§37)
        if let Some(main) = self.funcs.get("main") {
            // allowed: void main() or int main()
            // spec says second is int main(string[] args) but Phase 1 restricts to int main()
            // We'll allow void main() and int main() and int main() with zero params for simplicity
            // If int main has params, forbid for now? Keep simple: allow 0 params only.
            if !( (main.ret == Ty::Void && main.params.is_empty()) || (main.ret == Ty::Int && main.params.is_empty()) ) {
                // also allow int main() as per phase 1
                self.errors.push(SemError{message: format!("invalid `main` signature: expected `void main()` or `int main()`, found `{} main({})`", main.ret, main.params.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")), span: main.span});
            }
        } else {
            self.errors.push(SemError{message: "missing `main` function".into(), span: prog.span});
        }

        // Second pass: check bodies
        for item in &prog.items {
            if let Item::Function(f) = item {
                self.check_function(f);
            }
        }
        // Return errors (drain)
        std::mem::take(&mut self.errors)
    }

    fn check_function(&mut self, f: &Function) {
        let ret_ty = Ty::from(&f.ret_ty);
        self.cur_ret = Some(ret_ty.clone());
        self.push_scope();
        // declare params in scope
        for p in &f.params {
            self.declare_var(&p.name, Ty::from(&p.ty), p.name_span);
        }
        let always_returns = self.check_block(&f.body, &ret_ty);
        if ret_ty != Ty::Void && !always_returns {
            self.errors.push(SemError{message: format!("function `{}` missing return on some path (returns `{ret_ty}`)", f.name), span: f.span});
        }
        self.pop_scope();
        self.cur_ret = None;
    }

    // returns true if block always returns (last stmt returns or if both branches return etc)
    fn check_block(&mut self, block: &Block, ret_ty: &Ty) -> bool {
        self.push_scope();
        let mut always_returns = false;
        for stmt in &block.stmts {
            // unreachable after return? warn but not error
            let stmt_returns = self.check_stmt(stmt, ret_ty);
            if stmt_returns { always_returns = true; }
            // if this stmt always returns, remaining stmts are unreachable — but continue checking for errors
        }
        self.pop_scope();
        always_returns
    }

    fn check_stmt(&mut self, stmt: &Stmt, ret_ty: &Ty) -> bool {
        match stmt {
            Stmt::VarDecl(d) => {
                // type is already validated in parse (only int/bool/void) — but void variables illegal
                let decl_ty: Ty = Ty::from(&d.ty);
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
                let else_ret = if let Some(else_b) = &s.else_block {
                    self.check_block(else_b, ret_ty)
                } else { false };
                then_ret && else_ret
            }
            Stmt::While(s) => {
                let cond_ty = self.check_expr(&s.cond);
                if cond_ty != Ty::Bool {
                    self.errors.push(SemError{message: format!("`while` condition must be `bool`, found `{cond_ty}`"), span: s.cond.span});
                }
                // body checked, but while never guarantees return (even if body returns)
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
                if let Some(ty) = self.lookup_var(name) { ty }
                else {
                    self.errors.push(SemError{message: format!("undefined variable `{name}`"), span: expr.span});
                    Ty::Int // poison to continue
                }
            }
            ExprKind::Paren(inner) => self.check_expr(inner),
            ExprKind::Unary{op, expr: inner} => {
                let t = self.check_expr(inner);
                match op {
                    UnaryOp::Not => {
                        if t != Ty::Bool { self.errors.push(SemError{message: format!("`not` requires `bool`, found `{t}`"), span: expr.span}); }
                        Ty::Bool
                    }
                    UnaryOp::Neg | UnaryOp::Pos => {
                        if t != Ty::Int { self.errors.push(SemError{message: format!("unary `{op:?}` requires `int`, found `{t}`"), span: expr.span}); }
                        Ty::Int
                    }
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
                        // Phase 1: allow int equality via `is`; require same type
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
            ExprKind::Assign{target, target_span, value} => {
                let var_ty = self.lookup_var(target);
                let val_ty = self.check_expr(value);
                if let Some(ty) = var_ty {
                    if ty != val_ty {
                        self.errors.push(SemError{message: format!("assignment type mismatch: `{target}` is `{ty}`, found `{val_ty}`"), span: expr.span});
                    }
                    ty
                } else {
                    self.errors.push(SemError{message: format!("undefined variable `{target}`"), span: *target_span});
                    val_ty
                }
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
        }
    }
}

pub fn check(prog: &Program) -> Vec<SemError> {
    let mut c = Checker::new();
    c.check_program(prog)
}
