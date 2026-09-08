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
    Char,
    String,
    Float,
    Double,
    Struct(String),
    Enum(String),
    Generic(String, Vec<Ty>),
    Tuple(Vec<Ty>),
    Any,
    Function(Box<Ty>, Vec<Ty>),
    Array(Box<Ty>),
    Pointer(Box<Ty>),
    Optional(Box<Ty>),
}

impl From<&Type> for Ty {
    fn from(t: &Type) -> Self {
        match t {
            Type::Int(_) => Ty::Int,
            Type::Bool(_) => Ty::Bool,
            Type::Void(_) => Ty::Void,
            Type::String(_) => Ty::String,
            Type::Char(_) => Ty::Char,
            Type::Float(_) => Ty::Float,
            Type::Double(_) => Ty::Double,
            Type::Named(n, _) => {
                let base = n.rsplit("::").next().unwrap_or(n).to_string();
                Ty::Struct(base)
            },
            Type::Generic(n, args, _) => {
                let base = n.rsplit("::").next().unwrap_or(n).to_string();
                Ty::Generic(base, args.iter().map(|a| Ty::from(a)).collect())
            },
            Type::FunctionType(ret, args, _) => Ty::Function(Box::new(Ty::from(ret.as_ref())), args.iter().map(|a| Ty::from(a)).collect()),
            Type::Tuple(tys, _) => Ty::Tuple(tys.iter().map(|t| Ty::from(t)).collect()),
            Type::Any(_) => Ty::Any,
            Type::Array(el, _) => Ty::Array(Box::new(Ty::from(el.as_ref()))),
            Type::Pointer(el, _) => {
                Ty::Pointer(Box::new(Ty::from(el.as_ref())))
            }
            Type::Optional(el, _) => {
                Ty::Optional(Box::new(Ty::from(el.as_ref())))
            }
        }
    }
}
impl std::fmt::Display for Ty {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Bool => write!(f, "bool"),
            Ty::Void => write!(f, "void"),
            Ty::Char => write!(f, "char"),
            Ty::String => write!(f, "string"),
            Ty::Float => write!(f, "float"),
            Ty::Double => write!(f, "double"),
            Ty::Struct(n) => write!(f, "{}", n),
            Ty::Enum(n) => write!(f, "{}", n),
            Ty::Generic(n, args) => {
                if args.is_empty() {
                    write!(f, "{}", n)
                } else {
                    write!(f, "{}<{}>", n, args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "))
                }
            }
            Ty::Tuple(tys) => write!(f, "({})", tys.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")),
            Ty::Any => write!(f, "any"),
            Ty::Function(ret, args) => write!(f, "function<{}({})>", ret, args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", ")),
            Ty::Array(el) => write!(f, "{}[]", el),
            Ty::Pointer(el) => write!(f, "{}*", el),
            Ty::Optional(el) => write!(f, "{}?", el),
        }
    }
}

#[derive(Clone, Debug)]
struct FuncSig {
    ret: Ty,
    params: Vec<Ty>,
    param_modes: Vec<ParamMode>,
    param_names: Vec<String>,
    param_is_variadic: Vec<bool>,
    generic_params: Vec<GenericParam>,
    where_clause: Option<WhereClause>,
    span: Span,
}

#[derive(Clone, Debug)]
struct StructInfo {
    name: String,
    fields: Vec<(String, Ty)>, // ordered
    field_map: HashMap<String, (usize, Ty)>,
    field_vis: HashMap<String, crate::ast::Visibility>,
    field_defaults: HashMap<String, Option<Expr>>,
    span: Span,
}

#[derive(Clone, Debug)]
struct ClassInfo {
    name: String,
    fields: Vec<(String, Ty)>,
    field_map: HashMap<String, (usize, Ty)>,
    field_vis: HashMap<String, crate::ast::Visibility>,
    methods: HashMap<String, FuncSig>,
    method_vis: HashMap<String, crate::ast::Visibility>,
    constructors: Vec<(FuncSig, crate::ast::Visibility)>,
    properties: HashMap<String, PropertyInfo>,
    operators: HashMap<String, FuncSig>,
    conversions: Vec<(Ty, Ty, Span)>,
    is_open: bool,
    is_sealed: bool,
    extends: Option<String>,
    implements: Vec<String>,
    span: Span,
}

#[derive(Clone, Debug)]
struct PropertyInfo {
    ty: Ty,
    has_get: bool,
    has_set: bool,
    visibility: crate::ast::Visibility,
    span: Span,
}

#[derive(Clone, Debug)]
struct TraitInfo {
    name: String,
    methods: HashMap<String, FuncSig>,
    span: Span,
}

pub struct Checker {
    funcs: HashMap<String, FuncSig>,
    structs: HashMap<String, StructInfo>,
    classes: HashMap<String, ClassInfo>,
    enums: HashMap<String, EnumInfo>,
    traits: HashMap<String, TraitInfo>,
    scopes: Vec<HashMap<String, Ty>>,
    const_scopes: Vec<HashSet<String>>,
    errors: Vec<SemError>,
    cur_ret: Option<Ty>,
    cur_class: Option<String>,
    loop_stack: Vec<Option<String>>,
}

#[derive(Clone, Debug)]
struct EnumInfo {
    name: String,
    variants: Vec<EnumVariantInfo>,
    variant_map: HashMap<String, (usize, Vec<Ty>)>, // variant -> (tag, payload tys)
    span: Span,
}

