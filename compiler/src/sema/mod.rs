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
    Struct(String),
    Enum(String),
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
            Type::Named(n, _) => Ty::Struct(n.clone()),
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
            Ty::Struct(n) => write!(f, "{}", n),
            Ty::Enum(n) => write!(f, "{}", n),
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
    span: Span,
}

#[derive(Clone, Debug)]
struct StructInfo {
    name: String,
    fields: Vec<(String, Ty)>, // ordered
    field_map: HashMap<String, (usize, Ty)>,
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
    errors: Vec<SemError>,
    cur_ret: Option<Ty>,
    cur_class: Option<String>,
    loop_stack: Vec<Option<String>>,
}

#[derive(Clone, Debug)]
struct EnumInfo {
    name: String,
    variants: Vec<EnumVariantInfo>,
    variant_map: HashMap<String, (usize, Option<Ty>)>, // variant -> (tag, payload ty)
    span: Span,
}

#[derive(Clone, Debug)]
struct EnumVariantInfo {
    name: String,
    tag: usize,
    payload_ty: Option<Ty>,
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
            errors: Vec::new(),
            cur_ret: None,
            cur_class: None,
            loop_stack: Vec::new(),
        }
    }

    fn loop_depth(&self) -> usize { self.loop_stack.len() }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
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
        // Handle enum vs struct vs class distinction for Named types
        if let Ty::Struct(ref n) = t {
            if self.enums.contains_key(n) {
                t = Ty::Enum(n.clone());
            } else if !self.structs.contains_key(n) && !self.classes.contains_key(n) {
                self.errors.push(SemError{message: format!("unknown type `{n}`"), span: ty.span()});
            }
        }
        // For compound types, ensure inner is known (array element etc) – From already handled, but check nested struct/enum existence
        match &t {
            Ty::Array(el) | Ty::Pointer(el) | Ty::Optional(el) => {
                if let Ty::Struct(ref n) = **el {
                    if !self.structs.contains_key(n) && !self.classes.contains_key(n) && !self.enums.contains_key(n) {
                        self.errors.push(SemError{message: format!("unknown type `{n}`"), span: ty.span()});
                    }
                }
                if let Ty::Enum(ref n) = **el {
                    if !self.enums.contains_key(n) { self.errors.push(SemError{message: format!("unknown type `{n}`"), span: ty.span()}); }
                }
            }
            _ => {}
        }
        t
    }

    pub fn check_program(&mut self, prog: &Program) -> Vec<SemError> {
        // First pass: collect struct definitions
        for item in &prog.items {
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
                        fmap.insert(f.name.clone(), (idx, fty.clone()));
                        fields.push((f.name.clone(), fty));
                    }
                    self.structs.insert(
                        s.name.clone(),
                        StructInfo {
                            name: s.name.clone(),
                            fields,
                            field_map: fmap,
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
                            let param_tys: Vec<Ty> = m.params.iter().map(|p| {
                                let ty = self.resolve_type(&p.ty);
                                if ty == Ty::Void { self.errors.push(SemError{message: format!("parameter `{}` cannot be `void`", p.name), span: p.span}); }
                                ty
                            }).collect();
                            let ret_ty = self.resolve_type(&m.ret_ty);
                            methods.insert(m.name.clone(), FuncSig{ret: ret_ty, params: param_tys, span: m.name_span});
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
                    // Inherit parent fields if extends
                    if let Some(ref parent) = extends_name {
                        if let Some(pinfo) = self.classes.get(parent).cloned() {
                            for (idx, (fname, fty)) in pinfo.fields.iter().enumerate() {
                                fmap.insert(fname.clone(), (idx, fty.clone()));
                                if let Some(v) = pinfo.field_vis.get(fname) { fvis.insert(fname.clone(), *v); }
                                else { fvis.insert(fname.clone(), crate::ast::Visibility::Default); }
                                fields.push((fname.clone(), fty.clone()));
                                seen.insert(fname.clone());
                            }
                        } else if let Some(sinfo) = self.structs.get(parent).cloned() {
                            for (idx, (fname, fty)) in sinfo.fields.iter().enumerate() {
                                fmap.insert(fname.clone(), (idx, fty.clone()));
                                fvis.insert(fname.clone(), crate::ast::Visibility::Default);
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
                        let real_idx = offset + idx;
                        fmap.insert(f.name.clone(), (real_idx, fty.clone()));
                        fvis.insert(f.name.clone(), f.visibility);
                        fields.push((f.name.clone(), fty));
                    }
                    // Also insert class layout into structs map for field access / instantiation
                    self.structs.insert(c.name.clone(), StructInfo{name: c.name.clone(), fields: fields.clone(), field_map: fmap.clone(), span: c.span});
                    // Collect methods
                    let mut methods = HashMap::new();
                    let mut method_vis: HashMap<String, crate::ast::Visibility> = HashMap::new();
                    for m in &c.methods {
                        if methods.contains_key(&m.name) {
                            self.errors.push(SemError{message: format!("duplicate method `{}` in class `{}`", m.name, c.name), span: m.name_span});
                        } else {
                            let param_tys: Vec<Ty> = m.params.iter().map(|p| {
                                let t = self.resolve_type(&p.ty);
                                if t == Ty::Void { self.errors.push(SemError{message: format!("parameter `{}` cannot be `void`", p.name), span: p.span}); }
                                t
                            }).collect();
                            let ret_ty = self.resolve_type(&m.ret_ty);
                            let mut pseen = HashSet::new();
                            for p in &m.params { if !pseen.insert(&p.name) { self.errors.push(SemError{message: format!("duplicate param `{}`", p.name), span: p.name_span}); } }
                            methods.insert(m.name.clone(), FuncSig{ret: ret_ty, params: param_tys, span: m.name_span});
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
                        for p in &ctor.params {
                            let ty = self.resolve_type(&p.ty);
                            if ty == Ty::Void { self.errors.push(SemError{message: format!("constructor param `{}` cannot be `void`", p.name), span: p.span}); }
                            if !pseen.insert(&p.name) { self.errors.push(SemError{message: format!("duplicate param `{}` in constructor", p.name), span: p.name_span}); }
                            param_tys.push(ty);
                        }
                        // constructors are void return
                        ctor_sigs.push((FuncSig{ret: Ty::Void, params: param_tys, span: ctor.name_span}, ctor.visibility));
                    }
                    // Validate destructors: name must match class name
                    for dtor in &c.destructors {
                        if dtor.name != c.name {
                            self.errors.push(SemError{message: format!("destructor name `~{}` must match class name `{}`", dtor.name, c.name), span: dtor.name_span});
                        }
                    }
                    // Collect properties
                    let mut prop_map = HashMap::new();
                    for prop in &c.properties {
                        if seen.contains(&prop.name) || methods.contains_key(&prop.name) || prop_map.contains_key(&prop.name) {
                            self.errors.push(SemError{message: format!("duplicate property/member `{}` in class `{}`", prop.name, c.name), span: prop.name_span});
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
                    self.classes.insert(c.name.clone(), ClassInfo{name: c.name.clone(), fields, field_map: fmap, field_vis: fvis, methods, method_vis, constructors: ctor_sigs, properties: prop_map, is_open: c.is_open, is_sealed: c.is_sealed, extends: extends_name.clone(), implements: implements_names.clone(), span: c.span});
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
                        let payload_ty = v.payload_ty.as_ref().map(|t| {
                            let pt = self.resolve_type(t);
                            if pt == Ty::Void { self.errors.push(SemError{message: format!("variant `{}` payload cannot be `void`", v.name), span: v.span}); }
                            pt
                        });
                        let tag = v.discriminant.map(|d| d as usize).unwrap_or(idx);
                        vmap.insert(v.name.clone(), (tag, payload_ty.clone()));
                        variants.push(EnumVariantInfo{name: v.name.clone(), tag, payload_ty, span: v.span});
                    }
                    self.enums.insert(e.name.clone(), EnumInfo{name: e.name.clone(), variants, variant_map: vmap, span: e.span});
                }
            }
        }
        // Second pass: collect function signatures
        for item in &prog.items {
            if let Item::Function(f) = item {
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
                        .map(|p| {
                            let t = self.resolve_type(&p.ty);
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
                            span: f.name_span,
                        },
                    );
                }
            }
        }
        // Validate main
        if let Some(main) = self.funcs.get("main").cloned() {
            if !((main.ret == Ty::Void && main.params.is_empty())
                || (main.ret == Ty::Int && main.params.is_empty()))
            {
                self.errors.push(SemError{message: format!("invalid `main` signature: expected `void main()` or `int main()`, found `{} main({})`", main.ret, main.params.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")), span: main.span});
            }
        } else {
            self.errors.push(SemError {
                message: "missing `main` function".into(),
                span: prog.span,
            });
        }

        // Third pass: check function bodies
        for item in &prog.items {
            if let Item::Function(f) = item {
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

    fn check_method(&mut self, class_name: &str, f: &Function) {
        let ret_ty = self.resolve_type(&f.ret_ty);
        self.cur_ret = Some(ret_ty.clone());
        self.cur_class = Some(class_name.to_string());
        self.push_scope();
        // implicit `this`
        self.declare_var("this", Ty::Struct(class_name.to_string()), f.name_span);
        for p in &f.params {
            let ty = self.resolve_type(&p.ty);
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
                    if init_ty != decl_ty && decl_ty != Ty::Void {
                        self.errors.push(SemError{message: format!("type mismatch in initializer: expected `{decl_ty}`, found `{init_ty}`"), span: init.span});
                    }
                }
                self.declare_var(&d.name, decl_ty, d.name_span);
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
            ExprKind::BoolLit(_) => Ty::Bool,
            ExprKind::Ident(name) => {
                if let Some(ty) = self.lookup_var(name) {
                    ty
                } else {
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
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let lt = self.check_expr(lhs);
                let rt = self.check_expr(rhs);
                match op {
                    BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Mod => {
                        if lt != Ty::Int || rt != Ty::Int {
                            self.errors.push(SemError{message: format!("arithmetic `{op:?}` requires `int`, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Int
                    }
                    BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                        if lt != Ty::Int || rt != Ty::Int {
                            self.errors.push(SemError{message: format!("comparison `{op:?}` requires `int`, found `{lt}` and `{rt}`"), span: expr.span});
                        }
                        Ty::Bool
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
                }
            }
            ExprKind::Assign { lhs, value } => {
                let lhs_ty = self.check_lvalue(lhs);
                let rhs_ty = self.check_expr(value);
                if lhs_ty != rhs_ty {
                    self.errors.push(SemError{message: format!("assignment type mismatch: expected `{lhs_ty}`, found `{rhs_ty}`"), span: expr.span});
                }
                lhs_ty
            }
            ExprKind::Call {
                callee,
                callee_span,
                args,
            } => {
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
                                    let aty = self.check_expr(arg);
                                    if &aty != &sig.params[i] {
                                        self.errors.push(SemError{message: format!("ctor arg {}: expected `{}`, found `{aty}`", i+1, sig.params[i]), span: arg.span});
                                    }
                                }
                                return struct_ty;
                            } else {
                                self.errors.push(SemError{message: format!("no matching constructor for `{callee}` with {} args", args.len()), span: *callee_span});
                                for arg in args { let _ = self.check_expr(arg); }
                                return struct_ty;
                            }
                        }
                    }
                    // No explicit ctor: check against fields (struct literal via call)
                    let field_tys: Vec<Ty> = self.structs.get(callee).map(|s| s.fields.iter().map(|(_,ty)| ty.clone()).collect()).unwrap_or_default();
                    if !field_tys.is_empty() && args.len() == field_tys.len() {
                        for (i, arg) in args.iter().enumerate() {
                            let aty = self.check_expr(arg);
                            let fty = &field_tys[i];
                            if &aty != fty {
                                self.errors.push(SemError{message: format!("ctor arg {}: expected `{}`, found `{aty}`", i+1, fty), span: arg.span});
                            }
                        }
                        return struct_ty;
                    }
                    // Fallback: just type-check args and return struct
                    for arg in args { let _ = self.check_expr(arg); }
                    return struct_ty;
                }
                let sig = self.funcs.get(callee).cloned();
                if let Some(sig) = sig {
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
                        let aty = self.check_expr(arg);
                        if let Some(param_ty) = sig.params.get(i) {
                            if &aty != param_ty {
                                self.errors.push(SemError{message: format!("argument {} of `{callee}`: expected `{}`, found `{aty}`", i+1, param_ty), span: arg.span});
                            }
                        }
                    }
                    sig.ret
                } else {
                    self.errors.push(SemError {
                        message: format!("undefined function `{callee}`"),
                        span: *callee_span,
                    });
                    for arg in args {
                        let _ = self.check_expr(arg);
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
                // Check missing fields
                for (fname, _) in &sinfo.fields {
                    if !seen.contains(fname) {
                        self.errors.push(SemError {
                            message: format!(
                                "missing field `{fname}` in `{sname}` literal"
                            ),
                            span: expr.span,
                        });
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
                        for a in args { let _ = self.check_expr(a); }
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
                        if meth.params.len() != args.len() {
                            self.errors.push(SemError{message: format!("method `{}` expects {} args, found {}", method, meth.params.len(), args.len()), span: *method_span});
                        }
                        for (i, a) in args.iter().enumerate() {
                            let aty = self.check_expr(a);
                            if let Some(pt) = meth.params.get(i) {
                                if &aty != pt { self.errors.push(SemError{message: format!("arg {} of `{}`: expected `{}`, found `{}`", i+1, method, pt, aty), span: a.span}); }
                            }
                        }
                        meth.ret.clone()
                    } else {
                        // Also check if class was actually struct with no methods? Then try struct field? but method not found
                        self.errors.push(SemError{message: format!("class `{sname}` has no method `{method}`"), span: *method_span});
                        for a in args { let _ = self.check_expr(a); }
                        Ty::Int
                    }
                } else {
                    // Try structs map for method? For now treat as class not found, check if struct has method? struct has no methods
                    self.errors.push(SemError{message: format!("unknown class `{sname}`"), span: object.span});
                    for a in args { let _ = self.check_expr(a); }
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
                    if let Some((_, payload_ty)) = einfo.variant_map.get(variant) {
                        if let Some(pt) = payload_ty {
                            if args.len() != 1 {
                                self.errors.push(SemError{message: format!("variant `{variant}` expects 1 payload, found {}", args.len()), span: *variant_span});
                            } else {
                                let aty = self.check_expr(&args[0]);
                                if &aty != pt { self.errors.push(SemError{message: format!("variant `{variant}` payload: expected `{}`, found `{}`", pt, aty), span: args[0].span}); }
                            }
                        } else {
                            if !args.is_empty() {
                                self.errors.push(SemError{message: format!("variant `{variant}` expects no args"), span: *variant_span});
                            }
                            for a in args { let _ = self.check_expr(a); }
                        }
                    }
                } else {
                    for a in args { let _ = self.check_expr(a); }
                }
                enum_ty
            }
            ExprKind::Match(m) => {
                let scrut_ty = self.check_expr(&m.scrutinee);
                // scrutinee must be int or bool for Phase 2 simple
                if scrut_ty != Ty::Int
                    && scrut_ty != Ty::Bool
                    && !matches!(scrut_ty, Ty::Struct(_))
                    && !matches!(scrut_ty, Ty::Enum(_))
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
                        Pattern::Enum{variant, variant_span, payload} => {
                            if let Ty::Enum(ref ename) = scrut_ty {
                                if let Some(einfo) = self.enums.get(ename).cloned() {
                                    if let Some((_, pty_opt)) = einfo.variant_map.get(variant) {
                                        match (payload, pty_opt) {
                                            (Some(inner), Some(expected)) => {
                                                match inner.as_ref() {
                                                    Pattern::Wildcard(_) => {},
                                                    Pattern::LitInt(_, s) => if *expected != Ty::Int { self.errors.push(SemError{message: format!("payload for `{}` expects `{}`, found `int`", variant, expected), span: *s}); },
                                                    Pattern::LitBool(_, s) => if *expected != Ty::Bool { self.errors.push(SemError{message: format!("payload for `{}` expects `{}`, found `bool`", variant, expected), span: *s}); },
                                                    Pattern::Var(_, _) => {},
                                                    Pattern::Enum{..} => self.errors.push(SemError{message: "nested enum payload pattern not supported".into(), span: *variant_span}),
                                                }
                                            }
                                            (None, Some(_)) => self.errors.push(SemError{message: format!("variant `{}` expects payload", variant), span: *variant_span}),
                                            (Some(_), None) => self.errors.push(SemError{message: format!("variant `{}` has no payload but pattern provides one", variant), span: *variant_span}),
                                            (None, None) => {},
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
                        Pattern::Enum{ payload: Some(inner), variant, ..} => {
                            if let Pattern::Var(vname, vspan) = inner.as_ref() {
                                if let Ty::Enum(ref ename) = scrut_ty {
                                    if let Some(einfo) = self.enums.get(ename) {
                                        if let Some((_, Some(pty))) = einfo.variant_map.get(variant) {
                                            self.declare_var(vname, pty.clone(), *vspan);
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
        }
    }

    fn check_lvalue(&mut self, expr: &Expr) -> Ty {
        match &expr.kind {
            ExprKind::Ident(name) => {
                if let Some(ty) = self.lookup_var(name) {
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