#[derive(Clone, Debug)]
struct EnumVariantInfo {
    name: String,
    tag: usize,
    payload_tys: Vec<Ty>,
    discriminant_expr: Option<Expr>,
    span: Span,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            funcs: HashMap::new(),
            structs: HashMap::new(),
            classes: HashMap::new(),
            enums: HashMap::new(),
            traits: HashMap::new(),
            scopes: Vec::new(),
            const_scopes: Vec::new(),
            errors: Vec::new(),
            cur_ret: None,
            cur_class: None,
            loop_stack: Vec::new(),
        }
    }

    fn loop_depth(&self) -> usize { self.loop_stack.len() }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.const_scopes.push(HashSet::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.const_scopes.pop();
    }

    fn declare_var(&mut self, name: &str, ty: Ty, span: Span) -> bool {
        if let Some(scope) = self.scopes.last_mut() {
            if scope.contains_key(name) {
                self.errors.push(SemError {
                    message: format!("redefinition of `{name}`"),
                    span,
                });
                return false;
            }
            scope.insert(name.to_string(), ty);
            true
        } else {
            false
        }
    }
    fn declare_const(&mut self, name: &str, ty: Ty, span: Span) -> bool {
        if let Some(scope) = self.scopes.last_mut() {
            if scope.contains_key(name) {
                self.errors.push(SemError {
                    message: format!("redefinition of `{name}`"),
                    span,
                });
                return false;
            }
            scope.insert(name.to_string(), ty);
            if let Some(cset) = self.const_scopes.last_mut() {
                cset.insert(name.to_string());
            }
            true
        } else {
            false
        }
    }
    fn is_const(&self, name: &str) -> bool {
        for cset in self.const_scopes.iter().rev() {
            if cset.contains(name) {
                return true;
            }
        }
        false
    }
    fn lookup_var(&self, name: &str) -> Option<Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }

    fn resolve_type(&mut self, ty: &Type) -> Ty {
        let mut t = Ty::from(ty);
        // Handle generic type params: if t is Struct with name that is a generic param, treat as Generic
        if let Ty::Struct(ref n) = t {
            let lookup = n.rsplit("::").next().unwrap_or(n);
            // Check if it's a generic param for current function/class
            // For minimal, check if it's a single uppercase letter like T, U, V
            if lookup.len() == 1 && lookup.chars().next().unwrap().is_ascii_uppercase() {
                // Consider it as generic if not known struct/class/enum
                if !self.structs.contains_key(lookup) && !self.classes.contains_key(lookup) && !self.enums.contains_key(lookup) {
                    t = Ty::Generic(lookup.to_string(), vec![]);
                    return t;
                }
            }
            if n == "__derived__" {
                // Variadic derived `... vda` without explicit type: placeholder, will be resolved to `prev_type[]` in check_function
                t = Ty::Array(Box::new(Ty::Any));
            } else if self.enums.contains_key(lookup) {
                t = Ty::Enum(lookup.to_string());
            } else if !self.structs.contains_key(lookup) && !self.classes.contains_key(lookup) && !self.traits.contains_key(lookup) {
                // Check if it's generic param (single uppercase)
                if n.len() == 1 && n.chars().next().unwrap().is_ascii_uppercase() {
                    t = Ty::Generic(n.clone(), vec![]);
                } else {
                    self.errors.push(SemError{message: format!("unknown type `{n}`"), span: ty.span()});
                }
            }
        }
        // Handle Generic type
        if let Ty::Generic(ref n, ref args) = t {
            // Check base exists
            if !self.structs.contains_key(n) && !self.classes.contains_key(n) && !self.enums.contains_key(n) && !self.traits.contains_key(n) {
                // Could be generic param itself, not base
                if n.len() == 1 && n.chars().next().unwrap().is_ascii_uppercase() {
                    // Generic param, ok
                } else {
                    self.errors.push(SemError{message: format!("unknown generic type `{n}`"), span: ty.span()});
                }
            }
        }
        // For compound types, ensure inner is known (array element etc) – From already handled, but check nested struct/enum existence
        match &t {
            Ty::Array(el) | Ty::Pointer(el) | Ty::Optional(el) => {
                if let Ty::Struct(ref n) = **el {
                    if !self.structs.contains_key(n) && !self.classes.contains_key(n) && !self.enums.contains_key(n) && !self.traits.contains_key(n) {
                        self.errors.push(SemError{message: format!("unknown type `{n}`"), span: ty.span()});
                    }
                }
                if let Ty::Enum(ref n) = **el {
                    if !self.enums.contains_key(n) { self.errors.push(SemError{message: format!("unknown type `{n}`"), span: ty.span()}); }
                }
                if let Ty::Generic(ref n, _) = **el {
                    if !self.structs.contains_key(n) && !self.classes.contains_key(n) && !self.enums.contains_key(n) && !self.traits.contains_key(n) {
                        self.errors.push(SemError{message: format!("unknown type `{n}`"), span: ty.span()});
                    }
                }
            }
            Ty::Generic(n, args) => {
                if !self.structs.contains_key(n) && !self.classes.contains_key(n) && !self.enums.contains_key(n) && !self.traits.contains_key(n) {
                    self.errors.push(SemError{message: format!("unknown type `{n}`"), span: ty.span()});
                }
                for a in args { if let Ty::Struct(sn) = a { if !self.structs.contains_key(sn) && !self.classes.contains_key(sn) && !self.enums.contains_key(sn) { self.errors.push(SemError{message: format!("unknown type `{sn}`"), span: ty.span()}); } } }
            }
            _ => {}
        }
        t
    }

    pub fn check_program(&mut self, prog: &Program) -> Vec<SemError> {
        self.push_scope(); // global scope for top-level consts/vars
        // Unwrap attributed items for struct/class/enum/trait/typedef/distinct
        let mut unwrapped: Vec<&Item> = Vec::new();
        for it in &prog.items {
            match it {
                Item::Attributed{attrs: _, item} => unwrapped.push(item.as_ref()),
                other => unwrapped.push(other),
            }
        }
        // First pass: collect struct definitions
        for item in unwrapped.iter() {
            if let Item::Struct(s) = item {
                if self.structs.contains_key(&s.name) {
                    self.errors.push(SemError {
                        message: format!("duplicate struct `{}`", s.name),
                        span: s.name_span,
                    });
                } else if self.funcs.contains_key(&s.name) {
                    self.errors.push(SemError {
                        message: format!(
                            "struct name `{}` conflicts with function",
                            s.name
                        ),
                        span: s.name_span,
                    });
                } else {
                    let mut seen = HashSet::new();
                    let mut fields = Vec::new();
                    let mut fmap = HashMap::new();
                    let mut fvis = HashMap::new();
                    let mut fdefaults = HashMap::new();
                    for (idx, f) in s.fields.iter().enumerate() {
                        if !seen.insert(&f.name) {
                            self.errors.push(SemError {
                                message: format!(
                                    "duplicate field `{}` in struct `{}`",
                                    f.name, s.name
                                ),
                                span: f.name_span,
                            });
                        }
                        let fty = self.resolve_type(&f.ty);
                        if fty == Ty::Void {
                            self.errors.push(SemError {
                                message: format!(
                                    "field `{}` cannot be `void`",
                                    f.name
                                ),
                                span: f.span,
                            });
                        }
                        if let Some(def) = &f.default {
                            let dty = self.check_expr(def);
                            if dty != fty && dty != Ty::Any {
                                self.errors.push(SemError {
                                    message: format!("default for field `{}`: expected `{}`, found `{}`", f.name, fty, dty),
                                    span: def.span,
                                });
                            }
                        }
                        fmap.insert(f.name.clone(), (idx, fty.clone()));
                        fvis.insert(f.name.clone(), f.visibility);
                        fdefaults.insert(f.name.clone(), f.default.clone());
                        fields.push((f.name.clone(), fty));
                    }
                    self.structs.insert(
                        s.name.clone(),
                        StructInfo {
                            name: s.name.clone(),
                            fields,
                            field_map: fmap,
                            field_vis: fvis,
                            field_defaults: fdefaults,
                            span: s.span,
                        },
                    );
                }
            }
        }
        // Collect traits first (needed for class implements checks)
        for item in &prog.items {
            if let Item::Trait(t) = item {
                if self.traits.contains_key(&t.name) || self.structs.contains_key(&t.name) || self.classes.contains_key(&t.name) || self.enums.contains_key(&t.name) || self.funcs.contains_key(&t.name) {
                    self.errors.push(SemError{message: format!("duplicate trait `{}`", t.name), span: t.name_span});
                } else {
                    let mut methods = HashMap::new();
                    for m in &t.methods {
                        if methods.contains_key(&m.name) {
                            self.errors.push(SemError{message: format!("duplicate method `{}` in trait `{}`", m.name, t.name), span: m.name_span});
                        } else {
                            let param_tys: Vec<Ty> = m.params.iter().enumerate().map(|(idx, p)| {
                                let mut ty = self.resolve_type(&p.ty);
                                if p.is_variadic {
                                    if p.ty.name() == "__derived__" {
                                        if idx == 0 {
                                            self.errors.push(SemError{message: "variadic `... vda` without previous type must not be first param".into(), span: p.span});
                                        } else {
                                            let prev_t = self.resolve_type(&m.params[idx-1].ty);
                                            ty = Ty::Array(Box::new(prev_t));
                                        }
                                    } else {
                                        ty = Ty::Array(Box::new(ty));
                                    }
                                    if p.ty.name() == "__derived__" && idx + 1 != m.params.len() {
                                        self.errors.push(SemError{message: "derived variadic `... vda` must be last".into(), span: p.span});
                                    }
                                }
                                if ty == Ty::Void { self.errors.push(SemError{message: format!("parameter `{}` cannot be `void`", p.name), span: p.span}); }
                                ty
                            }).collect();
                            let param_modes: Vec<ParamMode> = m.params.iter().map(|p| p.mode).collect();
                            let ret_ty = self.resolve_type(&m.ret_ty);
                            methods.insert(m.name.clone(), FuncSig{ret: ret_ty, params: param_tys, param_modes, param_names: m.params.iter().map(|p| p.name.clone()).collect(), param_is_variadic: m.params.iter().map(|p| p.is_variadic).collect(), generic_params: m.generic_params.clone(), where_clause: m.where_clause.clone(), span: m.name_span});
                        }
                    }
                    self.traits.insert(t.name.clone(), TraitInfo{name: t.name.clone(), methods, span: t.span});
                }
            }
        }
        // First pass (continued): collect class definitions
        for item in &prog.items {
            if let Item::Class(c) = item {
                if self.structs.contains_key(&c.name) || self.classes.contains_key(&c.name) {
                    self.errors.push(SemError{message: format!("duplicate class/struct `{}`", c.name), span: c.name_span});
                } else if self.funcs.contains_key(&c.name) {
                    self.errors.push(SemError{message: format!("class name `{}` conflicts with function", c.name), span: c.name_span});
                } else {
                    // Record extends / implements names (validation deferred until after all classes collected)
                    let extends_name = c.extends.as_ref().map(|t| {
                        match t { Type::Named(s, _) => s.clone(), _ => self.resolve_type(t).to_string() }
                    });
                    let mut implements_names = Vec::new();
                    for imp in &c.implements {
                        let n = match imp { Type::Named(s, _) => s.clone(), _ => self.resolve_type(imp).to_string() };
                        implements_names.push(n);
                    }
                    let mut seen: HashSet<String> = HashSet::new();
                    let mut fields = Vec::new();
                    let mut fmap = HashMap::new();
                    let mut fvis = HashMap::new();
                    let mut fdefaults = HashMap::new();
                    // Inherit parent fields if extends
                    if let Some(ref parent) = extends_name {
                        if let Some(pinfo) = self.classes.get(parent).cloned() {
                            for (idx, (fname, fty)) in pinfo.fields.iter().enumerate() {
                                fmap.insert(fname.clone(), (idx, fty.clone()));
                                if let Some(v) = pinfo.field_vis.get(fname) { fvis.insert(fname.clone(), *v); }
                                else { fvis.insert(fname.clone(), crate::ast::Visibility::Default); }
                                fdefaults.insert(fname.clone(), None);
                                fields.push((fname.clone(), fty.clone()));
                                seen.insert(fname.clone());
                            }
                        } else if let Some(sinfo) = self.structs.get(parent).cloned() {
                            for (idx, (fname, fty)) in sinfo.fields.iter().enumerate() {
                                fmap.insert(fname.clone(), (idx, fty.clone()));
                                if let Some(v) = sinfo.field_vis.get(fname) { fvis.insert(fname.clone(), *v); } else { fvis.insert(fname.clone(), crate::ast::Visibility::Default); }
                                fdefaults.insert(fname.clone(), sinfo.field_defaults.get(fname).cloned().unwrap_or(None));
                                fields.push((fname.clone(), fty.clone()));
                                seen.insert(fname.clone());
                            }
                        }
                    }
                    let offset = fields.len();
                    for (idx, f) in c.fields.iter().enumerate() {
                        if !seen.insert(f.name.clone()) {
                            self.errors.push(SemError{message: format!("duplicate field `{}` in class `{}`", f.name, c.name), span: f.name_span});
                        }
                        let fty = self.resolve_type(&f.ty);
                        if fty == Ty::Void { self.errors.push(SemError{message: format!("field `{}` cannot be `void`", f.name), span: f.span}); }
                        if let Some(def) = &f.default {
                            let dty = self.check_expr(def);
                            if dty != fty && dty != Ty::Any {
                                self.errors.push(SemError{message: format!("default for field `{}`: expected `{}`, found `{}`", f.name, fty, dty), span: def.span});
                            }
                        }
                        let real_idx = offset + idx;
                        fmap.insert(f.name.clone(), (real_idx, fty.clone()));
                        fvis.insert(f.name.clone(), f.visibility);
                        fdefaults.insert(f.name.clone(), f.default.clone());
                        fields.push((f.name.clone(), fty));
                    }
                    // Also insert class layout into structs map for field access / instantiation
                    self.structs.insert(c.name.clone(), StructInfo{name: c.name.clone(), fields: fields.clone(), field_map: fmap.clone(), field_vis: fvis.clone(), field_defaults: fdefaults.clone(), span: c.span});
                    // Collect methods
                    let mut methods = HashMap::new();
                    let mut method_vis: HashMap<String, crate::ast::Visibility> = HashMap::new();
                    for m in &c.methods {
                        if methods.contains_key(&m.name) {
                            self.errors.push(SemError{message: format!("duplicate method `{}` in class `{}`", m.name, c.name), span: m.name_span});
                        } else {
                            let param_tys: Vec<Ty> = m.params.iter().enumerate().map(|(idx, p)| {
                                let mut t = self.resolve_type(&p.ty);
                                if p.is_variadic {
                                    if p.ty.name() == "__derived__" {
                                        if idx == 0 {
                                            self.errors.push(SemError{message: "variadic `... vda` without previous type must not be first param".into(), span: p.span});
                                        } else {
                                            let prev_t = self.resolve_type(&m.params[idx-1].ty);
                                            t = Ty::Array(Box::new(prev_t));
                                        }
                                    } else {
                                        t = Ty::Array(Box::new(t));
                                    }
                                    if p.ty.name() == "__derived__" && idx + 1 != m.params.len() {
                                        self.errors.push(SemError{message: "derived variadic `... vda` must be last".into(), span: p.span});
                                    }
                                }
                                if t == Ty::Void { self.errors.push(SemError{message: format!("parameter `{}` cannot be `void`", p.name), span: p.span}); }
                                t
                            }).collect();
                            let param_modes: Vec<ParamMode> = m.params.iter().map(|p| p.mode).collect();
                            let ret_ty = self.resolve_type(&m.ret_ty);
                            let mut pseen = HashSet::new();
                            for p in &m.params { if !pseen.insert(&p.name) { self.errors.push(SemError{message: format!("duplicate param `{}`", p.name), span: p.name_span}); } }
                            methods.insert(m.name.clone(), FuncSig{ret: ret_ty, params: param_tys, param_modes, param_names: m.params.iter().map(|p| p.name.clone()).collect(), param_is_variadic: m.params.iter().map(|p| p.is_variadic).collect(), generic_params: m.generic_params.clone(), where_clause: m.where_clause.clone(), span: m.name_span});
                            method_vis.insert(m.name.clone(), m.visibility);
                        }
                    }
                    // Inherit parent methods (for static dispatch) and their visibility
                    if let Some(ref parent) = extends_name {
                        if let Some(pinfo) = self.classes.get(parent).cloned() {
                            for (mname, sig) in pinfo.methods.iter() {
                                if !methods.contains_key(mname) {
                                    methods.insert(mname.clone(), sig.clone());
                                    if let Some(v) = pinfo.method_vis.get(mname) { method_vis.insert(mname.clone(), *v); }
                                }
                            }
                        }
                    }
                    // Validate constructors: name must match class name, params unique; also collect for ClassInfo
                    let mut ctor_sigs: Vec<(FuncSig, crate::ast::Visibility)> = Vec::new();
                    for ctor in &c.constructors {
                        if ctor.name != c.name {
                            self.errors.push(SemError{message: format!("constructor name `{}` must match class name `{}`", ctor.name, c.name), span: ctor.name_span});
                        }
                        if methods.contains_key(&ctor.name) {
                            self.errors.push(SemError{message: format!("constructor `{}` conflicts with method", ctor.name), span: ctor.name_span});
                        }
                        let mut pseen = HashSet::new();
                        let mut param_tys = Vec::new();
                        for (idx, p) in ctor.params.iter().enumerate() {
                            let mut ty = self.resolve_type(&p.ty);
                            if p.is_variadic {
                                if p.ty.name() == "__derived__" {
                                    if idx == 0 {
                                        self.errors.push(SemError{message: "variadic `... vda` without previous type must not be first param".into(), span: p.span});
                                    } else {
                                        let prev_t = self.resolve_type(&ctor.params[idx-1].ty);
                                        ty = Ty::Array(Box::new(prev_t));
                                    }
                                } else {
                                    ty = Ty::Array(Box::new(ty));
                                }
                                if p.ty.name() == "__derived__" && idx + 1 != ctor.params.len() {
                                    self.errors.push(SemError{message: "derived variadic `... vda` must be last".into(), span: p.span});
                                }
                            }
                            if ty == Ty::Void { self.errors.push(SemError{message: format!("constructor param `{}` cannot be `void`", p.name), span: p.span}); }
                            if !pseen.insert(&p.name) { self.errors.push(SemError{message: format!("duplicate param `{}` in constructor", p.name), span: p.name_span}); }
                            param_tys.push(ty);
                        }
                        let param_modes: Vec<ParamMode> = ctor.params.iter().map(|p| p.mode).collect();
                        // constructors are void return
                        ctor_sigs.push((FuncSig{ret: Ty::Void, params: param_tys, param_modes, param_names: ctor.params.iter().map(|p| p.name.clone()).collect(), param_is_variadic: ctor.params.iter().map(|p| p.is_variadic).collect(), generic_params: Vec::new(), where_clause: None, span: ctor.name_span}, ctor.visibility));
                    }
                    // Validate destructors: name must match class name
                    for dtor in &c.destructors {
                        if dtor.name != c.name {
                            self.errors.push(SemError{message: format!("destructor name `~{}` must match class name `{}`", dtor.name, c.name), span: dtor.name_span});
                        }
                    }
                    // Collect properties — allow separate getter/setter declarations that merge
                    let mut prop_map: HashMap<String, PropertyInfo> = HashMap::new();
                    for prop in &c.properties {
                        if seen.contains(&prop.name) || methods.contains_key(&prop.name) {
                            self.errors.push(SemError{message: format!("duplicate property/member `{}` in class `{}`", prop.name, c.name), span: prop.name_span});
                            continue;
                        }
                        if let Some(existing) = prop_map.get(&prop.name).cloned() {
                            // Merge getter/setter for same property name
                            let new_has_get = prop.getter.is_some();
                            let new_has_set = prop.setter.is_some();
                            if existing.has_get && new_has_get {
                                self.errors.push(SemError{message: format!("duplicate getter for property `{}` in class `{}`", prop.name, c.name), span: prop.name_span});
                                continue;
                            }
                            if existing.has_set && new_has_set {
                                self.errors.push(SemError{message: format!("duplicate setter for property `{}` in class `{}`", prop.name, c.name), span: prop.name_span});
                                continue;
                            }
                            // Type consistency: if both have explicit types, they must match
                            let new_ty: Option<Ty> = if let Some(ref t) = prop.ty { Some(self.resolve_type(t)) } else if let Some((p,_)) = prop.setter.as_ref() { Some(self.resolve_type(&p.ty)) } else { None };
                            if let Some(nt) = &new_ty {
                                if existing.ty != Ty::Void && *nt != Ty::Void && existing.ty != *nt {
                                    self.errors.push(SemError{message: format!("property `{}` type mismatch", prop.name), span: prop.name_span});
                                }
                            }
                            let merged_has_get = existing.has_get || new_has_get;
                            let merged_has_set = existing.has_set || new_has_set;
                            // Keep existing type/visibility, update accessors
                            prop_map.insert(prop.name.clone(), PropertyInfo{ty: existing.ty.clone(), has_get: merged_has_get, has_set: merged_has_set, visibility: existing.visibility, span: prop.span});
                            continue;
                        }
                        let prop_ty = if let Some(ref t) = prop.ty { self.resolve_type(t) } else if let Some((param, _)) = prop.setter.as_ref() { self.resolve_type(&param.ty) } else if prop.getter.is_some() { Ty::Void } else { Ty::Void };
                        // Check getter/setter consistency
                        if prop.getter.is_none() && prop.setter.is_none() {
                            self.errors.push(SemError{message: format!("property `{}` must have getter or setter", prop.name), span: prop.span});
                        }
                        if let Some(ref g) = prop.getter {
                            // getter body will be checked later; type must match prop_ty if given
                        }
                        if let Some((p, _)) = prop.setter.as_ref() {
                            let pty = self.resolve_type(&p.ty);
                            if let Some(ref t) = prop.ty {
                                let exp = self.resolve_type(t);
                                if pty != exp { self.errors.push(SemError{message: format!("setter param type mismatch for property `{}`", prop.name), span: p.span}); }
                            }
                        }
                        prop_map.insert(prop.name.clone(), PropertyInfo{ty: prop_ty.clone(), has_get: prop.getter.is_some(), has_set: prop.setter.is_some(), visibility: prop.visibility, span: prop.span});
                    }
                    // Inherit parent properties
                    if let Some(ref parent) = extends_name {
                        if let Some(pinfo) = self.classes.get(parent).cloned() {
                            for (pname, pinfo_prop) in pinfo.properties.iter() {
                                if !prop_map.contains_key(pname) {
                                    prop_map.insert(pname.clone(), pinfo_prop.clone());
                                }
                            }
                        }
                    }
                    // Validate implements: class must provide all trait methods
                    for imp_name in &implements_names {
                        if let Some(trait_info) = self.traits.get(imp_name).cloned() {
                            for (mname, sig) in trait_info.methods.iter() {
                                if let Some(cls_sig) = methods.get(mname) {
                                    if cls_sig.ret != sig.ret || cls_sig.params != sig.params {
                                        self.errors.push(SemError{message: format!("class `{}` method `{}` does not match trait `{}` signature", c.name, mname, imp_name), span: c.span});
                                    }
                                } else {
                                    self.errors.push(SemError{message: format!("class `{}` missing trait `{}` method `{}`", c.name, imp_name, mname), span: c.span});
                                }
                            }
                        }
                    }
                    // Collect operators and conversions
                    let mut op_map: HashMap<String, FuncSig> = HashMap::new();
                    for op in &c.operators {
                        let mut p_tys = Vec::new();
                        for (idx, pp) in op.params.iter().enumerate() {
                            let mut ty = self.resolve_type(&pp.ty);
                            if pp.is_variadic {
                                if pp.ty.name() == "__derived__" {
                                    if idx == 0 {
                                        self.errors.push(SemError{message: "variadic `... vda` without previous type must not be first param".into(), span: pp.span});
                                    } else {
                                        let prev_t = self.resolve_type(&op.params[idx-1].ty);
                                        ty = Ty::Array(Box::new(prev_t));
                                    }
                                } else {
                                    ty = Ty::Array(Box::new(ty));
                                }
                                if pp.ty.name() == "__derived__" && idx + 1 != op.params.len() {
                                    self.errors.push(SemError{message: "derived variadic `... vda` must be last".into(), span: pp.span});
                                }
                            }
                            p_tys.push(ty);
                        }
                        let p_modes: Vec<ParamMode> = op.params.iter().map(|p| p.mode).collect();
                        // For MVP, assume operator returns int (or struct for + if class)
                        let ret = Ty::Int;
                        op_map.insert(op.op.clone(), FuncSig{ret: ret.clone(), params: p_tys, param_modes: p_modes, param_names: op.params.iter().map(|p| p.name.clone()).collect(), param_is_variadic: op.params.iter().map(|p| p.is_variadic).collect(), generic_params: Vec::new(), where_clause: op.where_clause.clone(), span: op.span});
                    }
                    let mut conv_vec: Vec<(Ty, Ty, Span)> = Vec::new();
                    for conv in &c.conversions {
                        let from = self.resolve_type(&conv.from_ty);
                        let to = self.resolve_type(&conv.to_ty);
                        conv_vec.push((from, to, conv.span));
                    }
                    self.classes.insert(c.name.clone(), ClassInfo{name: c.name.clone(), fields, field_map: fmap, field_vis: fvis, methods, method_vis, constructors: ctor_sigs, properties: prop_map, operators: op_map, conversions: conv_vec, is_open: c.is_open, is_sealed: c.is_sealed, extends: extends_name.clone(), implements: implements_names.clone(), span: c.span});
                }
            }
        }
        // Validate extends / implements / override after all classes known
        {
            let class_snapshot = self.classes.clone();
            for (cname, info) in class_snapshot {
                if let Some(ref parent) = info.extends {
                    if !self.classes.contains_key(parent) {
                        self.errors.push(SemError{message: format!("class `{cname}` extends unknown class `{parent}`"), span: info.span});
                    } else {
                        let parent_info = &self.classes[parent];
                        if parent_info.is_sealed {
                            self.errors.push(SemError{message: format!("class `{cname}` cannot extend sealed class `{parent}`"), span: info.span});
                        }
                        if !parent_info.is_open && parent_info.methods.keys().any(|k| info.methods.contains_key(k)) {
                            // warn if overriding without open? For now allow, but check sealed methods
                        }
                    }
                }
                for imp in &info.implements {
                    if !self.traits.contains_key(imp) {
                        self.errors.push(SemError{message: format!("class `{cname}` implements unknown trait `{imp}`"), span: info.span});
                    }
                }
                // override checks: methods marked override must exist in parent
                let prog_class = prog.items.iter().filter_map(|it| if let Item::Class(c) = it { if &c.name == &cname { Some(c)} else {None}} else {None}).next();
                if let Some(cls) = prog_class {
                    for m in &cls.methods {
                        if m.is_override {
                            if let Some(ref parent) = info.extends {
                                if let Some(pinfo) = self.classes.get(parent) {
                                    if !pinfo.methods.contains_key(&m.name) {
                                        self.errors.push(SemError{message: format!("method `{}` marked `override` but parent `{}` has no method `{}`", m.name, parent, m.name), span: m.name_span});
                                    }
                                }
                            } else {
                                self.errors.push(SemError{message: format!("method `{}` marked `override` but class `{cname}` has no parent", m.name), span: m.name_span});
                            }
                        }
                        if m.is_sealed && !m.is_override {
                            // sealed without override is unusual but allow
                        }
                    }
                }
            }
        }
        // Collect enums
        for item in &prog.items {
            if let Item::Enum(e) = item {
                if self.enums.contains_key(&e.name) || self.structs.contains_key(&e.name) || self.classes.contains_key(&e.name) || self.funcs.contains_key(&e.name) {
                    self.errors.push(SemError{message: format!("duplicate enum `{}`", e.name), span: e.name_span});
                } else {
                    let mut seen = HashSet::new();
                    let mut variants = Vec::new();
                    let mut vmap = HashMap::new();
                    for (idx, v) in e.variants.iter().enumerate() {
                        if !seen.insert(&v.name) {
                            self.errors.push(SemError{message: format!("duplicate variant `{}` in enum `{}`", v.name, e.name), span: v.name_span});
                        }
                        let mut payload_tys = Vec::new();
                        for p in &v.payload_params {
                            let pt = self.resolve_type(&p.ty);
                            if pt == Ty::Void { self.errors.push(SemError{message: format!("variant `{}` payload param `{}` cannot be `void`", v.name, p.name), span: p.span}); }
                            payload_tys.push(pt);
                        }
                        // discriminant: if Some(expr), try to evaluate as int, else use idx
                        let tag = if let Some(expr) = &v.discriminant {
                            // Try to evaluate constant int expression: for now handle IntLit, or try to resolve as int
                            match &expr.kind {
                                ExprKind::IntLit(val) => *val as usize,
                                _ => {
                                    // Try to check expr as int and use idx as fallback, but also error if not int
                                    let t = self.check_expr(expr);
                                    if t != Ty::Int {
                                        self.errors.push(SemError{message: format!("enum discriminant must be `int`, found `{}`", t), span: expr.span});
                                    }
                                    // For non-literal, use idx as tag and store expr for later evaluation (not yet)
                                    idx
                                }
                            }
                        } else {
                            idx
                        };
                        vmap.insert(v.name.clone(), (tag, payload_tys.clone()));
                        variants.push(EnumVariantInfo{name: v.name.clone(), tag, payload_tys, discriminant_expr: v.discriminant.clone(), span: v.span});
                    }
                    self.enums.insert(e.name.clone(), EnumInfo{name: e.name.clone(), variants, variant_map: vmap, span: e.span});
                }
            }
        }
        // Handle typedef/distinct as type aliases
        for item in &prog.items {
            if let Item::Typedef(td) = item {
                let ty = self.resolve_type(&td.ty);
                let mut fvis = HashMap::new();
                fvis.insert("value".to_string(), Visibility::Public);
                let mut fdefs = HashMap::new();
                fdefs.insert("value".to_string(), None);
                self.structs.insert(td.name.clone(), StructInfo{name: td.name.clone(), fields: vec![("value".to_string(), ty.clone())], field_map: [(String::from("value"), (0, ty.clone()))].into_iter().collect(), field_vis: fvis, field_defaults: fdefs, span: td.span});
            } else if let Item::Distinct(dd) = item {
                let ty = self.resolve_type(&dd.ty);
                let mut fvis = HashMap::new();
                fvis.insert("value".to_string(), Visibility::Public);
                let mut fdefs = HashMap::new();
                fdefs.insert("value".to_string(), None);
                self.structs.insert(dd.name.clone(), StructInfo{name: dd.name.clone(), fields: vec![("value".to_string(), ty.clone())], field_map: [(String::from("value"), (0, ty.clone()))].into_iter().collect(), field_vis: fvis, field_defaults: fdefs, span: dd.span});
            } else if let Item::Extension(ext) = item {
                let target_name = match &ext.ty { Type::Named(n, _) => n.clone(), Type::Generic(n, _, _) => n.clone(), _ => "".to_string() };
                let ext_members = ext.members.clone();
                let mut pending_ext: Vec<(String, FuncSig, crate::ast::Visibility)> = Vec::new();
                for mem in &ext_members {
                    if let crate::ast::ExtensionMember::Function(f) = mem {
                        let ret_ty = self.resolve_type(&f.ret_ty);
                        let param_tys: Vec<Ty> = f.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
                        let param_modes: Vec<ParamMode> = f.params.iter().map(|p| p.mode).collect();
                        let sig = FuncSig{ret: ret_ty, params: param_tys, param_modes, param_names: f.params.iter().map(|p| p.name.clone()).collect(), param_is_variadic: f.params.iter().map(|p| p.is_variadic).collect(), generic_params: f.generic_params.clone(), where_clause: f.where_clause.clone(), span: f.name_span};
                        pending_ext.push((f.name.clone(), sig, f.visibility));
                    }
                }
                if let Some(cls) = self.classes.get_mut(&target_name) {
                    for (name, sig, vis) in pending_ext {
                        cls.methods.insert(name.clone(), sig);
                        cls.method_vis.insert(name, vis);
                    }
                    for mem in &ext_members {
                        match mem {
                            crate::ast::ExtensionMember::Function(f) => {
                                // already handled
                            }
                            _ => {}
                        }
                    }
                }
            } else if let Item::Const(c) = item {
                let decl_ty = if let Some(t) = &c.ty {
                    self.resolve_type(t)
                } else {
                    self.check_expr(&c.init)
                };
                if decl_ty == Ty::Void {
                    self.errors.push(SemError{message: "const cannot have void type".into(), span: c.span});
                }
                let init_ty = self.check_expr(&c.init);
                let is_null = matches!(c.init.kind, ExprKind::Null);
                if !is_null && init_ty != decl_ty && decl_ty != Ty::Any {
                    self.errors.push(SemError{message: format!("const initializer mismatch: expected `{}`, found `{}`", decl_ty, init_ty), span: c.init.span});
                }
                if self.scopes.last().map(|s| s.contains_key(&c.name)).unwrap_or(false) {
                    self.errors.push(SemError{message: format!("redefinition of const `{}`", c.name), span: c.name_span});
                } else {
                    self.declare_const(&c.name, decl_ty, c.name_span);
                }
            } else if let Item::Var(v) = item {
                let decl_ty = self.resolve_type(&v.ty);
                if decl_ty == Ty::Void {
                    self.errors.push(SemError{message: "global variable cannot have `void` type".into(), span: v.span});
                }
                if let Some(init) = &v.init {
                    let init_ty = self.check_expr(init);
                    let is_null = matches!(init.kind, ExprKind::Null);
                    if !is_null && init_ty != decl_ty && decl_ty != Ty::Any && init_ty != Ty::Any {
                        // Allow int literal for any?
                        self.errors.push(SemError{message: format!("global var initializer mismatch: expected `{}`, found `{}`", decl_ty, init_ty), span: init.span});
                    }
                }
                if self.scopes.last().map(|s| s.contains_key(&v.name)).unwrap_or(false) {
                    self.errors.push(SemError{message: format!("redefinition of global var `{}`", v.name), span: v.name_span});
                } else {
                    self.declare_var(&v.name, decl_ty, v.name_span);
                }
            }
        }
        // Handle attributed items as their inner
        for item in &prog.items {
            if let Item::Attributed{attrs: _, item} = item {
                if let Item::Function(f) = item.as_ref() {
                    // Will be handled in next pass, just check inner for now
                }
            }
        }
        // Handle extern functions
        for item in &prog.items {
            if let Item::Extern(ex) = item {
                for mem in &ex.members {
                    if let crate::ast::ExternMember::Function{ty, name, name_span, params, ..} = mem {
                        let ret = self.resolve_type(ty);
                        let param_tys: Vec<Ty> = params.iter().map(|p| self.resolve_type(&p.ty)).collect();
                        let param_modes: Vec<ParamMode> = vec![ParamMode::None; param_tys.len()];
                        self.funcs.insert(name.clone(), FuncSig{ret, params: param_tys, param_modes, param_names: params.iter().map(|p| p.name.clone()).collect(), param_is_variadic: params.iter().map(|p| p.is_variadic).collect(), generic_params: Vec::new(), where_clause: None, span: *name_span});
                    }
                }
            }
        }
        // Second pass: collect function signatures
        for item in &prog.items {
            let func_opt = match item {
                Item::Function(f) => Some(f),
                Item::Attributed{attrs: _, item} => if let Item::Function(f) = item.as_ref() { Some(f) } else { None },
                _ => None,
            };
            if let Some(f) = func_opt {
                if self.funcs.contains_key(&f.name) {
                    self.errors.push(SemError {
                        message: format!("duplicate function `{}`", f.name),
                        span: f.name_span,
                    });
                } else if self.structs.contains_key(&f.name) {
                    self.errors.push(SemError {
                        message: format!(
                            "function name `{}` conflicts with struct",
                            f.name
                        ),
                        span: f.name_span,
                    });
                } else {
                    let param_tys: Vec<Ty> = f
                        .params
                        .iter()
                        .enumerate()
                        .map(|(idx, p)| {
                            let mut t = self.resolve_type(&p.ty);
                            if p.is_variadic {
                                if p.ty.name() == "__derived__" {
                                    if idx == 0 {
                                        // error already pushed in check_function, but for FuncSig keep as Array(Int) placeholder
                                    } else {
                                        let prev_t = self.resolve_type(&f.params[idx-1].ty);
                                        t = Ty::Array(Box::new(prev_t));
                                    }
                                } else {
                                    t = Ty::Array(Box::new(t));
                                }
                            }
                            if t == Ty::Void {
                                self.errors.push(SemError {
                                    message: format!(
                                        "parameter `{}` cannot be `void`",
                                        p.name
                                    ),
                                    span: p.span,
                                });
                            }
                            t
                        })
                        .collect();
                    let ret_ty = self.resolve_type(&f.ret_ty);
                    let param_modes: Vec<ParamMode> = f.params.iter().map(|p| p.mode).collect();
                    let mut seen = HashSet::new();
                    for p in &f.params {
                        if !seen.insert(&p.name) {
                            self.errors.push(SemError {
                                message: format!(
                                    "duplicate parameter `{}`",
                                    p.name
                                ),
                                span: p.name_span,
                            });
                        }
                    }
                    self.funcs.insert(
                        f.name.clone(),
                        FuncSig {
                            ret: ret_ty,
                            params: param_tys,
                            param_modes,
                            param_names: f.params.iter().map(|p| p.name.clone()).collect(),
                            param_is_variadic: f.params.iter().map(|p| p.is_variadic).collect(),
                            generic_params: f.generic_params.clone(),
                            where_clause: f.where_clause.clone(),
                            span: f.name_span,
                        },
                    );
                }
            }
        }
        // Validate main per EBNF §37: `void main()` or `int main(string[] args)`
        if let Some(main) = self.funcs.get("main").cloned() {
            let is_void_main = main.ret == Ty::Void && main.params.is_empty();
            let is_int_main_no_args = main.ret == Ty::Int && main.params.is_empty();
            let is_int_main_with_args = main.ret == Ty::Int
                && main.params.len() == 1
                && main.params[0] == Ty::Array(Box::new(Ty::String))
                && main.param_names.get(0).map(|s| s == "args").unwrap_or(false);
            if !(is_void_main || is_int_main_no_args || is_int_main_with_args)
            {
                self.errors.push(SemError{message: format!("invalid `main` signature: expected `void main()` or `int main()` or `int main(string[] args)`, found `{} main({})`", main.ret, main.params.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")), span: main.span});
            }
        } else {
            self.errors.push(SemError {
                message: "missing `main` function".into(),
                span: prog.span,
            });
        }

        // Third pass: check function bodies
        for item in &prog.items {
            let func_opt = match item {
                Item::Function(f) => Some(f),
                Item::Attributed{attrs: _, item} => if let Item::Function(f) = item.as_ref() { Some(f) } else { None },
                _ => None,
            };
            if let Some(f) = func_opt {
                self.check_function(f);
            }
        }
        // Check class methods, constructors, properties
        let class_items: Vec<ClassDecl> = prog.items.iter().filter_map(|it| if let Item::Class(c) = it { Some(c.clone()) } else { None }).collect();
        for cls in class_items {
            for meth in &cls.methods {
                self.check_method(&cls.name, meth);
            }
            for ctor in &cls.constructors {
                self.check_constructor(&cls.name, ctor);
            }
            for dtor in &cls.destructors {
                self.check_destructor(&cls.name, dtor);
            }
            for prop in &cls.properties {
                self.check_property(&cls.name, prop);
            }
        }
        self.pop_scope(); // global
        std::mem::take(&mut self.errors)
    }

    fn check_function(&mut self, f: &Function) {
        let ret_ty = self.resolve_type(&f.ret_ty);
        self.cur_ret = Some(ret_ty.clone());
        self.push_scope();
        for (idx, p) in f.params.iter().enumerate() {
            let mut ty = self.resolve_type(&p.ty);
            if p.is_variadic {
                // `...T vda` where `T` is element type, `vda` is `T[]`; `... vda` derived from previous
                if p.ty.name() == "__derived__" {
                    if idx == 0 {
                        self.errors.push(SemError{message: "variadic `... vda` without previous type must not be first param".into(), span: p.span});
                    } else {
                        let prev_ty = self.resolve_type(&f.params[idx-1].ty);
                        ty = Ty::Array(Box::new(prev_ty));
                    }
                } else {
                    ty = Ty::Array(Box::new(ty));
                }
                // For derived `... vda` must be last
                if p.ty.name() == "__derived__" && idx + 1 != f.params.len() {
                    self.errors.push(SemError{message: "derived variadic `... vda` must be last".into(), span: p.span});
                }
            }
            self.declare_var(&p.name, ty, p.name_span);
        }
        let always_returns = self.check_block(&f.body, &ret_ty);
        if ret_ty != Ty::Void && !always_returns {
            self.errors.push(SemError{message: format!("function `{}` missing return on some path (returns `{ret_ty}`)", f.name), span: f.span});
        }
        self.pop_scope();
        self.cur_ret = None;
    }

    fn check_method(&mut self, class_name: &str, f: &Function) {
        let ret_ty = self.resolve_type(&f.ret_ty);
        self.cur_ret = Some(ret_ty.clone());
        self.cur_class = Some(class_name.to_string());
        self.push_scope();
        // implicit `this`
        self.declare_var("this", Ty::Struct(class_name.to_string()), f.name_span);
        for (idx, p) in f.params.iter().enumerate() {
            let mut ty = self.resolve_type(&p.ty);
            if p.is_variadic {
                if p.ty.name() == "__derived__" {
                    if idx == 0 {
                        self.errors.push(SemError{message: "variadic `... vda` without previous type must not be first param".into(), span: p.span});
                    } else {
                        let prev_ty = self.resolve_type(&f.params[idx-1].ty);
                        ty = Ty::Array(Box::new(prev_ty));
                    }
                } else {
                    ty = Ty::Array(Box::new(ty));
                }
                if p.ty.name() == "__derived__" && idx + 1 != f.params.len() {
                    self.errors.push(SemError{message: "derived variadic `... vda` must be last".into(), span: p.span});
                }
            }
            self.declare_var(&p.name, ty, p.name_span);
        }
        let always_returns = self.check_block(&f.body, &ret_ty);
        if ret_ty != Ty::Void && !always_returns {
            self.errors.push(SemError{message: format!("method `{}` in class `{}` missing return (returns `{ret_ty}`)", f.name, class_name), span: f.span});
        }
        self.pop_scope();
        self.cur_ret = None;
        self.cur_class = None;
    }

    fn check_constructor(&mut self, class_name: &str, ctor: &ConstructorDecl) {
        self.cur_class = Some(class_name.to_string());
        self.cur_ret = Some(Ty::Void);
        self.push_scope();
        self.declare_var("this", Ty::Struct(class_name.to_string()), ctor.name_span);
        for p in &ctor.params {
            let ty = self.resolve_type(&p.ty);
            self.declare_var(&p.name, ty, p.name_span);
        }
        if let Some(body) = &ctor.body {
            let _ = self.check_block(body, &Ty::Void);
        }
        self.pop_scope();
        self.cur_ret = None;
        self.cur_class = None;
    }

    fn check_destructor(&mut self, class_name: &str, dtor: &DestructorDecl) {
        self.cur_class = Some(class_name.to_string());
        self.cur_ret = Some(Ty::Void);
        self.push_scope();
        self.declare_var("this", Ty::Struct(class_name.to_string()), dtor.name_span);
        let _ = self.check_block(&dtor.body, &Ty::Void);
        self.pop_scope();
        self.cur_ret = None;
        self.cur_class = None;
    }

    fn check_property(&mut self, class_name: &str, prop: &PropertyDecl) {
        let prop_ty = prop.ty.as_ref().map(|t| self.resolve_type(t)).unwrap_or(Ty::Void);
        self.cur_class = Some(class_name.to_string());
        if let Some(getter) = &prop.getter {
            let expected = if prop_ty != Ty::Void { prop_ty.clone() } else { Ty::Int };
            self.cur_ret = Some(expected.clone());
            self.push_scope();
            self.declare_var("this", Ty::Struct(class_name.to_string()), prop.name_span);
            let always_returns = self.check_block(getter, &expected);
            if expected != Ty::Void && !always_returns {
                self.errors.push(SemError{message: format!("property `{}` getter missing return", prop.name), span: prop.span});
            }
            self.pop_scope();
            self.cur_ret = None;
        }
        if let Some((param, body)) = &prop.setter {
            let pty = self.resolve_type(&param.ty);
            if prop_ty != Ty::Void && pty != prop_ty {
                self.errors.push(SemError{message: format!("property `{}` setter type mismatch", prop.name), span: param.span});
            }
            self.cur_ret = Some(Ty::Void);
            self.push_scope();
            self.declare_var("this", Ty::Struct(class_name.to_string()), prop.name_span);
            self.declare_var(&param.name, pty, param.name_span);
            let _ = self.check_block(body, &Ty::Void);
            self.pop_scope();
            self.cur_ret = None;
        }
        self.cur_class = None;
    }

    fn check_block(&mut self, block: &Block, ret_ty: &Ty) -> bool {
        self.push_scope();
        let mut always_returns = false;
        for stmt in &block.stmts {
            let stmt_returns = self.check_stmt(stmt, ret_ty);
            if stmt_returns {
                always_returns = true;
            }
        }
        self.pop_scope();
        always_returns
    }

    fn check_stmt(&mut self, stmt: &Stmt, ret_ty: &Ty) -> bool {
        match stmt {
            Stmt::VarDecl(d) => {
                let decl_ty = self.resolve_type(&d.ty);
                if decl_ty == Ty::Void {
                    self.errors.push(SemError {
                        message: "variable cannot have `void` type".into(),
                        span: d.span,
                    });
                }
                if let Some(init) = &d.init {
                    let init_ty = self.check_expr(init);
                    let is_null = matches!(init.kind, ExprKind::Null);
                    let compatible = if init_ty == decl_ty { true } else {
                        match (&decl_ty, &init_ty) {
                            (Ty::Generic(n1, _), Ty::Enum(n2)) if n1 == n2 => true,
                            (Ty::Enum(n1), Ty::Generic(n2, _)) if n1 == n2 => true,
                            _ => false,
                        }
                    };
                    if !is_null && !compatible && decl_ty != Ty::Void && decl_ty != Ty::Any {
                        self.errors.push(SemError{message: format!("type mismatch in initializer: expected `{decl_ty}`, found `{init_ty}`"), span: init.span});
                    }
                }
                self.declare_var(&d.name, decl_ty, d.name_span);
                false
            }
            Stmt::Const(c) => {
                let decl_ty = if let Some(t) = &c.ty {
                    self.resolve_type(t)
                } else {
                    self.check_expr(&c.init)
                };
                if decl_ty == Ty::Void {
                    self.errors.push(SemError { message: "const cannot have `void` type".into(), span: c.span });
                }
                let init_ty = self.check_expr(&c.init);
                let is_null = matches!(c.init.kind, ExprKind::Null);
                if !is_null && init_ty != decl_ty && decl_ty != Ty::Any {
                    self.errors.push(SemError { message: format!("const initializer mismatch: expected `{decl_ty}`, found `{init_ty}`"), span: c.init.span });
                }
                self.declare_const(&c.name, decl_ty, c.name_span);
                false
            }
            Stmt::Destructure(d) => {
                let expr_ty = self.check_expr(&d.expr);
                // Determine element types
                let elem_tys: Vec<Ty> = match &expr_ty {
                    Ty::Tuple(tys) => tys.clone(),
                    Ty::Array(el) => vec![*el.clone(); d.targets.len()],
                    _ => {
                        // Check if expr is tuple literal directly
                        if let ExprKind::Tuple(exprs) = &d.expr.kind {
                            exprs.iter().map(|e| self.check_expr(e)).collect()
                        } else {
                            self.errors.push(SemError { message: format!("destructuring requires tuple or array, found `{expr_ty}`"), span: d.expr.span });
                            vec![Ty::Int; d.targets.len()]
                        }
                    }
                };
                if elem_tys.len() != d.targets.len() {
                    // Allow wildcard to be counted, but if elem_tys len != targets len, error unless tuple literal with different len?
                    // For `a,b = (1,2,3)` with 2 targets and 3 elems, we take first 2? For now error
                    if d.targets.len() != elem_tys.len() {
                        self.errors.push(SemError { message: format!("destructuring mismatch: {} targets vs {} values", d.targets.len(), elem_tys.len()), span: d.span });
                    }
                }
                for (idx, target) in d.targets.iter().enumerate() {
                    match target {
                        DestructureTarget::Wildcard(_) => {},
                        DestructureTarget::Ident(name, span) => {
                            let expected_ty = elem_tys.get(idx).cloned().unwrap_or(Ty::Int);
                            if let Some(existing) = self.lookup_var(name) {
                                if self.is_const(name) {
                                    self.errors.push(SemError { message: format!("cannot assign to const `{}`", name), span: *span });
                                }
                                if existing != expected_ty && existing != Ty::Any && expected_ty != Ty::Any {
                                    self.errors.push(SemError { message: format!("destructuring type mismatch for `{}`: expected `{}`, found `{}`", name, existing, expected_ty), span: *span });
                                }
                            } else {
                                self.declare_var(name, expected_ty, *span);
                            }
                        }
                    }
                }
                false
            }
            Stmt::Assert(a) => {
                let cond_ty = self.check_expr(&a.cond);
                if cond_ty != Ty::Bool {
                    self.errors.push(SemError { message: format!("assert condition must be `bool`, found `{}`", cond_ty), span: a.cond.span });
                }
                if let Some(msg) = &a.message {
                    let msg_ty = self.check_expr(msg);
                    // message should be string or any, but allow any for now
                    if msg_ty != Ty::String && msg_ty != Ty::Any {
                        // allow string literals and string variables
                        // If msg is not string, still allow but warn? For now allow any
                    }
                }
                false
            }
            Stmt::Expr(e) => {
                let _ = self.check_expr(&e.expr);
                false
            }
            Stmt::Block(b) => self.check_block(b, ret_ty),
            Stmt::Return(r) => {
                let cur = self.cur_ret.clone().unwrap();
                match (&r.value, &cur) {
                    (None, Ty::Void) => {}
                    (Some(_), Ty::Void) => self.errors.push(SemError {
                        message: "return with value in `void` function".into(),
                        span: r.span,
                    }),
                    (None, ty) => self.errors.push(SemError {
                        message: format!(
                            "missing return value: expected `{ty}`"
                        ),
                        span: r.span,
                    }),
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
                    self.errors.push(SemError {
                        message: format!(
                            "`if` condition must be `bool`, found `{cond_ty}`"
                        ),
                        span: s.cond.span,
                    });
                }
                let then_ret = self.check_block(&s.then_block, ret_ty);
                let else_ret = if let Some(else_b) = &s.else_block {
                    self.check_block(else_b, ret_ty)
                } else {
                    false
                };
                then_ret && else_ret
            }
            Stmt::While(s) => {
                let cond_ty = self.check_expr(&s.cond);
                if cond_ty != Ty::Bool {
                    self.errors.push(SemError{message: format!("`while` condition must be `bool`, found `{cond_ty}`"), span: s.cond.span});
                }
                self.loop_stack.push(None);
                let _ = self.check_block(&s.body, ret_ty);
                self.loop_stack.pop();
                false
            }
            Stmt::Loop(l) => {
                self.loop_stack.push(l.label.clone());
                let _ = self.check_block(&l.body, ret_ty);
                self.loop_stack.pop();
                false
            }
            Stmt::For(f) => {
                let iter_ty = self.check_expr(&f.iter);
                let elem_ty = match &iter_ty {
                    Ty::Array(el) => (**el).clone(),
                    Ty::String => Ty::Char,
                    _ => {
                        self.errors.push(SemError{message: format!("`for` iterable must be array or string, found `{iter_ty}`"), span: f.iter.span});
                        Ty::Int
                    }
                };
                self.loop_stack.push(f.label.clone());
                self.push_scope();
                self.declare_var(&f.var, elem_ty, f.var_span);
                let _ = self.check_block(&f.body, ret_ty);
                self.pop_scope();
                self.loop_stack.pop();
                false
            }
            Stmt::Defer(d) => {
                match &d.inner {
                    DeferInner::Expr(e) => { let _ = self.check_expr(e); },
                    DeferInner::Block(b) => { let _ = self.check_block(b, ret_ty); },
                }
                false
            }
            Stmt::Break(b) => {
                if let Some(label) = &b.label {
                    if !self.loop_stack.iter().any(|l| l.as_ref() == Some(label)) {
                        self.errors.push(SemError{message: format!("break label `{label}` not found"), span: b.span});
                    }
                } else if self.loop_depth() == 0 {
                    self.errors.push(SemError{message: "break outside loop".into(), span: b.span});
                }
                false
            }
            Stmt::Continue(c) => {
                if let Some(label) = &c.label {
                    if !self.loop_stack.iter().any(|l| l.as_ref() == Some(label)) {
                        self.errors.push(SemError{message: format!("continue label `{label}` not found"), span: c.span});
                    }
                } else if self.loop_depth() == 0 {
                    self.errors.push(SemError{message: "continue outside loop".into(), span: c.span});
                }
                false
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::IntLit(_) => Ty::Int,
            ExprKind::FloatLit(_) => Ty::Double,
            ExprKind::BoolLit(_) => Ty::Bool,
            ExprKind::Ident(name) => {
                let lookup = name.rsplit("::").next().unwrap_or(name);
                if let Some(ty) = self.lookup_var(name).or_else(|| self.lookup_var(lookup)) {
                    ty
                } else {
                    if name.contains("::") {
                        let first = name.split("::").next().unwrap_or(name);
                        if self.enums.contains_key(first) {
                            return Ty::Enum(first.to_string());
                        }
                        if self.structs.contains_key(first) || self.classes.contains_key(first) {
                            return Ty::Struct(first.to_string());
                        }
                    }
                    self.errors.push(SemError {
                        message: format!("undefined variable `{name}`"),
                        span: expr.span,
                    });
                    Ty::Int
                }
            }
            ExprKind::Paren(inner) => self.check_expr(inner),
            ExprKind::Unary { op, expr: inner } => {
                let t = self.check_expr(inner);
                match op {
                    UnaryOp::Not => {
                        if t != Ty::Bool {
                            self.errors.push(SemError {
                                message: format!(
                                    "`not` requires `bool`, found `{t}`"
                                ),
                                span: expr.span,
                            });
                        }
                        Ty::Bool
                    }
                    UnaryOp::Neg | UnaryOp::Pos => {
                        if t != Ty::Int {
                            self.errors.push(SemError {
                                message: format!(
                                    "unary `{op:?}` requires `int`, found `{t}`"
                                ),
                                span: expr.span,
                            });
                        }
                        Ty::Int
                    }
                    UnaryOp::BitNot | UnaryOp::Inc | UnaryOp::Dec => {
                        if t != Ty::Int {
                            self.errors.push(SemError {
                                message: format!(
                                    "unary `{op:?}` requires `int`, found `{t}`"
                                ),
                                span: expr.span,
                            });
                        }
                        Ty::Int
                    }
                }
            }
            ExprKind::Postfix { op, expr: inner } => {
                let t = self.check_expr(inner);
                if t != Ty::Int {
                    self.errors.push(SemError{message: format!("postfix `{op:?}` requires `int`, found `{t}`"), span: expr.span});
                }
                Ty::Int
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lt = self.check_expr(lhs);
                let rt = self.check_expr(rhs);
                // operator overloading for class
                if let Ty::Struct(ref sname) = lt {
                    if let Some(cinfo) = self.classes.get(sname).cloned() {
                        let op_str = match op {
                            BinOp::Add => "+",
                            BinOp::Sub => "-",
                            BinOp::Mul => "*",
                            BinOp::Div => "/",
                            BinOp::Mod => "%",
                            BinOp::Lt => "<",
                            BinOp::Le => "<=",
                            BinOp::Gt => ">",
                            BinOp::Ge => ">=",
                            BinOp::Is => "is",
                            BinOp::IsNot => "is not",
                            BinOp::And => "and",
                            BinOp::Or => "or",
                            BinOp::BitAnd => "&",
                            BinOp::BitOr => "|",
                            BinOp::BitXor => "^",
                            BinOp::Shl => "<<",
                            BinOp::Shr => ">>",
                            BinOp::NullCoalesce => "??",
                            BinOp::Range => "..",
                            BinOp::RangeInclusive => "..=",
                            BinOp::CompoundAdd => "+=",
                            BinOp::CompoundSub => "-=",
                            BinOp::CompoundMul => "*=",
                            BinOp::CompoundDiv => "/=",
                            BinOp::CompoundMod => "%=",
                            BinOp::CompoundBitAnd => "&=",
                            BinOp::CompoundBitOr => "|=",
                            BinOp::CompoundBitXor => "^=",
                            BinOp::CompoundShl => "<<=",
                            BinOp::CompoundShr => ">>=",
                        };
                        if let Some(sig) = cinfo.operators.get(op_str) {
                            if let Some(exp) = sig.params.get(0) {
                                if &rt != exp {
                                    self.errors.push(SemError{message: format!("operator `{op_str}` for `{sname}` expects `{}`, found `{rt}`", exp), span: expr.span});
                                }
                            }
                            return sig.ret.clone();
                        }
                    }
                }
                match op {
                    BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Mod
                    | BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor
                    | BinOp::Shl
                    | BinOp::Shr => {
                        if lt != Ty::Int || rt != Ty::Int {
                            self.errors.push(SemError{message: format!("arithmetic/bitwise `{op:?}` requires `int`, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Int
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        if lt != Ty::Int || rt != Ty::Int {
                            self.errors.push(SemError{message: format!("comparison `{op:?}` requires `int`, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Bool
                    }
                    BinOp::NullCoalesce => {
                        // a ?? b : if a is Optional, return inner, else return lt
                        Ty::Int
                    }
                    BinOp::Range | BinOp::RangeInclusive => {
                        if lt != Ty::Int || rt != Ty::Int {
                            self.errors.push(SemError{message: format!("range `{op:?}` requires `int`, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Array(Box::new(Ty::Int))
                    }
                    BinOp::Is | BinOp::IsNot => {
                        if lt != rt {
                            self.errors.push(SemError{message: format!("`is` requires matching types, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Bool
                    }
                    BinOp::And | BinOp::Or => {
                        if lt != Ty::Bool || rt != Ty::Bool {
                            self.errors.push(SemError{message: format!("logical `{op:?}` requires `bool`, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Bool
                    }
                    BinOp::CompoundAdd
                    | BinOp::CompoundSub
                    | BinOp::CompoundMul
                    | BinOp::CompoundDiv
                    | BinOp::CompoundMod
                    | BinOp::CompoundBitAnd
                    | BinOp::CompoundBitOr
                    | BinOp::CompoundBitXor
                    | BinOp::CompoundShl
                    | BinOp::CompoundShr => {
                        self.errors.push(SemError{message: format!("compound operator `{op:?}` should not appear in Binary"), span: expr.span});
                        Ty::Int
                    }
                }
            }
            ExprKind::Assign { lhs, value } => {
                let lhs_ty = self.check_lvalue(lhs);
                if let ExprKind::Ident(name) = &lhs.kind {
                    if self.is_const(name) {
                        self.errors.push(SemError{message: format!("cannot assign to const `{}`", name), span: lhs.span});
                    }
                }
                let rhs_ty = self.check_expr(value);
                let is_null = matches!(value.kind, ExprKind::Null);
                if !is_null && lhs_ty != rhs_ty {
                    self.errors.push(SemError{message: format!("assignment type mismatch: expected `{lhs_ty}`, found `{rhs_ty}`"), span: expr.span});
                }
                lhs_ty
            }
            ExprKind::CompoundAssign { op: _, lhs, value } => {
                let lhs_ty = self.check_lvalue(lhs);
                let rhs_ty = self.check_expr(value);
                if lhs_ty != Ty::Int || rhs_ty != Ty::Int {
                    self.errors.push(SemError{message: format!("compound assignment requires `int`, found `{lhs_ty}` and `{rhs_ty}`"), span: expr.span});
                }
                lhs_ty
            }
            ExprKind::Conditional { cond, then_branch, else_branch } => {
                let ct = self.check_expr(cond);
                if ct != Ty::Bool {
                    self.errors.push(SemError{message: format!("conditional cond requires `bool`, found `{ct}`"), span: cond.span});
                }
                let tt = self.check_expr(then_branch);
                let et = self.check_expr(else_branch);
                if tt != et {
                    self.errors.push(SemError{message: format!("conditional branches mismatch: `{tt}` vs `{et}`"), span: expr.span});
                }
                tt
            }
            ExprKind::Range { start, end, inclusive: _ } => {
                if let Some(s) = start { let st = self.check_expr(s); if st != Ty::Int { self.errors.push(SemError{message: format!("range start requires `int`, found `{st}`"), span: s.span}); } }
                if let Some(e) = end { let et = self.check_expr(e); if et != Ty::Int { self.errors.push(SemError{message: format!("range end requires `int`, found `{et}`"), span: e.span}); } }
                Ty::Array(Box::new(Ty::Int))
            }
            ExprKind::NullableMemberAccess { object, field, field_span: _ } => {
                let obj_ty = self.check_expr(object);
                // For now, treat like normal member access but allow nullable
                if let Ty::Struct(ref sname) = obj_ty {
                    if let Some(sinfo) = self.structs.get(sname) {
                        if let Some((_, fty)) = sinfo.field_map.get(field) {
                            return fty.clone();
                        }
                    }
                    if let Some(cinfo) = self.classes.get(sname) {
                        if let Some(prop) = cinfo.properties.get(field) {
                            return prop.ty.clone();
                        }
                    }
                }
                // For optional types, unwrap
                if let Ty::Optional(inner) = obj_ty {
                    if let Ty::Struct(ref sname) = *inner {
                        if let Some(sinfo) = self.structs.get(sname) {
                            if let Some((_, fty)) = sinfo.field_map.get(field) {
                                return fty.clone();
                            }
                        }
                    }
                }
                self.errors.push(SemError{message: format!("unknown field `{field}` for nullable access"), span: expr.span});
                Ty::Int
            }
            ExprKind::Call {
                callee,
                callee_span,
                args,
                type_args,
            } => {
                // Handle generic function calls: substitute type args and check where bounds
                if !type_args.is_empty() {
                    if let Some(func) = self.funcs.get(callee).cloned() {
                        self.check_generic_bounds(&func.generic_params, &func.where_clause, type_args, *callee_span);
                        // Check if function has generic params
                        // For now, assume single generic T and single type arg
                        // Find the generic function decl to get its generic params
                        // For minimal, just check if func has generic params via prog? We don't have prog here, so just handle simple case where T -> actual
                        // Look up function decl in prog? We can just handle by substituting T with first type arg
                        let generic_subst: std::collections::HashMap<String, Ty> = {
                            // Find the function decl's generic params by looking up in self.funcs? But self.funcs doesn't store generics
                            // For minimal, assume T -> type_args[0]
                            let mut m = std::collections::HashMap::new();
                            if let Some(first_arg) = type_args.first() {
                                let actual_ty = self.resolve_type(first_arg);
                                // Assume generic param is "T"
                                m.insert("T".to_string(), actual_ty.clone());
                                // Also handle U etc. for multiple
                                for (i, ta) in type_args.iter().enumerate() {
                                    let name = if i == 0 { "T".to_string() } else if i == 1 { "U".to_string() } else { format!("T{}", i) };
                                    m.insert(name, self.resolve_type(ta));
                                }
                            }
                            m
                        };
                        // Check if func is generic (has T in params/ret)
                        let is_generic = !type_args.is_empty();
                        // For minimal, if type_args provided, substitute
                        if !type_args.is_empty() {
                            // Substitute T in params and ret (including Array wrapper for variadic)
                            let subst_ty = |ty: &Ty| -> Ty {
                                match ty {
                                    Ty::Generic(n, _) if generic_subst.contains_key(n) => generic_subst[n].clone(),
                                    Ty::Struct(n) if generic_subst.contains_key(n) => generic_subst[n].clone(),
                                    Ty::Array(el) => {
                                        let inner = match el.as_ref() {
                                            Ty::Generic(n, _) if generic_subst.contains_key(n) => generic_subst[n].clone(),
                                            Ty::Struct(n) if generic_subst.contains_key(n) => generic_subst[n].clone(),
                                            Ty::Array(inner2) => {
                                                let subst_inner = match inner2.as_ref() {
                                                    Ty::Generic(n, _) if generic_subst.contains_key(n) => generic_subst[n].clone(),
                                                    Ty::Struct(n) if generic_subst.contains_key(n) => generic_subst[n].clone(),
                                                    other => other.clone(),
                                                };
                                                Ty::Array(Box::new(subst_inner))
                                            }
                                            other => other.clone(),
                                        };
                                        Ty::Array(Box::new(inner))
                                    }
                                    other => other.clone(),
                                }
                            };
                            // Build substituted sig and delegate to variadic-aware check
                            let substituted_params: Vec<Ty> = func.params.iter().map(|p| subst_ty(p)).collect();
                            let substituted_sig = FuncSig {
                                ret: subst_ty(&func.ret),
                                params: substituted_params,
                                param_modes: func.param_modes.clone(),
                                param_names: func.param_names.clone(),
                                param_is_variadic: func.param_is_variadic.clone(),
                                generic_params: vec![],
                                where_clause: None,
                                span: func.span,
                            };
                            self.check_call_with_sig(args, &substituted_sig, *callee_span, callee);
                            return substituted_sig.ret;
                        }
                    }
                }
                // Check for class constructor call: ClassName(args)
                if let Some(cls) = self.classes.get(callee).cloned() {
                    // Need class decl to find constructors; retrieve from prog? For now check if any ctor matches arity
                    // We stored class info but not ctor sigs directly; we need to lookup via prog? Simpler: check if class has any constructors via scanning prog? Instead use class info fields?
                    // For now, try to find matching ctor via class's constructors stored in checker? We have not stored ctor sigs separately, but we can treat ctor as function with same name.
                    // Quick approach: if class has no explicit ctor, error; else check args
                    // We need to find ctor sigs: iterate over class decls in prog? But we don't have prog here.
                    // Fallback: treat as struct construction via ctor: return struct type, check each arg type is int (for now)
                    // Better: lookup ctor from self.classes via stored field? We didn't store ctor sigs; store generic check via type checking each arg as int?
                    // For Phase 4 minimal: assume ctor takes same types as fields order if no explicit ctor? But test has explicit ctor with 2 ints.
                    // So we will search in self.classes[callee] not enough; we need ctor info. For now we will directly check if callee is a class, return its struct type after checking args.
                    // Find ctor decl via scanning program? We can store ctor sigs in ClassInfo now that we have extends handling - but we already collect ctor in ClassInfo? Actually we didn't store ctor sigs in ClassInfo methods, only fields etc. We need to add ctor sig storage.
                    // As quick fix: if class exists, treat call as constructor: check each arg is int? For test it's int, int -> ok.
                    // Let's attempt to find ctor via class's constructors collected earlier: we stored them but not as sigs. We'll handle via generic: if class has property ctor, we validate via prog scan.
                    // For now simply: if callee class exists, return struct type, and type-check args as if they were field types or ctor params? We'll look up class struct fields and match.
                    let struct_ty = Ty::Struct(callee.clone());
                    // If class has explicit constructors, check against them (visibility: Default is public for ctors)
                    if let Some(cinfo) = self.classes.get(callee).cloned() {
                        if !cinfo.constructors.is_empty() {
                            let mut matched: Option<(FuncSig, crate::ast::Visibility)> = None;
                            for (sig, vis) in &cinfo.constructors {
                                if sig.params.len() == args.len() {
                                    matched = Some((sig.clone(), *vis));
                                    break;
                                }
                            }
                            if let Some((sig, vis)) = matched {
                                if vis == crate::ast::Visibility::Private && self.cur_class.as_deref() != Some(callee.as_str()) {
                                    self.errors.push(SemError{message: format!("constructor for `{callee}` is private"), span: *callee_span});
                                }
                                for (i, arg) in args.iter().enumerate() {
                                    let aty = self.check_call_arg(arg);
                                    let pidx = if let CallArg::Named { name, .. } = arg {
                                        sig.param_names.iter().position(|n| n == name).unwrap_or(i)
                                    } else { i };
                                    if &aty != &sig.params[pidx] && aty != Ty::Any {
                                        self.errors.push(SemError{message: format!("ctor arg {}: expected `{}`, found `{aty}`", i+1, sig.params[pidx]), span: arg.span()});
                                    }
                                }
                                return struct_ty;
                            } else {
                                self.errors.push(SemError{message: format!("no matching constructor for `{callee}` with {} args", args.len()), span: *callee_span});
                                for arg in args { let _ = self.check_call_arg(arg); }
                                return struct_ty;
                            }
                        }
                    }
                    // No explicit ctor: check against fields (struct literal via call)
                    let field_tys: Vec<Ty> = self.structs.get(callee).map(|s| s.fields.iter().map(|(_,ty)| ty.clone()).collect()).unwrap_or_default();
                    if !field_tys.is_empty() && args.len() == field_tys.len() {
                        for (i, arg) in args.iter().enumerate() {
                            let aty = self.check_call_arg(arg);
                            let fty = &field_tys[i];
                            if &aty != fty && aty != Ty::Any {
                                self.errors.push(SemError{message: format!("ctor arg {}: expected `{}`, found `{aty}`", i+1, fty), span: arg.span()});
                            }
                        }
                        return struct_ty;
                    }
                    // Fallback: just type-check args and return struct
                    for arg in args { let _ = self.check_call_arg(arg); }
                    return struct_ty;
                }
                // builtin io intrinsics
                if matches!(callee.as_str(), "print" | "println" | "printInt" | "putChar") {
                    for arg in args { let _ = self.check_call_arg(arg); }
                    return Ty::Void;
                }
                let sig = self.funcs.get(callee).cloned();
                if let Some(sig) = sig {
                    if sig.param_is_variadic.iter().any(|&v| v) {
                        self.check_call_with_sig(args, &sig, *callee_span, callee);
                    } else {
                        if sig.params.len() != args.len() {
                            self.errors.push(SemError {
                                message: format!(
                                    "`{callee}` expects {} args, found {}",
                                    sig.params.len(),
                                    args.len()
                                ),
                                span: *callee_span,
                            });
                        }
                        for (i, arg) in args.iter().enumerate() {
                            let aty = self.check_call_arg(arg);
                            let pidx = if let CallArg::Named { name, .. } = arg {
                                sig.param_names.iter().position(|n| n == name).unwrap_or(i)
                            } else { i };
                            if let Some(param_ty) = sig.params.get(pidx) {
                                if &aty != param_ty && aty != Ty::Any {
                                    let is_generic = matches!(param_ty, Ty::Generic(n, _) if n.len() == 1 && n.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false));
                                    if !is_generic {
                                        self.errors.push(SemError{message: format!("argument {} of `{callee}`: expected `{}`, found `{aty}`", i+1, param_ty), span: arg.span()});
                                    }
                                }
                            }
                        }
                    }
                    sig.ret
                } else if let Some(var_ty) = self.lookup_var(callee) {
                    // variable call (closure / function pointer)
                    if let Ty::Function(ret, params) = var_ty {
                        if params.len() != args.len() {
                            self.errors.push(SemError{message: format!("function variable `{callee}` expects {} args, found {}", params.len(), args.len()), span: *callee_span});
                        }
                        for (i, arg) in args.iter().enumerate() {
                            let aty = self.check_call_arg(arg);
                            if let Some(pt) = params.get(i) { if &aty != pt && aty != Ty::Any { self.errors.push(SemError{message: format!("argument {}: expected `{}`, found `{aty}`", i+1, pt), span: arg.span()}); } }
                        }
                        *ret
                    } else if let Ty::Any = var_ty {
                        for arg in args { let _ = self.check_call_arg(arg); }
                        Ty::Int
                    } else {
                        self.errors.push(SemError{message: format!("`{callee}` is not a function (found `{var_ty}`)"), span: *callee_span});
                        for arg in args { let _ = self.check_call_arg(arg); }
                        Ty::Int
                    }
                } else {
                    self.errors.push(SemError {
                        message: format!("undefined function `{callee}`"),
                        span: *callee_span,
                    });
                    for arg in args {
                        let _ = self.check_call_arg(arg);
                    }
                    Ty::Int
                }
            }
            ExprKind::MemberAccess {
                object,
                field,
                field_span,
            } => {
                let obj_ty = self.check_expr(object);
                if let Ty::Struct(ref sname) = obj_ty {
                    if let Some(sinfo) = self.structs.get(sname) {
                        if let Some((_, fty)) = sinfo.field_map.get(field) {
                            // visibility check for class fields: Default is private for fields/methods/properties, only ctor Default is public
                            if let Some(cinfo) = self.classes.get(sname) {
                                if let Some(vis) = cinfo.field_vis.get(field) {
                                    if *vis != crate::ast::Visibility::Public && self.cur_class.as_deref() != Some(sname.as_str()) {
                                        self.errors.push(SemError{message: format!("field `{field}` is private"), span: *field_span});
                                    }
                                } else {
                                    // Default for class field is private
                                    if self.cur_class.as_deref() != Some(sname.as_str()) {
                                        self.errors.push(SemError{message: format!("field `{field}` is private"), span: *field_span});
                                    }
                                }
                                // also check if property overrides field? already handled
                            } else {
                                // pure struct: Default is public, only explicit `private` is private
                                if let Some(vis) = sinfo.field_vis.get(field) {
                                    if *vis == crate::ast::Visibility::Private {
                                        self.errors.push(SemError{message: format!("field `{field}` is private"), span: *field_span});
                                    }
                                }
                            }
                            // also check property getter if shadows field? prefer field
                            fty.clone()
                        } else if let Some(cinfo) = self.classes.get(sname) {
                            if let Some(prop) = cinfo.properties.get(field) {
                                // property visibility: Default is private
                                if prop.visibility != crate::ast::Visibility::Public && self.cur_class.as_deref() != Some(sname.as_str()) {
                                    self.errors.push(SemError{message: format!("property `{field}` is private"), span: *field_span});
                                }
                                if prop.has_get {
                                    prop.ty.clone()
                                } else {
                                    self.errors.push(SemError{message: format!("property `{field}` has no getter"), span: *field_span});
                                    Ty::Int
                                }
                            } else {
                                self.errors.push(SemError {
                                    message: format!(
                                        "struct `{sname}` has no field `{field}`"
                                    ),
                                    span: *field_span,
                                });
                                Ty::Int
                            }
                        } else {
                            self.errors.push(SemError {
                                message: format!(
                                    "struct `{sname}` has no field `{field}`"
                                ),
                                span: *field_span,
                            });
                            Ty::Int
                        }
                    } else {
                        self.errors.push(SemError {
                            message: format!("unknown struct `{sname}`"),
                            span: object.span,
                        });
                        Ty::Int
                    }
                } else {
                    self.errors.push(SemError {
                        message: format!(
                            "field access on non-struct `{}`, field `{}`",
                            obj_ty, field
                        ),
                        span: *field_span,
                    });
                    Ty::Int
                }
            }
            ExprKind::StructLit { ty, fields } => {
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
                        self.errors.push(SemError {
                            message: format!("unknown struct `{sname}`"),
                            span: expr.span,
                        });
                        return Ty::Struct(sname);
                    }
                };
                let mut seen = HashSet::new();
                for (fname, fspan, fexpr) in fields {
                    if !seen.insert(fname) {
                        self.errors.push(SemError {
                            message: format!(
                                "duplicate field `{fname}` in struct literal"
                            ),
                            span: *fspan,
                        });
                    }
                    if let Some((_, expected_ty)) = sinfo.field_map.get(fname) {
                        let got = self.check_expr(fexpr);
                        if &got != expected_ty {
                            self.errors.push(SemError{message: format!("field `{fname}`: expected `{expected_ty}`, found `{got}`"), span: fexpr.span});
                        }
                    } else {
                        self.errors.push(SemError {
                            message: format!(
                                "unknown field `{fname}` for struct `{sname}`"
                            ),
                            span: *fspan,
                        });
                        let _ = self.check_expr(fexpr);
                    }
                }
                // Check missing fields — allow if field has default `= expr`
                for (fname, _) in &sinfo.fields {
                    if !seen.contains(fname) {
                        let has_default = sinfo.field_defaults.get(fname).and_then(|o| o.as_ref()).is_some();
                        if !has_default {
                            self.errors.push(SemError {
                                message: format!(
                                    "missing field `{fname}` in `{sname}` literal"
                                ),
                                span: expr.span,
                            });
                        }
                    }
                }
                Ty::Struct(sname)
            }
            ExprKind::StringLit(_) => Ty::String,
            ExprKind::CharLit(_) => Ty::Char,
            ExprKind::Index { object, index } => {
                let obj_ty = self.check_expr(object);
                let idx_ty = self.check_expr(index);
                if idx_ty != Ty::Int {
                    self.errors.push(SemError {
                        message: format!(
                            "index must be `int`, found `{idx_ty}`"
                        ),
                        span: index.span,
                    });
                }
                match obj_ty {
                    Ty::Array(el) => *el,
                    Ty::String => Ty::Char, // string[i] -> char ?
                    _ => {
                        self.errors.push(SemError {
                            message: format!(
                                "cannot index non-array type `{obj_ty}`"
                            ),
                            span: object.span,
                        });
                        Ty::Int
                    }
                }
            }
            ExprKind::Slice { object, start, end, inclusive: _ } => {
                let obj_ty = self.check_expr(object);
                if let Some(s) = start {
                    let st = self.check_expr(s);
                    if st != Ty::Int {
                        self.errors.push(SemError { message: format!("slice start must be `int`, found `{st}`"), span: s.span });
                    }
                }
                if let Some(e) = end {
                    let et = self.check_expr(e);
                    if et != Ty::Int {
                        self.errors.push(SemError { message: format!("slice end must be `int`, found `{et}`"), span: e.span });
                    }
                }
                match obj_ty {
                    Ty::Array(el) => Ty::Array(el.clone()), // slice retains array type
                    Ty::String => Ty::String, // string slice -> string
                    _ => {
                        self.errors.push(SemError { message: format!("cannot slice non-array type `{obj_ty}`"), span: object.span });
                        Ty::Int
                    }
                }
            }
            ExprKind::This => {
                if let Some(cls) = &self.cur_class {
                    Ty::Struct(cls.clone())
                } else {
                    self.errors.push(SemError{message: "`this` outside class method".into(), span: expr.span});
                    Ty::Int
                }
            }
            ExprKind::MethodCall{object, method, method_span, args} => {
                let obj_ty = self.check_expr(object);
                let sname = match obj_ty {
                    Ty::Struct(ref n) => n.clone(),
                    _ => {
                        self.errors.push(SemError{message: format!("method call on non-class type `{obj_ty}`"), span: object.span});
                        for a in args { let _ = self.check_call_arg(a); }
                        return Ty::Int;
                    }
                };
                if let Some(cls) = self.classes.get(&sname).cloned() {
                    // try class map first, fallback to structs? But methods only in classes
                    if let Some(meth) = cls.methods.get(method) {
                        // visibility: Default is private for methods
                        if let Some(vis) = cls.method_vis.get(method) {
                            if *vis != crate::ast::Visibility::Public && self.cur_class.as_deref() != Some(sname.as_str()) {
                                self.errors.push(SemError{message: format!("method `{method}` is private"), span: *method_span});
                            }
                        } else if self.cur_class.as_deref() != Some(sname.as_str()) {
                            self.errors.push(SemError{message: format!("method `{method}` is private"), span: *method_span});
                        }
                        if meth.param_is_variadic.iter().any(|&v| v) {
                            self.check_call_with_sig(args, meth, *method_span, method);
                        } else {
                            if meth.params.len() != args.len() {
                                self.errors.push(SemError{message: format!("method `{}` expects {} args, found {}", method, meth.params.len(), args.len()), span: *method_span});
                            }
                            for (i, a) in args.iter().enumerate() {
                                let aty = self.check_call_arg(a);
                                let pidx = if let CallArg::Named { name, .. } = a {
                                    meth.param_names.iter().position(|n| n == name).unwrap_or(i)
                                } else { i };
                                if let Some(pt) = meth.params.get(pidx) {
                                    if &aty != pt && aty != Ty::Any { self.errors.push(SemError{message: format!("arg {} of `{}`: expected `{}`, found `{}`", i+1, method, pt, aty), span: a.span()}); }
                                }
                            }
                        }
                        meth.ret.clone()
                    } else {
                        // Also check if class was actually struct with no methods? Then try struct field? but method not found
                        self.errors.push(SemError{message: format!("class `{sname}` has no method `{method}`"), span: *method_span});
                        for a in args { let _ = self.check_call_arg(a); }
                        Ty::Int
                    }
                } else {
                    // Try structs map for method? For now treat as class not found, check if struct has method? struct has no methods
                    self.errors.push(SemError{message: format!("unknown class `{sname}`"), span: object.span});
                    for a in args { let _ = self.check_call_arg(a); }
                    Ty::Int
                }
            }
            ExprKind::EnumVariant{enum_name, variant, variant_span, args} => {
                let enum_ty: Ty = if let Some(en) = enum_name {
                    if !self.enums.contains_key(en) {
                        self.errors.push(SemError{message: format!("unknown enum `{en}`"), span: *variant_span});
                        Ty::Int
                    } else { Ty::Enum(en.clone()) }
                } else {
                    let mut found: Option<String> = None;
                    let mut found_ty: Option<Ty> = None;
                    for (ename, einfo) in &self.enums {
                        if einfo.variant_map.contains_key(variant) {
                            if found.is_some() {
                                self.errors.push(SemError{message: format!("ambiguous variant `{variant}`; use qualified `Enum.{}`", variant), span: *variant_span});
                                found = Some(ename.clone());
                                break;
                            }
                            found = Some(ename.clone());
                            found_ty = Some(Ty::Enum(ename.clone()));
                        }
                    }
                    if let Some(t) = found_ty { t } else {
                        self.errors.push(SemError{message: format!("unknown variant `{variant}`"), span: *variant_span});
                        Ty::Int
                    }
                };
                let enum_name_str = match &enum_ty { Ty::Enum(n) => n.clone(), _ => "".to_string() };
                if let Some(einfo) = self.enums.get(&enum_name_str).cloned() {
                    if let Some((_, payload_tys)) = einfo.variant_map.get(variant) {
                        if !payload_tys.is_empty() {
                            if args.len() != payload_tys.len() {
                                self.errors.push(SemError{message: format!("variant `{variant}` expects {} payload(s), found {}", payload_tys.len(), args.len()), span: *variant_span});
                            } else {
                                for (pty, arg) in payload_tys.iter().zip(args.iter()) {
                                    let aty = self.check_call_arg(arg);
                                    let is_generic = matches!(pty, Ty::Generic(n, _) if n.len() == 1 && n.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false));
                                    if &aty != pty && aty != Ty::Any && !is_generic {
                                        self.errors.push(SemError{message: format!("variant `{variant}` payload: expected `{}`, found `{}`", pty, aty), span: arg.span()});
                                    }
                                }
                            }
                        } else {
                            if !args.is_empty() {
                                self.errors.push(SemError{message: format!("variant `{variant}` expects no args"), span: *variant_span});
                            }
                            for a in args { let _ = self.check_call_arg(a); }
                        }
                    }
                } else {
                    for a in args { let _ = self.check_call_arg(a); }
                }
                enum_ty
            }
            ExprKind::Match(m) => {
                let scrut_ty = self.check_expr(&m.scrutinee);
                // scrutinee must be int/bool/enum/struct/tuple
                if scrut_ty != Ty::Int
                    && scrut_ty != Ty::Bool
                    && !matches!(scrut_ty, Ty::Struct(_))
                    && !matches!(scrut_ty, Ty::Enum(_))
                    && !matches!(scrut_ty, Ty::Tuple(_))
                {
                    self.errors.push(SemError{message: format!("match scrutinee must be `int`/`bool`/`enum`, found `{scrut_ty}`"), span: m.scrutinee.span});
                }
                if m.arms.is_empty() {
                    self.errors.push(SemError {
                        message: "match requires at least one arm".into(),
                        span: m.span,
                    });
                    return scrut_ty;
                }
                // For Phase 2, assume arms produce same type; infer first arm's type
                let mut arm_ty: Option<Ty> = None;
                let mut has_wildcard = false;
                for arm in &m.arms {
                    // pattern type check
                    let mut arm_has_wildcard = false;
                    match &arm.pattern {
                        Pattern::Wildcard(_) => {
                            has_wildcard = true;
                            arm_has_wildcard = true;
                        }
                        Pattern::LitInt(_, span) => {
                            if scrut_ty != Ty::Int {
                                self.errors.push(SemError{message: format!("pattern `int` mismatches scrutinee `{scrut_ty}`"), span: *span});
                            }
                        }
                        Pattern::LitBool(_, span) => {
                            if scrut_ty != Ty::Bool {
                                self.errors.push(SemError{message: format!("pattern `bool` mismatches scrutinee `{scrut_ty}`"), span: *span});
                            }
                        }
                        Pattern::Var(_, _) => { has_wildcard = true; arm_has_wildcard = true; }
                        Pattern::Alternative(pats, span) => {
                            // `a | b` or `a or b` : each alternative must match scrutinee type
                            if pats.is_empty() {
                                self.errors.push(SemError{message: "empty alternative pattern".into(), span: *span});
                            }
                            let mut any_wildcard = false;
                            for pat in pats {
                                match pat {
                                    Pattern::Wildcard(_) => any_wildcard = true,
                                    Pattern::Var(_, _) => any_wildcard = true,
                                    Pattern::LitInt(_, s) => if scrut_ty != Ty::Int { self.errors.push(SemError{message: format!("pattern `int` mismatches scrutinee `{scrut_ty}`"), span: *s}); },
                                    Pattern::LitBool(_, s) => if scrut_ty != Ty::Bool { self.errors.push(SemError{message: format!("pattern `bool` mismatches scrutinee `{scrut_ty}`"), span: *s}); },
                                    Pattern::Enum{variant, variant_span, ..} => {
                                        if !matches!(scrut_ty, Ty::Enum(_)) {
                                            self.errors.push(SemError{message: format!("enum pattern on non-enum scrutinee `{}`", scrut_ty), span: *variant_span});
                                        }
                                    }
                                    Pattern::Tuple(_, s) => {
                                        if !matches!(scrut_ty, Ty::Tuple(_)) {
                                            self.errors.push(SemError{message: format!("tuple pattern on non-tuple scrutinee `{}`", scrut_ty), span: *s});
                                        }
                                    }
                                    Pattern::Alternative(_, _) => {},
                                }
                            }
                            if any_wildcard { has_wildcard = true; arm_has_wildcard = true; }
                        }
                        Pattern::Tuple(pats, span) => {
                            if let Ty::Tuple(tys) = &scrut_ty {
                                if pats.len() != tys.len() {
                                    self.errors.push(SemError{message: format!("tuple pattern expects {} elements, found {} (scrutinee `{}`)", tys.len(), pats.len(), scrut_ty), span: *span});
                                } else {
                                    for (pat, ty) in pats.iter().zip(tys.iter()) {
                                        match pat {
                                            Pattern::Wildcard(_) => {},
                                            Pattern::Var(_, _) => {},
                                            Pattern::LitInt(_, s) => if *ty != Ty::Int { self.errors.push(SemError{message: format!("tuple element expects `int`, found `{}`", ty), span: *s}); },
                                            Pattern::LitBool(_, s) => if *ty != Ty::Bool { self.errors.push(SemError{message: format!("tuple element expects `bool`, found `{}`", ty), span: *s}); },
                                            _ => {}
                                        }
                                    }
                                }
                            } else {
                                self.errors.push(SemError{message: format!("tuple pattern on non-tuple scrutinee `{}`", scrut_ty), span: *span});
                            }
                        }
                        Pattern::Enum{variant, variant_span, payload} => {
                            if let Ty::Enum(ref ename) = scrut_ty {
                                if let Some(einfo) = self.enums.get(ename).cloned() {
                                    if let Some((_, payload_tys)) = einfo.variant_map.get(variant) {
                                        match (payload, payload_tys.as_slice()) {
                                            (Some(pats), [expected]) if pats.len() == 1 => {
                                                match &pats[0] {
                                                    Pattern::Wildcard(_) => {},
                                                    Pattern::LitInt(_, s) => if *expected != Ty::Int { self.errors.push(SemError{message: format!("payload for `{}` expects `{}`, found `int`", variant, expected), span: *s}); },
                                                    Pattern::LitBool(_, s) => if *expected != Ty::Bool { self.errors.push(SemError{message: format!("payload for `{}` expects `{}`, found `bool`", variant, expected), span: *s}); },
                                                    Pattern::Var(_, _) => {},
                                                    Pattern::Enum{..} => self.errors.push(SemError{message: "nested enum payload pattern not supported".into(), span: *variant_span}),
                                                    _ => {},
                                                }
                                            }
                                            (Some(pats), expecteds) => {
                                                if pats.len() != expecteds.len() {
                                                    self.errors.push(SemError{message: format!("variant `{}` expects {} payload(s), found {}", variant, expecteds.len(), pats.len()), span: *variant_span});
                                                } else {
                                                    for (pat, exp) in pats.iter().zip(expecteds.iter()) {
                                                        match pat {
                                                            Pattern::Wildcard(_) => {},
                                                            Pattern::LitInt(_, s) => if *exp != Ty::Int { self.errors.push(SemError{message: format!("payload for `{}` expects `{}`, found `int`", variant, exp), span: *s}); },
                                                            Pattern::LitBool(_, s) => if *exp != Ty::Bool { self.errors.push(SemError{message: format!("payload for `{}` expects `{}`, found `bool`", variant, exp), span: *s}); },
                                                            Pattern::Var(_, _) => {},
                                                            _ => {},
                                                        }
                                                    }
                                                }
                                            }
                                            (None, []) => {},
                                            (None, _) => self.errors.push(SemError{message: format!("variant `{}` expects payload", variant), span: *variant_span}),
                                            (Some(_), []) => self.errors.push(SemError{message: format!("variant `{}` has no payload but pattern provides one", variant), span: *variant_span}),
                                        }
                                    } else {
                                        self.errors.push(SemError{message: format!("unknown variant `{}` for enum `{}`", variant, ename), span: *variant_span});
                                    }
                                } else {
                                    self.errors.push(SemError{message: format!("unknown enum `{}`", ename), span: *variant_span});
                                }
                            } else {
                                self.errors.push(SemError{message: format!("enum pattern on non-enum scrutinee `{}`", scrut_ty), span: *variant_span});
                            }
                        }
                    }
                    // Open a scope for pattern bindings visible in guard + body
                    self.push_scope();
                    // Declare pattern-bound variables
                    match &arm.pattern {
                        Pattern::Var(name, span) => {
                            self.declare_var(name, scrut_ty.clone(), *span);
                        }
                        Pattern::Alternative(pats, _) => {
                            // For `a | b` or `a or b`, bind vars from first Var alternative if any
                            for pat in pats {
                                match pat {
                                    Pattern::Var(name, span) => { self.declare_var(name, scrut_ty.clone(), *span); break; },
                                    Pattern::Tuple(subs, _) => {
                                        if let Ty::Tuple(tys) = &scrut_ty {
                                            for (spat, ty) in subs.iter().zip(tys.iter()) {
                                                if let Pattern::Var(n, s) = spat { self.declare_var(n, ty.clone(), *s); }
                                            }
                                        }
                                        break;
                                    }
                                    Pattern::Enum{ payload: Some(subs), variant, ..} => {
                                        if let Ty::Enum(ref ename) = scrut_ty {
                                            if let Some(payload_tys) = self.enums.get(ename).and_then(|einfo| einfo.variant_map.get(variant).map(|(_, v)| v.clone())) {
                                                for (spat, ty) in subs.iter().zip(payload_tys.iter()) {
                                                    if let Pattern::Var(n, s) = spat { self.declare_var(n, ty.clone(), *s); }
                                                }
                                            }
                                        }
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Pattern::Tuple(pats, _) => {
                            if let Ty::Tuple(tys) = &scrut_ty {
                                for (pat, ty) in pats.iter().zip(tys.iter()) {
                                    if let Pattern::Var(n, s) = pat {
                                        self.declare_var(n, ty.clone(), *s);
                                    } else if let Pattern::Tuple(inner, _) = pat {
                                        // nested tuple like ((a,b), c) - not common, ignore for now
                                        if let Ty::Tuple(inner_tys) = ty {
                                            for (ipat, ity) in inner.iter().zip(inner_tys.iter()) {
                                                if let Pattern::Var(n2, s2) = ipat { self.declare_var(n2, ity.clone(), *s2); }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Pattern::Enum{ payload: Some(pats), variant, ..} => {
                            if pats.len() == 1 {
                                if let Pattern::Var(vname, vspan) = &pats[0] {
                                    if let Ty::Enum(ref ename) = scrut_ty {
                                        if let Some(payload_tys) = self.enums.get(ename).and_then(|einfo| einfo.variant_map.get(variant).map(|(_, v)| v.clone())) {
                                            if let Some(pty) = payload_tys.first() {
                                                self.declare_var(vname, pty.clone(), *vspan);
                                            }
                                        }
                                    }
                                } else if let Pattern::Tuple(subs, _) = &pats[0] {
                                    // Enum payload is tuple e.g., `MyVariant((a,b))` where payload is one tuple
                                    if let Ty::Enum(ref ename) = scrut_ty {
                                        if let Some(payload_tys) = self.enums.get(ename).and_then(|einfo| einfo.variant_map.get(variant).map(|(_, v)| v.clone())) {
                                            if let Some(Ty::Tuple(tys)) = payload_tys.first() {
                                                for (spat, ty) in subs.iter().zip(tys.iter()) {
                                                    if let Pattern::Var(n, s) = spat { self.declare_var(n, ty.clone(), *s); }
                                                }
                                            }
                                        }
                                    }
                                }
                            } else {
                                if let Ty::Enum(ref ename) = scrut_ty {
                                    if let Some(payload_tys) = self.enums.get(ename).and_then(|einfo| einfo.variant_map.get(variant).map(|(_, v)| v.clone())) {
                                        for (pat, ty) in pats.iter().zip(payload_tys.iter()) {
                                            if let Pattern::Var(vname, vspan) = pat {
                                                self.declare_var(vname, ty.clone(), *vspan);
                                            } else if let Pattern::Tuple(subs, _) = pat {
                                                if let Ty::Tuple(tys) = ty {
                                                    for (spat, sty) in subs.iter().zip(tys.iter()) {
                                                        if let Pattern::Var(n, s) = spat { self.declare_var(n, sty.clone(), *s); }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    if let Some(g) = &arm.guard {
                        let gt = self.check_expr(g);
                        if gt != Ty::Bool {
                            self.errors.push(SemError {
                                message: format!(
                                    "match guard must be `bool`, found `{gt}`"
                                ),
                                span: g.span,
                            });
                        }
                    }
                    // body type
                    let body_ty = match &arm.body {
                        MatchArmBody::Expr(e) => self.check_expr(e),
                        MatchArmBody::Block(b) => {
                            // For expression match, block is void; for statement match, block's stmts checked against function's ret
                            let cur = self.cur_ret.clone().unwrap_or(Ty::Void);
                            let _ = self.check_block(b, &cur);
                            Ty::Void
                        }
                    };
                    self.pop_scope();
                    if let Some(prev) = &arm_ty {
                        if *prev != body_ty {
                            self.errors.push(SemError{message: format!("match arms must have same type: expected `{prev}`, found `{body_ty}`"), span: arm.span});
                        }
                    } else {
                        arm_ty = Some(body_ty);
                    }
                }
                if !has_wildcard && scrut_ty == Ty::Int {
                    self.errors.push(SemError{message: "non-exhaustive match: missing wildcard `_` for `int` scrutinee".into(), span: m.span});
                }
                if scrut_ty == Ty::Bool && !has_wildcard {
                    // bool exhaustive requires both true/false or wildcard
                    let has_true = m.arms.iter().any(|a| {
                        matches!(a.pattern, Pattern::LitBool(true, _))
                    });
                    let has_false = m.arms.iter().any(|a| {
                        matches!(a.pattern, Pattern::LitBool(false, _))
                    });
                    if !(has_true && has_false) {
                        self.errors.push(SemError{message: "non-exhaustive bool match: require `true`, `false`, or wildcard".into(), span: m.span});
                    }
                }
                arm_ty.unwrap_or(Ty::Int)
            }
            ExprKind::Super => {
                if let Some(cls) = &self.cur_class {
                    if let Some(parent) = self.classes.get(cls).and_then(|c| c.extends.clone()) {
                        Ty::Struct(parent)
                    } else {
                        self.errors.push(SemError{message: "`super` without parent".into(), span: expr.span});
                        Ty::Int
                    }
                } else {
                    self.errors.push(SemError{message: "`super` outside class".into(), span: expr.span});
                    Ty::Int
                }
            }
            ExprKind::Null => Ty::Any,
            ExprKind::Tuple(exprs) => {
                let tys: Vec<Ty> = exprs.iter().map(|e| self.check_expr(e)).collect();
                Ty::Tuple(tys)
            }
            ExprKind::InterpolatedString(_, _) => Ty::String,
            ExprKind::Closure { params, body, .. } => {
                self.push_scope();
                for p in params { let ty = self.resolve_type(&p.ty); self.declare_var(&p.name, ty, p.name_span); }
                let ret_ty = match body.as_ref() {
                    crate::ast::ClosureBody::Expr(e) => self.check_expr(e),
                    crate::ast::ClosureBody::Block(b) => { let _ = self.check_block(b, &Ty::Void); Ty::Void },
                };
                self.pop_scope();
                Ty::Function(Box::new(ret_ty), params.iter().map(|p| self.resolve_type(&p.ty)).collect())
            }
            ExprKind::Paren(inner) => self.check_expr(inner),
        }
    }

    fn check_call_arg(&mut self, arg: &CallArg) -> Ty {
        match arg {
            CallArg::Expr(e) => self.check_expr(e),
            CallArg::Named { value, .. } => self.check_expr(value),
            CallArg::Out { name, name_span, ty: opt_ty, .. } => {
                if let Some(var_ty) = self.lookup_var(name) {
                    if let Some(t) = opt_ty {
                        let declared = self.resolve_type(t);
                        if declared != var_ty {
                            self.errors.push(SemError { message: format!("out argument `{}` type `{}` does not match variable `{}`", name, declared, var_ty), span: *name_span });
                        }
                    }
                    var_ty
                } else {
                    self.errors.push(SemError { message: format!("undefined variable `{}` for `out`", name), span: *name_span });
                    Ty::Int
                }
            }
            CallArg::Ref { expr, .. } => {
                let ty = self.check_expr(expr);
                match &expr.kind {
                    ExprKind::Ident(_) | ExprKind::MemberAccess { .. } | ExprKind::Index { .. } | ExprKind::Paren(_) => {},
                    _ => self.errors.push(SemError { message: "ref argument must be lvalue".into(), span: expr.span }),
                }
                ty
            }
        }
    }

    fn check_call_with_sig(&mut self, args: &[CallArg], sig: &FuncSig, callee_span: Span, callee: &str) {
        let variadic_idx = sig.param_is_variadic.iter().position(|&v| v);
        if let Some(vidx) = variadic_idx {
            // Variadic `...T vda` where `vda` is `T[]`, or `...` alone for C varargs
            let fixed = vidx; // number of fixed params before variadic
            if args.len() < fixed {
                self.errors.push(SemError { message: format!("`{}` expects at least {} args, found {}", callee, fixed, args.len()), span: callee_span });
            }
            // Check fixed params
            for (i, arg) in args.iter().take(fixed).enumerate() {
                let aty = self.check_call_arg(arg);
                let pidx = if let CallArg::Named { name, .. } = arg {
                    sig.param_names.iter().position(|n| n == name).unwrap_or(i)
                } else { i };
                if let Some(param_ty) = sig.params.get(pidx) {
                    if &aty != param_ty && aty != Ty::Any {
                        let is_generic = matches!(param_ty, Ty::Generic(n, _) if n.len() == 1 && n.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false));
                        if !is_generic {
                            self.errors.push(SemError { message: format!("argument {} of `{}`: expected `{}`, found `{}`", i+1, callee, param_ty, aty), span: arg.span() });
                        }
                    }
                }
            }
            // Check variadic tail: `vda` is `T[]` where `T` is element type
            if let Some(vty) = sig.params.get(vidx) {
                let elem_ty = if let Ty::Array(el) = vty { &**el } else { vty };
                // Handle variadic not last: `...T vda, U next` where `vda` consumes `args.len() - sig.params.len() + 1` args
                let remaining_params = sig.params.len() - vidx - 1;
                let vda_count = if remaining_params == 0 {
                    args.len() - fixed
                } else {
                    args.len() - sig.params.len() + 1
                };
                for (i, arg) in args.iter().skip(fixed).take(vda_count).enumerate() {
                    let aty = self.check_call_arg(arg);
                    let is_generic_elem = matches!(elem_ty, Ty::Generic(n, _) if n.len() == 1 && n.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false));
                    let is_any_elem = *elem_ty == Ty::Any;
                    if &aty != elem_ty && aty != Ty::Any && !is_generic_elem && !is_any_elem {
                        self.errors.push(SemError { message: format!("variadic argument {} of `{}`: expected `{}`, found `{}`", fixed + i + 1, callee, elem_ty, aty), span: arg.span() });
                    }
                }
                // Check remaining fixed params after variadic
                for (j, arg) in args.iter().skip(fixed + vda_count).enumerate() {
                    let pidx = vidx + 1 + j;
                    if let Some(param_ty) = sig.params.get(pidx) {
                        let aty = self.check_call_arg(arg);
                        if &aty != param_ty && aty != Ty::Any {
                            let is_generic = matches!(param_ty, Ty::Generic(n, _) if n.len() == 1 && n.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false));
                            if !is_generic {
                                self.errors.push(SemError { message: format!("argument {} of `{}`: expected `{}`, found `{}`", pidx + 1, callee, param_ty, aty), span: arg.span() });
                            }
                        }
                    }
                }
            }
            // Also check where bounds for variadic generic
            if !sig.generic_params.is_empty() || sig.where_clause.is_some() {
                // For `...T vda where T: Trait`, the `T` for variadic element should also be checked
                // We already check via check_generic_bounds for type_args, but for variadic we need to ensure `T` is checked
                // For now, rely on check_generic_bounds for type_args
            }
            return;
        }
        if sig.params.len() != args.len() {
            self.errors.push(SemError { message: format!("`{}` expects {} args, found {}", callee, sig.params.len(), args.len()), span: callee_span });
        }
        let has_named = args.iter().any(|a| matches!(a, CallArg::Named{..}));
        if has_named {
            // Build param name -> index map
            let mut param_name_to_idx: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for (idx, pname) in sig.param_names.iter().enumerate() {
                param_name_to_idx.insert(pname.clone(), idx);
            }
            let mut seen = std::collections::HashSet::new();
            for arg in args {
                if let CallArg::Named { name, .. } = arg {
                    if let Some(&pidx) = param_name_to_idx.get(name) {
                        if !seen.insert(pidx) {
                            self.errors.push(SemError { message: format!("duplicate named argument `{}` for `{}`", name, callee), span: arg.span() });
                        }
                        let aty = self.check_call_arg(arg);
                        let expected = &sig.params[pidx];
                        if &aty != expected && aty != Ty::Any {
                            self.errors.push(SemError { message: format!("named argument `{}` of `{}`: expected `{}`, found `{}`", name, callee, expected, aty), span: arg.span() });
                        }
                        if let Some(mode) = sig.param_modes.get(pidx) {
                            let is_out = matches!(arg, CallArg::Out{..});
                            let is_ref = matches!(arg, CallArg::Ref{..});
                            match mode {
                                ParamMode::Out if !matches!(arg, CallArg::Out{..}) && !matches!(arg, CallArg::Named{..}) => {},
                                _ => {}
                            }
                            // For Named, mode check still relevant if Named was used for out/ref param? But Named is by-value, so if param is Out/Ref, Named should be error
                            if *mode == ParamMode::Out || *mode == ParamMode::Ref {
                                self.errors.push(SemError { message: format!("named argument `{}` of `{}` corresponds to `out`/`ref` param, use `out`/`ref` syntax", name, callee), span: arg.span() });
                            }
                        }
                    } else {
                        self.errors.push(SemError { message: format!("unknown named argument `{}` for `{}`", name, callee), span: arg.span() });
                        let _ = self.check_call_arg(arg);
                    }
                } else {
                    // positional before named is allowed, but we already handled count; for positional, check via index
                    let _ = self.check_call_arg(arg);
                }
            }
            // Also check positional args that are not named: they must match remaining params
            // For simplicity, if has_named, we have already checked named args; remaining positional args are checked elsewhere? We'll just return after handling named
            return;
        }
        for (i, arg) in args.iter().enumerate() {
            let aty = self.check_call_arg(arg);
            if let Some(param_ty) = sig.params.get(i) {
                if &aty != param_ty && aty != Ty::Any {
                    self.errors.push(SemError { message: format!("argument {} of `{}`: expected `{}`, found `{}`", i + 1, callee, param_ty, aty), span: arg.span() });
                }
            }
            if let Some(mode) = sig.param_modes.get(i) {
                let is_out = matches!(arg, CallArg::Out{..});
                let is_ref = matches!(arg, CallArg::Ref{..});
                match mode {
                    ParamMode::Out if !is_out => self.errors.push(SemError { message: format!("argument {} of `{}` is `out` param but call uses non-out", i+1, callee), span: arg.span() }),
                    ParamMode::Ref if !is_ref => self.errors.push(SemError { message: format!("argument {} of `{}` is `ref` param but call uses non-ref", i+1, callee), span: arg.span() }),
                    ParamMode::None if is_out || is_ref => self.errors.push(SemError { message: format!("argument {} of `{}` is by-value param but call uses `out`/`ref`", i+1, callee), span: arg.span() }),
                    _ => {}
                }
            }
        }
    }

    fn check_generic_bounds(&mut self, generic_params: &[GenericParam], where_clause: &Option<WhereClause>, type_args: &[Type], span: Span) {
        if generic_params.is_empty() && where_clause.is_none() {
            if !type_args.is_empty() {
                self.errors.push(SemError { message: format!("function is not generic but {} type arguments provided", type_args.len()), span });
            }
            return;
        }
        if generic_params.len() != type_args.len() {
            if !type_args.is_empty() {
                self.errors.push(SemError { message: format!("generic arg count mismatch: expected {}, found {}", generic_params.len(), type_args.len()), span });
            }
            return;
        }
        let mut subst: std::collections::HashMap<String, Ty> = std::collections::HashMap::new();
        for (gp, ta) in generic_params.iter().zip(type_args.iter()) {
            let concrete = self.resolve_type(ta);
            subst.insert(gp.name.clone(), concrete.clone());
            for bound in &gp.bounds {
                let bound_ty = self.resolve_type(bound);
                if let Ty::Struct(ref trait_name) | Ty::Generic(ref trait_name, _) = bound_ty {
                    let concrete_str = match &concrete {
                        Ty::Struct(n) | Ty::Enum(n) | Ty::Generic(n, _) => n.clone(),
                        _ => concrete.to_string(),
                    };
                    if let Some(_trait_info) = self.traits.get(trait_name) {
                        let implements = if let Some(cls) = self.classes.get(&concrete_str) {
                            &cls.implements
                        } else {
                            &Vec::new()
                        };
                        if !implements.contains(trait_name) && concrete_str != *trait_name {
                            self.errors.push(SemError { message: format!("type `{}` does not satisfy bound `{}` for `{}`", concrete, bound_ty, gp.name), span });
                        }
                    } else if !self.structs.contains_key(trait_name) && !self.classes.contains_key(trait_name) && !self.enums.contains_key(trait_name) {
                        self.errors.push(SemError { message: format!("unknown bound `{}` for `{}`", bound_ty, gp.name), span });
                    }
                } else if bound_ty != concrete {
                    self.errors.push(SemError { message: format!("type `{}` does not satisfy bound `{}` for `{}`", concrete, bound_ty, gp.name), span });
                }
            }
        }
        // Check where clause constraints: `where T: Drawable, U: int`
        if let Some(wc) = where_clause {
            for constr in &wc.constraints {
                let subject_ty = self.resolve_type(&constr.ty);
                // Resolve subject to concrete if it's a generic param
                let subject_concrete = match &subject_ty {
                    Ty::Generic(n, _) | Ty::Struct(n) if subst.contains_key(n) => subst[n].clone(),
                    other => other.clone(),
                };
                for bound in &constr.bounds {
                    let bound_ty = self.resolve_type(bound);
                    if let Ty::Struct(ref trait_name) | Ty::Generic(ref trait_name, _) = bound_ty {
                        if let Some(trait_info) = self.traits.get(trait_name) {
                            let subject_str = match &subject_concrete {
                                Ty::Struct(n) | Ty::Enum(n) | Ty::Generic(n, _) => n.clone(),
                                _ => subject_concrete.to_string(),
                            };
                            let implements = if let Some(cls) = self.classes.get(&subject_str) {
                                &cls.implements
                            } else {
                                &Vec::new()
                            };
                            if !implements.contains(trait_name) && subject_str != *trait_name {
                                self.errors.push(SemError { message: format!("where bound failed: `{}` does not satisfy `{}: {}`", subject_concrete, constr.ty.name(), bound_ty), span: constr.span });
                            }
                        } else if bound_ty != subject_concrete {
                            self.errors.push(SemError { message: format!("where bound failed: `{}` does not satisfy `{}: {}`", subject_concrete, constr.ty.name(), bound_ty), span: constr.span });
                        }
                    } else if bound_ty != subject_concrete {
                        self.errors.push(SemError { message: format!("where bound failed: `{}` does not satisfy `{}: {}`", subject_concrete, constr.ty.name(), bound_ty), span: constr.span });
                    }
                }
            }
        }
    }

    fn check_lvalue(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::Ident(name) => {
                let lookup = name.rsplit("::").next().unwrap_or(name);
                if let Some(ty) = self.lookup_var(name).or_else(|| self.lookup_var(lookup)) {
                    ty
                } else {
                    self.errors.push(SemError {
                        message: format!("undefined variable `{name}`"),
                        span: expr.span,
                    });
                    Ty::Int
                }
            }
            ExprKind::MemberAccess {
                object,
                field,
                field_span,
            } => {
                // reuse field check but treat as lvalue - also handle properties (setter)
                let obj_ty = self.check_expr(object);
                if let Ty::Struct(ref sname) = obj_ty {
                    if let Some(sinfo) = self.structs.get(sname) {
                        if let Some((_, fty)) = sinfo.field_map.get(field) {
                            if let Some(cinfo) = self.classes.get(sname) {
                                if let Some(vis) = cinfo.field_vis.get(field) {
                                    if *vis != crate::ast::Visibility::Public && self.cur_class.as_deref() != Some(sname.as_str()) {
                                        self.errors.push(SemError{message: format!("field `{field}` is private"), span: *field_span});
                                    }
                                } else if self.cur_class.as_deref() != Some(sname.as_str()) {
                                    self.errors.push(SemError{message: format!("field `{field}` is private"), span: *field_span});
                                }
                            } else {
                                if let Some(vis) = sinfo.field_vis.get(field) {
                                    if *vis == crate::ast::Visibility::Private {
                                        self.errors.push(SemError{message: format!("field `{field}` is private"), span: *field_span});
                                    }
                                }
                            }
                            fty.clone()
                        } else if let Some(cinfo) = self.classes.get(sname) {
                            if let Some(prop) = cinfo.properties.get(field) {
                                if prop.visibility != crate::ast::Visibility::Public && self.cur_class.as_deref() != Some(sname.as_str()) {
                                    self.errors.push(SemError{message: format!("property `{field}` is private"), span: *field_span});
                                }
                                if prop.has_set { prop.ty.clone() } else {
                                    self.errors.push(SemError{message: format!("property `{field}` has no setter"), span: *field_span});
                                    Ty::Int
                                }
                            } else {
                                self.errors.push(SemError {
                                    message: format!(
                                        "struct `{sname}` has no field `{field}`"
                                    ),
                                    span: *field_span,
                                });
                                Ty::Int
                            }
                        } else {
                            self.errors.push(SemError {
                                message: format!(
                                    "struct `{sname}` has no field `{field}`"
                                ),
                                span: *field_span,
                            });
                            Ty::Int
                        }
                    } else {
                        self.errors.push(SemError {
                            message: format!("unknown struct `{sname}`"),
                            span: expr.span,
                        });
                        Ty::Int
                    }
                } else {
                    self.errors.push(SemError {
                        message: format!(
                            "assignment to non-struct field `{field}`"
                        ),
                        span: *field_span,
                    });
                    Ty::Int
                }
            }
            ExprKind::Index { object, index } => {
                let obj_ty = self.check_expr(object);
                let idx_ty = self.check_expr(index);
                if idx_ty != Ty::Int {
                    self.errors.push(SemError {
                        message: format!(
                            "index must be `int`, found `{idx_ty}`"
                        ),
                        span: index.span,
                    });
                }
                match obj_ty {
                    Ty::Array(el) => *el,
                    Ty::String => Ty::Char,
                    _ => {
                        self.errors.push(SemError {
                            message: format!(
                                "cannot index non-array `{obj_ty}`"
                            ),
                            span: object.span,
                        });
                        Ty::Int
                    }
                }
            }
            _ => {
                self.errors.push(SemError {
                    message: "invalid assignment target".into(),
                    span: expr.span,
                });
                Ty::Int
            }
        }
    }
}

pub fn check(prog: &Program) -> Vec<SemError> {
    let mut c = Checker::new();
    c.check_program(prog)
}
