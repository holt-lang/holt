//! Phase 2 codegen — LLVM via `inkwell` 0.10 (llvm21-1).
//! All locals/params are `alloca` in entry block; structs lowered to llvm.struct with GEP.

use std::collections::HashMap;
use std::path::Path;

use inkwell::IntPredicate;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicType, BasicTypeEnum, StructType};
use inkwell::values::{BasicValueEnum, FunctionValue, PointerValue};

use crate::ast::*;
use crate::token::Span;

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
    pub span: Span,
}

#[derive(Clone)]
struct LoopContext<'ctx> {
    cond_bb: inkwell::basic_block::BasicBlock<'ctx>,
    exit_bb: inkwell::basic_block::BasicBlock<'ctx>,
    label: Option<String>,
    defer_depth: usize,
}

pub struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    vars: Vec<HashMap<String, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>>,
    funcs: HashMap<String, (FunctionValue<'ctx>, TyInfo)>,
    struct_types: HashMap<String, StructType<'ctx>>,
    struct_fields: HashMap<String, HashMap<String, u32>>, // struct -> field -> index
    class_methods: HashMap<String, HashMap<String, (FunctionValue<'ctx>, TyInfo)>>,
    loop_stack: Vec<LoopContext<'ctx>>,
    defer_stack: Vec<Vec<DeferStmt>>,
    cur_fn: Option<FunctionValue<'ctx>>,
    cur_is_main: bool,
    cur_class: Option<String>,
}

#[derive(Clone, Debug)]
struct TyInfo {
    ret: crate::sema::Ty,
    params: Vec<crate::sema::Ty>,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self {
            context,
            module,
            builder,
            vars: Vec::new(),
            funcs: HashMap::new(),
            struct_types: HashMap::new(),
            struct_fields: HashMap::new(),
            class_methods: HashMap::new(),
            loop_stack: Vec::new(),
            defer_stack: Vec::new(),
            cur_fn: None,
            cur_is_main: false,
            cur_class: None,
        }
    }

    pub fn get_module_ir(&self) -> String {
        self.module.print_to_string().to_string()
    }

    pub fn compile_program(
        &mut self,
        prog: &Program,
    ) -> Result<(), CodegenError> {
        for item in &prog.items {
            match item {
                Item::Struct(s) => self.declare_struct(s)?,
                Item::Class(c) => self.declare_class(c)?,
                _ => {}
            }
        }
        for item in &prog.items {
            if let Item::Function(f) = item { self.declare_function(f)?; }
        }
        for item in &prog.items {
            match item {
                Item::Function(f) => self.codegen_function(f)?,
                Item::Class(c) => {
                    for m in &c.methods { self.codegen_class_method(c, m)?; }
                }
                _ => {}
            }
        }
        if let Err(e) = self.module.verify() {
            return Err(CodegenError {
                message: format!("LLVM verify failed: {e}"),
                span: prog.span,
            });
        }
        Ok(())
    }

    fn declare_struct(&mut self, s: &StructDecl) -> Result<(), CodegenError> {
        if self.struct_types.contains_key(&s.name) {
            return Err(CodegenError {
                message: format!("duplicate struct {}", s.name),
                span: s.name_span,
            });
        }
        let opaque = self.context.opaque_struct_type(&s.name);
        // Insert early to allow self-reference (not needed Phase 2) and duplicate check
        self.struct_types.insert(s.name.clone(), opaque);
        // Collect field LLVM types
        let mut field_map = HashMap::new();
        let mut field_tys: Vec<BasicTypeEnum<'ctx>> = Vec::new();
        for (idx, f) in s.fields.iter().enumerate() {
            let lty = self.llvm_ty_for(&f.ty);
            field_map.insert(f.name.clone(), idx as u32);
            field_tys.push(lty);
        }
        opaque.set_body(&field_tys, false);
        self.struct_fields.insert(s.name.clone(), field_map);
        Ok(())
    }

    fn declare_class(&mut self, c: &ClassDecl) -> Result<(), CodegenError> {
        if self.struct_types.contains_key(&c.name) {
            return Err(CodegenError{message: format!("duplicate class/struct `{}`", c.name), span: c.name_span});
        }
        let opaque = self.context.opaque_struct_type(&c.name);
        self.struct_types.insert(c.name.clone(), opaque);
        let mut field_map = HashMap::new();
        let mut field_tys = Vec::new();
        for (idx, f) in c.fields.iter().enumerate() {
            let lty = self.llvm_ty_for(&f.ty);
            field_map.insert(f.name.clone(), idx as u32);
            field_tys.push(lty);
        }
        opaque.set_body(&field_tys, false);
        self.struct_fields.insert(c.name.clone(), field_map);
        // Declare methods
        let mut methods = HashMap::new();
        for m in &c.methods {
            let ret_ty: crate::sema::Ty = (&m.ret_ty).into();
            let mut param_semas: Vec<crate::sema::Ty> = Vec::new();
            param_semas.push(crate::sema::Ty::Struct(c.name.clone()));
            for p in &m.params {
                param_semas.push((&p.ty).into());
            }
            let this_ty = self.context.ptr_type(inkwell::AddressSpace::default()).into();
            let mut param_llvm: Vec<inkwell::types::BasicMetadataTypeEnum> = vec![this_ty];
            for p in &m.params {
                let t: crate::sema::Ty = (&p.ty).into();
                if let Some(bt) = self.llvm_ty_for_sema(&t) { param_llvm.push(bt.into()); }
            }
            let fn_ty = match ret_ty {
                crate::sema::Ty::Void => self.context.void_type().fn_type(&param_llvm, false),
                crate::sema::Ty::Int => self.context.i64_type().fn_type(&param_llvm, false),
                crate::sema::Ty::Bool => self.context.bool_type().fn_type(&param_llvm, false),
                crate::sema::Ty::Char => self.context.i32_type().fn_type(&param_llvm, false),
                crate::sema::Ty::String => self.context.ptr_type(inkwell::AddressSpace::default()).fn_type(&param_llvm, false),
                crate::sema::Ty::Struct(ref n) => {
                    let st = self.struct_types.get(n).unwrap();
                    st.fn_type(&param_llvm, false)
                }
                crate::sema::Ty::Array(_) => self.context.i64_type().array_type(16).fn_type(&param_llvm, false),
                crate::sema::Ty::Pointer(_) => self.context.ptr_type(inkwell::AddressSpace::default()).fn_type(&param_llvm, false),
                crate::sema::Ty::Optional(ref el) => {
                    let inner = self.llvm_ty_for_sema(el).unwrap();
                    self.context.struct_type(&[inner.into(), self.context.bool_type().into()], false).fn_type(&param_llvm, false)
                }
            };
            let mangled = format!("{}__{}", c.name, m.name);
            let func = self.module.add_function(&mangled, fn_ty, None);
            let tyinfo = TyInfo{ret: ret_ty.clone(), params: param_semas.clone()};
            methods.insert(m.name.clone(), (func, tyinfo));
        }
        self.class_methods.insert(c.name.clone(), methods);
        Ok(())
    }

    fn llvm_ty_for(&self, ty: &Type) -> BasicTypeEnum<'ctx> {
        match ty {
            Type::Int(_) => self.context.i64_type().into(),
            Type::Bool(_) => self.context.bool_type().into(),
            Type::Char(_) => self.context.i32_type().into(),
            Type::String(_) => self
                .context
                .ptr_type(inkwell::AddressSpace::default())
                .into(),
            Type::Void(_) => {
                panic!("void not a first-class type in llvm_ty_for")
            }
            Type::Named(n, _) => {
                if let Some(st) = self.struct_types.get(n) {
                    st.as_basic_type_enum().into()
                } else {
                    panic!("unknown struct type {n}")
                }
            }
            Type::Array(_, _) => self.context.i64_type().array_type(16).into(),
            Type::Pointer(_, _) => self
                .context
                .ptr_type(inkwell::AddressSpace::default())
                .into(),
            Type::Optional(el, _) => {
                let inner = self.llvm_ty_for(el);
                self.context
                    .struct_type(
                        &[inner.into(), self.context.bool_type().into()],
                        false,
                    )
                    .into()
            }
        }
    }

    fn llvm_ty_for_sema(
        &self,
        ty: &crate::sema::Ty,
    ) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            crate::sema::Ty::Int => Some(self.context.i64_type().into()),
            crate::sema::Ty::Bool => Some(self.context.bool_type().into()),
            crate::sema::Ty::Char => Some(self.context.i32_type().into()),
            crate::sema::Ty::String => Some(
                self.context
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            ),
            crate::sema::Ty::Void => None,
            crate::sema::Ty::Struct(n) => {
                let st = self.struct_types.get(n).unwrap_or_else(|| {
                    panic!("unknown struct {n} in llvm_ty_for_sema")
                });
                Some(st.as_basic_type_enum().into())
            }
            crate::sema::Ty::Array(_) => {
                Some(self.context.i64_type().array_type(16).into())
            }
            crate::sema::Ty::Pointer(_) => Some(
                self.context
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            ),
            crate::sema::Ty::Optional(el) => {
                let inner = self.llvm_ty_for_sema(el).unwrap();
                Some(
                    self.context
                        .struct_type(
                            &[inner.into(), self.context.bool_type().into()],
                            false,
                        )
                        .into(),
                )
            }
        }
    }

    fn declare_function(&mut self, f: &Function) -> Result<(), CodegenError> {
        let ret_sema: crate::sema::Ty = (&f.ret_ty).into();
        let param_semas: Vec<crate::sema::Ty> =
            f.params.iter().map(|p| (&p.ty).into()).collect();

        let param_types: Vec<inkwell::types::BasicMetadataTypeEnum> =
            param_semas
                .iter()
                .filter_map(|t| self.llvm_ty_for_sema(t).map(|bt| bt.into()))
                .collect();

        // Special ABI for `main`: C `int main()` is always i32
        let fn_ty = if f.name == "main" {
            self.context.i32_type().fn_type(&param_types, false)
        } else {
            match ret_sema {
                crate::sema::Ty::Void => {
                    self.context.void_type().fn_type(&param_types, false)
                }
                crate::sema::Ty::Int => {
                    self.context.i64_type().fn_type(&param_types, false)
                }
                crate::sema::Ty::Bool => {
                    self.context.bool_type().fn_type(&param_types, false)
                }
                crate::sema::Ty::Char => {
                    self.context.i32_type().fn_type(&param_types, false)
                }
                crate::sema::Ty::String => self
                    .context
                    .ptr_type(inkwell::AddressSpace::default())
                    .fn_type(&param_types, false),
                crate::sema::Ty::Struct(ref n) => {
                    let st = self.struct_types.get(n).ok_or(CodegenError {
                        message: format!("unknown struct {n}"),
                        span: f.ret_ty.span(),
                    })?;
                    st.fn_type(&param_types, false)
                }
                crate::sema::Ty::Array(ref el) => {
                    // arrays as fixed [16 x elem] return — rarely used but support
                    let elem_ty = self.llvm_ty_for_sema(el).unwrap();
                    // For array element i64, array type is [16 x i64]
                    let arr_ty = self.context.i64_type().array_type(16);
                    arr_ty.fn_type(&param_types, false)
                }
                crate::sema::Ty::Pointer(_) => self
                    .context
                    .ptr_type(inkwell::AddressSpace::default())
                    .fn_type(&param_types, false),
                crate::sema::Ty::Optional(ref el) => {
                    let inner = self.llvm_ty_for_sema(el).unwrap();
                    self.context
                        .struct_type(
                            &[inner.into(), self.context.bool_type().into()],
                            false,
                        )
                        .fn_type(&param_types, false)
                }
            }
        };

        let func = self.module.add_function(&f.name, fn_ty, None);
        self.funcs.insert(
            f.name.clone(),
            (
                func,
                TyInfo {
                    ret: ret_sema,
                    params: param_semas,
                },
            ),
        );
        Ok(())
    }

    fn codegen_function(&mut self, f: &Function) -> Result<(), CodegenError> {
        let (func, info) =
            self.funcs.get(&f.name).cloned().ok_or(CodegenError {
                message: format!("undeclared func {}", f.name),
                span: f.name_span,
            })?;
        self.cur_fn = Some(func);
        self.cur_is_main = f.name == "main";
        let entry = self.context.append_basic_block(func, "entry");
        self.builder.position_at_end(entry);

        self.vars.push(HashMap::new());
        for (i, param) in f.params.iter().enumerate() {
            let llvm_ty = self.llvm_ty_for(&param.ty);
            let alloca = self.create_entry_block_alloca(&param.name, llvm_ty);
            let param_val = func.get_nth_param(i as u32).unwrap();
            self.builder.build_store(alloca, param_val).unwrap();
            self.vars
                .last_mut()
                .unwrap()
                .insert(param.name.clone(), (alloca, llvm_ty));
        }

        let always_returns = self.codegen_block(&f.body)?;

        if !always_returns
            && self
                .builder
                .get_insert_block()
                .unwrap()
                .get_terminator()
                .is_none()
        {
            if self.cur_is_main {
                let zero = self.context.i32_type().const_int(0, false);
                self.builder.build_return(Some(&zero)).unwrap();
            } else if info.ret == crate::sema::Ty::Void {
                self.builder.build_return(None).unwrap();
            } else {
                let zero: BasicValueEnum = match info.ret {
                    crate::sema::Ty::Int => {
                        self.context.i64_type().const_int(0, false).into()
                    }
                    crate::sema::Ty::Bool => {
                        self.context.bool_type().const_int(0, false).into()
                    }
                    crate::sema::Ty::Char => {
                        self.context.i32_type().const_int(0, false).into()
                    }
                    crate::sema::Ty::String => self
                        .context
                        .ptr_type(inkwell::AddressSpace::default())
                        .const_null()
                        .into(),
                    crate::sema::Ty::Struct(ref n) => {
                        self.struct_types.get(n).unwrap().const_zero().into()
                    }
                    crate::sema::Ty::Array(_) => self
                        .context
                        .i64_type()
                        .array_type(16)
                        .const_zero()
                        .into(),
                    crate::sema::Ty::Pointer(_) => self
                        .context
                        .ptr_type(inkwell::AddressSpace::default())
                        .const_null()
                        .into(),
                    crate::sema::Ty::Optional(ref el) => {
                        let inner = self.llvm_ty_for_sema(el).unwrap();
                        self.context
                            .struct_type(
                                &[
                                    inner.into(),
                                    self.context.bool_type().into(),
                                ],
                                false,
                            )
                            .const_zero()
                            .into()
                    }
                    crate::sema::Ty::Void => unreachable!(),
                };
                self.builder.build_return(Some(&zero)).unwrap();
            }
        }

        self.vars.pop();
        self.cur_fn = None;
        self.cur_is_main = false;
        if !func.verify(true) {
            return Err(CodegenError {
                message: format!("function {} failed verification", f.name),
                span: f.span,
            });
        }
        Ok(())
    }

    fn codegen_class_method(&mut self, class: &ClassDecl, method: &Function) -> Result<(), CodegenError> {
        let methods = self.class_methods.get(&class.name).ok_or(CodegenError{message: format!("unknown class {}", class.name), span: class.name_span})?;
        let (func, info) = methods.get(&method.name).cloned().ok_or(CodegenError{message: format!("unknown method {}", method.name), span: method.name_span})?;
        self.cur_fn = Some(func);
        self.cur_class = Some(class.name.clone());
        self.cur_is_main = false;
        let entry = self.context.append_basic_block(func, "entry");
        self.builder.position_at_end(entry);
        self.vars.push(HashMap::new());
        let this_ty: BasicTypeEnum<'ctx> = self.context.ptr_type(inkwell::AddressSpace::default()).into();
        let this_param = func.get_nth_param(0).unwrap();
        let this_alloca = self.create_entry_block_alloca("this", this_ty);
        self.builder.build_store(this_alloca, this_param).unwrap();
        self.vars.last_mut().unwrap().insert("this".to_string(), (this_alloca, this_ty));
        for (i, param) in method.params.iter().enumerate() {
            let llvm_ty = self.llvm_ty_for(&param.ty);
            let alloca = self.create_entry_block_alloca(&param.name, llvm_ty);
            let param_val = func.get_nth_param((i+1) as u32).unwrap();
            self.builder.build_store(alloca, param_val).unwrap();
            self.vars.last_mut().unwrap().insert(param.name.clone(), (alloca, llvm_ty));
        }
        let always_returns = self.codegen_block(&method.body)?;
        if !always_returns && self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            if info.ret == crate::sema::Ty::Void {
                self.builder.build_return(None).unwrap();
            } else {
                let zero: BasicValueEnum = match info.ret {
                    crate::sema::Ty::Int => self.context.i64_type().const_int(0, false).into(),
                    crate::sema::Ty::Bool => self.context.bool_type().const_int(0, false).into(),
                    crate::sema::Ty::Char => self.context.i32_type().const_int(0, false).into(),
                    crate::sema::Ty::String => self.context.ptr_type(inkwell::AddressSpace::default()).const_null().into(),
                    crate::sema::Ty::Struct(ref n) => self.struct_types.get(n).unwrap().const_zero().into(),
                    crate::sema::Ty::Array(_) => self.context.i64_type().array_type(16).const_zero().into(),
                    crate::sema::Ty::Pointer(_) => self.context.ptr_type(inkwell::AddressSpace::default()).const_null().into(),
                    crate::sema::Ty::Optional(ref el) => {
                        let inner = self.llvm_ty_for_sema(el).unwrap();
                        self.context.struct_type(&[inner.into(), self.context.bool_type().into()], false).const_zero().into()
                    }
                    crate::sema::Ty::Void => unreachable!(),
                };
                self.builder.build_return(Some(&zero)).unwrap();
            }
        }
        self.vars.pop();
        self.cur_fn = None;
        self.cur_class = None;
        if !func.verify(true) {
            return Err(CodegenError{message: format!("method {}::{} failed verification", class.name, method.name), span: method.span});
        }
        Ok(())
    }

    fn create_entry_block_alloca(
        &self,
        name: &str,
        ty: BasicTypeEnum<'ctx>,
    ) -> PointerValue<'ctx> {
        let func = self.cur_fn.unwrap();
        let entry = func.get_first_basic_block().unwrap();
        let builder = self.context.create_builder();
        if let Some(first) = entry.get_first_instruction() {
            builder.position_before(&first);
        } else {
            builder.position_at_end(entry);
        }
        match ty {
            BasicTypeEnum::IntType(t) => builder.build_alloca(t, name).unwrap(),
            BasicTypeEnum::FloatType(t) => {
                builder.build_alloca(t, name).unwrap()
            }
            BasicTypeEnum::PointerType(t) => {
                builder.build_alloca(t, name).unwrap()
            }
            BasicTypeEnum::ArrayType(t) => {
                builder.build_alloca(t, name).unwrap()
            }
            BasicTypeEnum::StructType(t) => {
                builder.build_alloca(t, name).unwrap()
            }
            BasicTypeEnum::VectorType(t) => {
                builder.build_alloca(t, name).unwrap()
            }
            BasicTypeEnum::ScalableVectorType(t) => {
                builder.build_alloca(t, name).unwrap()
            }
        }
    }

    fn emit_current_scope_defers(&mut self) -> Result<(), CodegenError> {
        if let Some(idx) = self.defer_stack.len().checked_sub(1) {
            let defers = self.defer_stack[idx].clone();
            for defer in defers.iter().rev() {
                match &defer.inner {
                    DeferInner::Expr(e) => { let _ = self.codegen_expr(e)?; }
                    DeferInner::Block(b) => { let _ = self.codegen_block(b)?; }
                }
                if self.builder.get_insert_block().unwrap().get_terminator().is_some() { break; }
            }
        }
        Ok(())
    }
    fn emit_all_defers(&mut self) -> Result<(), CodegenError> {
        for idx in (0..self.defer_stack.len()).rev() {
            let defers = self.defer_stack[idx].clone();
            for defer in defers.iter().rev() {
                match &defer.inner {
                    DeferInner::Expr(e) => { let _ = self.codegen_expr(e)?; }
                    DeferInner::Block(b) => { let _ = self.codegen_block(b)?; }
                }
                if self.builder.get_insert_block().unwrap().get_terminator().is_some() { break; }
            }
        }
        Ok(())
    }
    fn emit_defers_up_to(&mut self, target_depth: usize) -> Result<(), CodegenError> {
        for idx in (target_depth..self.defer_stack.len()).rev() {
            let defers = self.defer_stack[idx].clone();
            for defer in defers.iter().rev() {
                match &defer.inner {
                    DeferInner::Expr(e) => { let _ = self.codegen_expr(e)?; }
                    DeferInner::Block(b) => { let _ = self.codegen_block(b)?; }
                }
                if self.builder.get_insert_block().unwrap().get_terminator().is_some() { break; }
            }
        }
        Ok(())
    }

    fn codegen_block(&mut self, block: &Block) -> Result<bool, CodegenError> {
        self.vars.push(HashMap::new());
        self.defer_stack.push(Vec::new());
        let mut always_returns = false;
        for stmt in &block.stmts {
            if self
                .builder
                .get_insert_block()
                .unwrap()
                .get_terminator()
                .is_some()
            {
                let dead = self
                    .context
                    .append_basic_block(self.cur_fn.unwrap(), "dead");
                self.builder.position_at_end(dead);
            }
            let stmt_returns = self.codegen_stmt(stmt)?;
            if stmt_returns {
                always_returns = true;
            }
        }
        // Emit defers for this block on normal exit
        if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            self.emit_current_scope_defers()?;
        } else {
            // already terminated, just clear any remaining defers for this scope (they were emitted via return/break)
            if let Some(v) = self.defer_stack.last_mut() { v.clear(); }
        }
        self.defer_stack.pop();
        self.vars.pop();
        Ok(always_returns)
    }

    fn codegen_stmt(&mut self, stmt: &Stmt) -> Result<bool, CodegenError> {
        match stmt {
            Stmt::VarDecl(d) => {
                let ty = self.llvm_ty_for(&d.ty);
                let alloca = self.create_entry_block_alloca(&d.name, ty);
                self.vars
                    .last_mut()
                    .unwrap()
                    .insert(d.name.clone(), (alloca, ty));
                if let Some(init) = &d.init {
                    let val = self.codegen_expr(init)?;
                    self.builder.build_store(alloca, val).unwrap();
                } else {
                    // zero init for all types
                    let zero: BasicValueEnum = match &d.ty {
                        Type::Int(_) => {
                            self.context.i64_type().const_int(0, false).into()
                        }
                        Type::Bool(_) => {
                            self.context.bool_type().const_int(0, false).into()
                        }
                        Type::Char(_) => {
                            self.context.i32_type().const_int(0, false).into()
                        }
                        Type::String(_) => self
                            .context
                            .ptr_type(inkwell::AddressSpace::default())
                            .const_null()
                            .into(),
                        Type::Void(_) => unreachable!(),
                        Type::Named(n, _) => {
                            let st = self.struct_types.get(n).unwrap();
                            // zeroed struct with undef then insert zeros? Use const zero if possible
                            st.const_zero().into()
                        }
                        Type::Array(_, _) => self
                            .context
                            .i64_type()
                            .array_type(16)
                            .const_zero()
                            .into(),
                        Type::Pointer(_, _) => self
                            .context
                            .ptr_type(inkwell::AddressSpace::default())
                            .const_null()
                            .into(),
                        Type::Optional(el, _) => {
                            let inner_zero: BasicValueEnum = match el.as_ref() {
                                Type::Int(_) => self
                                    .context
                                    .i64_type()
                                    .const_int(0, false)
                                    .into(),
                                _ => self
                                    .context
                                    .i64_type()
                                    .const_int(0, false)
                                    .into(),
                            };
                            // optional as {inner, false}
                            let inner_ty = self.llvm_ty_for(el);
                            let opt_ty = self.context.struct_type(
                                &[
                                    inner_ty.into(),
                                    self.context.bool_type().into(),
                                ],
                                false,
                            );
                            opt_ty
                                .const_named_struct(&[
                                    inner_zero.into(),
                                    self.context
                                        .bool_type()
                                        .const_int(0, false)
                                        .into(),
                                ])
                                .into()
                        }
                    };
                    self.builder.build_store(alloca, zero).unwrap();
                }
                Ok(false)
            }
            Stmt::Expr(e) => {
                let _ = self.codegen_expr(&e.expr)?;
                Ok(false)
            }
            Stmt::Block(b) => self.codegen_block(b),
            Stmt::Return(r) => {
                self.emit_all_defers()?;
                if self.cur_is_main {
                    if let Some(expr) = &r.value {
                        let val = self.codegen_expr(expr)?;
                        let ret_val = if val.is_int_value()
                            && val.into_int_value().get_type().get_bit_width()
                                == 64
                        {
                            self.builder
                                .build_int_truncate(
                                    val.into_int_value(),
                                    self.context.i32_type(),
                                    "main.trunc",
                                )
                                .unwrap()
                                .into()
                        } else if val.is_int_value()
                            && val.into_int_value().get_type().get_bit_width()
                                == 1
                        {
                            self.builder
                                .build_int_z_extend(
                                    val.into_int_value(),
                                    self.context.i32_type(),
                                    "main.zext",
                                )
                                .unwrap()
                                .into()
                        } else if val.is_struct_value() {
                            // struct main? not supported, return 0
                            self.context.i32_type().const_int(0, false).into()
                        } else {
                            val
                        };
                        self.builder.build_return(Some(&ret_val)).unwrap();
                    } else {
                        let zero = self.context.i32_type().const_int(0, false);
                        self.builder.build_return(Some(&zero)).unwrap();
                    }
                } else if let Some(expr) = &r.value {
                    let val = self.codegen_expr(expr)?;
                    self.builder.build_return(Some(&val)).unwrap();
                } else {
                    self.builder.build_return(None).unwrap();
                }
                Ok(true)
            }
            Stmt::If(s) => {
                let cond = self.codegen_expr(&s.cond)?;
                let cond_bool = cond.into_int_value();
                let func = self.cur_fn.unwrap();
                let then_bb = self.context.append_basic_block(func, "if.then");
                let else_bb = if s.else_block.is_some() {
                    Some(self.context.append_basic_block(func, "if.else"))
                } else {
                    None
                };
                let merge_bb =
                    self.context.append_basic_block(func, "if.merge");
                if let Some(else_bb) = else_bb {
                    self.builder
                        .build_conditional_branch(cond_bool, then_bb, else_bb)
                        .unwrap();
                } else {
                    self.builder
                        .build_conditional_branch(cond_bool, then_bb, merge_bb)
                        .unwrap();
                }
                self.builder.position_at_end(then_bb);
                let then_ret = self.codegen_block(&s.then_block)?;
                if self
                    .builder
                    .get_insert_block()
                    .unwrap()
                    .get_terminator()
                    .is_none()
                {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }
                let else_ret = if let (Some(else_bb), Some(else_block)) =
                    (else_bb, &s.else_block)
                {
                    self.builder.position_at_end(else_bb);
                    let r = self.codegen_block(else_block)?;
                    if self
                        .builder
                        .get_insert_block()
                        .unwrap()
                        .get_terminator()
                        .is_none()
                    {
                        self.builder
                            .build_unconditional_branch(merge_bb)
                            .unwrap();
                    }
                    r
                } else {
                    false
                };
                self.builder.position_at_end(merge_bb);
                Ok(then_ret && else_ret)
            }
            Stmt::While(s) => {
                let func = self.cur_fn.unwrap();
                let cond_bb = self.context.append_basic_block(func, "while.cond");
                let body_bb = self.context.append_basic_block(func, "while.body");
                let exit_bb = self.context.append_basic_block(func, "while.exit");
                self.builder.build_unconditional_branch(cond_bb).unwrap();
                self.builder.position_at_end(cond_bb);
                let cond = self.codegen_expr(&s.cond)?;
                let cond_bool = cond.into_int_value();
                self.builder.build_conditional_branch(cond_bool, body_bb, exit_bb).unwrap();
                self.loop_stack.push(LoopContext{cond_bb, exit_bb, label: None, defer_depth: self.defer_stack.len()});
                self.builder.position_at_end(body_bb);
                let _ = self.codegen_block(&s.body)?;
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(cond_bb).unwrap();
                }
                self.loop_stack.pop();
                self.builder.position_at_end(exit_bb);
                Ok(false)
            }
            Stmt::Loop(l) => {
                let func = self.cur_fn.unwrap();
                let header_bb = self.context.append_basic_block(func, "loop.header");
                let body_bb = self.context.append_basic_block(func, "loop.body");
                let exit_bb = self.context.append_basic_block(func, "loop.exit");
                self.builder.build_unconditional_branch(header_bb).unwrap();
                self.builder.position_at_end(header_bb);
                self.builder.build_unconditional_branch(body_bb).unwrap();
                self.loop_stack.push(LoopContext{cond_bb: header_bb, exit_bb, label: l.label.clone(), defer_depth: self.defer_stack.len()});
                self.builder.position_at_end(body_bb);
                let _ = self.codegen_block(&l.body)?;
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(header_bb).unwrap();
                }
                self.loop_stack.pop();
                self.builder.position_at_end(exit_bb);
                Ok(false)
            }
            Stmt::For(f) => {
                // Desugar for var in iter do body => index loop over array
                // iter must be array (int[]); element type is int
                let func = self.cur_fn.unwrap();
                let cond_bb = self.context.append_basic_block(func, "for.cond");
                let body_bb = self.context.append_basic_block(func, "for.body");
                let inc_bb = self.context.append_basic_block(func, "for.inc");
                let exit_bb = self.context.append_basic_block(func, "for.exit");
                // Allocate index var __for_idx_<var>
                let idx_name = format!("__for_idx_{}", f.var);
                let idx_ty = self.context.i64_type().as_basic_type_enum();
                let idx_ptr = self.create_entry_block_alloca(&idx_name, idx_ty);
                self.builder.build_store(idx_ptr, self.context.i64_type().const_int(0, false)).unwrap();
                // Determine array to iterate: for now require iter is Ident array variable
                // We will evaluate iter expression? For simplicity we require iter is Ident of int[] variable, and we use its array length 16 constant
                // Create initial branch to cond
                self.builder.build_unconditional_branch(cond_bb).unwrap();
                self.builder.position_at_end(cond_bb);
                let idx_val = self.builder.build_load(idx_ty, idx_ptr, "for.idx.load").unwrap().into_int_value();
                let limit = self.context.i64_type().const_int(16, false);
                let cond = self.builder.build_int_compare(IntPredicate::SLT, idx_val, limit, "for.cond").unwrap();
                self.builder.build_conditional_branch(cond, body_bb, exit_bb).unwrap();
                self.loop_stack.push(LoopContext{cond_bb: inc_bb, exit_bb, label: f.label.clone(), defer_depth: self.defer_stack.len()});
                self.builder.position_at_end(body_bb);
                // Load element: arr[idx]
                // Resolve array var from iter: expect Ident
                let iter_val_opt: Option<(PointerValue<'ctx>, BasicTypeEnum<'ctx>)> = if let ExprKind::Ident(ref arr_name) = f.iter.kind {
                    self.lookup_var(arr_name)
                } else { None };
                // Create loop scope for var
                self.vars.push(HashMap::new());
                self.defer_stack.push(Vec::new());
                // Declare for var in this scope
                // If iter is array, element type is int
                let elem_val: Option<BasicValueEnum<'ctx>> = if let Some((arr_ptr, arr_ty)) = iter_val_opt {
                    if arr_ty.is_array_type() {
                        let arr_ty_a = arr_ty.into_array_type();
                        let elem_ptr = unsafe { self.builder.build_gep(arr_ty_a, arr_ptr, &[self.context.i64_type().const_int(0,false), idx_val], "for.elem.ptr").unwrap() };
                        Some(self.builder.build_load(self.context.i64_type(), elem_ptr, "for.elem").unwrap())
                    } else if arr_ty.is_pointer_type() {
                        let loaded_arr = self.builder.build_load(arr_ty, arr_ptr, "ptr.load").unwrap().into_pointer_value();
                        let elem_ptr = unsafe { self.builder.build_gep(self.context.i64_type(), loaded_arr, &[idx_val], "for.ptr.elem").unwrap() };
                        Some(self.builder.build_load(self.context.i64_type(), elem_ptr, "for.elem").unwrap())
                    } else { None }
                } else {
                    // For non-ident iter (e.g., string), try to codegen iter as pointer? For now fallback to 0
                    None
                };
                if let Some(v) = elem_val {
                    let elem_ty = v.get_type();
                    let var_ptr = self.create_entry_block_alloca(&f.var, elem_ty);
                    self.builder.build_store(var_ptr, v).unwrap();
                    self.vars.last_mut().unwrap().insert(f.var.clone(), (var_ptr, elem_ty));
                } else {
                    // fallback: declare var as int 0 if we couldn't resolve
                    let var_ptr = self.create_entry_block_alloca(&f.var, self.context.i64_type().into());
                    self.builder.build_store(var_ptr, self.context.i64_type().const_int(0,false)).unwrap();
                    self.vars.last_mut().unwrap().insert(f.var.clone(), (var_ptr, self.context.i64_type().into()));
                }
                let _ = self.codegen_block(&f.body)?;
                // after body, branch to inc
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    // emit defer for for-body scope before inc? The defer inside for body should run before inc
                    // Our codegen_block for body already emitted its defers on normal exit via its own defer handling
                    // But we still have outer for-var scope defers to emit before inc
                    // For simplicity, just branch to inc; inc will handle idx increment
                }
                // Pop for-var scope defer/var (but keep defer for next iteration? The for-var scope is per-iteration; we need to pop after body)
                // Actually for-var scope should be per iteration, but we pushed it before body; after body we should pop and emit its defers
                // Emit defers for for-var scope
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.emit_current_scope_defers().unwrap();
                }
                self.defer_stack.pop();
                self.vars.pop();
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(inc_bb).unwrap();
                }
                self.builder.position_at_end(inc_bb);
                let cur_idx = self.builder.build_load(idx_ty, idx_ptr, "for.idx").unwrap().into_int_value();
                let inc = self.builder.build_int_add(cur_idx, self.context.i64_type().const_int(1,false), "for.inc").unwrap();
                self.builder.build_store(idx_ptr, inc).unwrap();
                self.builder.build_unconditional_branch(cond_bb).unwrap();
                self.loop_stack.pop();
                self.builder.position_at_end(exit_bb);
                Ok(false)
            }
            Stmt::Defer(d) => {
                // Push onto current defer scope (innermost block)
                if let Some(top) = self.defer_stack.last_mut() {
                    top.push(d.clone());
                } else {
                    // No active block defer stack (should not happen, fallback to global)
                    self.defer_stack.push(vec![d.clone()]);
                }
                Ok(false)
            }
            Stmt::Break(b) => {
                let target_idx = if let Some(label) = &b.label {
                    self.loop_stack.iter().rposition(|lc| lc.label.as_ref() == Some(label))
                        .ok_or(CodegenError{message: format!("break label `{label}` not found"), span: b.span})?
                } else {
                    self.loop_stack.len().checked_sub(1).ok_or(CodegenError{message: "break outside loop".into(), span: b.span})?
                };
                let ctx = self.loop_stack[target_idx].clone();
                self.emit_defers_up_to(ctx.defer_depth)?;
                self.builder.build_unconditional_branch(ctx.exit_bb).unwrap();
                Ok(false)
            }
            Stmt::Continue(c) => {
                let target_idx = if let Some(label) = &c.label {
                    self.loop_stack.iter().rposition(|lc| lc.label.as_ref() == Some(label))
                        .ok_or(CodegenError{message: format!("continue label `{label}` not found"), span: c.span})?
                } else {
                    self.loop_stack.len().checked_sub(1).ok_or(CodegenError{message: "continue outside loop".into(), span: c.span})?
                };
                let ctx = self.loop_stack[target_idx].clone();
                self.emit_defers_up_to(ctx.defer_depth)?;
                self.builder.build_unconditional_branch(ctx.cond_bb).unwrap();
                Ok(false)
            }
        }
    }

    fn codegen_expr(
        &mut self,
        expr: &Expr,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        match &expr.kind {
            ExprKind::IntLit(v) => {
                Ok(self.context.i64_type().const_int(*v as u64, true).into())
            }
            ExprKind::BoolLit(b) => Ok(self
                .context
                .bool_type()
                .const_int(if *b { 1 } else { 0 }, false)
                .into()),
            ExprKind::Ident(name) => {
                let (ptr, ty) = self.lookup_var(name).ok_or(CodegenError {
                    message: format!("undefined var {name}"),
                    span: expr.span,
                })?;
                Ok(self.builder.build_load(ty, ptr, name).unwrap())
            }
            ExprKind::This => {
                let (ptr, ty) = self.lookup_var("this").ok_or(CodegenError{message: "`this` outside method".into(), span: expr.span})?;
                Ok(self.builder.build_load(ty, ptr, "this").unwrap())
            }
            ExprKind::MethodCall{object, method, method_span: _, args} => {
                // Determine this pointer for method call
                let this_ptr: PointerValue<'ctx> = match &object.kind {
                    ExprKind::Ident(name) => {
                        if let Some((ptr, ty)) = self.lookup_var(name) {
                            if ty.is_struct_type() {
                                // p is struct value instance, its alloca is the instance pointer
                                ptr
                            } else if ty.is_pointer_type() {
                                self.builder.build_load(ty, ptr, "this.load").unwrap().into_pointer_value()
                            } else {
                                return Err(CodegenError{message: format!("method call on non-class variable `{name}`"), span: expr.span});
                            }
                        } else { return Err(CodegenError{message: format!("undefined var {name}"), span: expr.span}); }
                    }
                    ExprKind::This => {
                        let (ptr, ty) = self.lookup_var("this").ok_or(CodegenError{message: "`this` outside method".into(), span: expr.span})?;
                        self.builder.build_load(ty, ptr, "this.load").unwrap().into_pointer_value()
                    }
                    ExprKind::MemberAccess{object: inner, field, ..} => {
                        // a.b.method() where a.b is struct field that is class instance
                        let field_ptr = self.codegen_field_ptr(inner, field)?;
                        field_ptr
                    }
                    _ => {
                        // Fallback: try codegen object as value and allocate temp? For now error
                        return Err(CodegenError{message: "method call object must be variable or field access".into(), span: expr.span});
                    }
                };
                let obj_ty = self.infer_expr_ty(object)?;
                let cls_name = match obj_ty {
                    crate::sema::Ty::Struct(ref n) => n.clone(),
                    _ => return Err(CodegenError{message: format!("method call on non-class"), span: expr.span}),
                };
                let methods = self.class_methods.get(&cls_name).ok_or(CodegenError{message: format!("unknown class {cls_name}"), span: expr.span})?;
                let (func, _info) = methods.get(method).cloned().ok_or(CodegenError{message: format!("unknown method {method} for class {cls_name}"), span: expr.span})?;
                let mut arg_vals: Vec<inkwell::values::BasicMetadataValueEnum> = vec![this_ptr.into()];
                for a in args {
                    let v = self.codegen_expr(a)?;
                    arg_vals.push(v.into());
                }
                let call = self.builder.build_call(func, &arg_vals, "call").unwrap();
                let vk = call.try_as_basic_value();
                if vk.is_basic() { Ok(vk.basic().unwrap()) } else { Ok(self.context.i64_type().const_int(0,false).into()) }
            }
            ExprKind::Paren(inner) => self.codegen_expr(inner),
            ExprKind::Unary { op, expr: inner } => {
                let v = self.codegen_expr(inner)?;
                match op {
                    UnaryOp::Not => {
                        let b = v.into_int_value();
                        Ok(self
                            .builder
                            .build_xor(
                                b,
                                self.context.bool_type().const_int(1, false),
                                "not",
                            )
                            .unwrap()
                            .into())
                    }
                    UnaryOp::Neg => {
                        let i = v.into_int_value();
                        Ok(self
                            .builder
                            .build_int_sub(
                                self.context.i64_type().const_int(0, false),
                                i,
                                "neg",
                            )
                            .unwrap()
                            .into())
                    }
                    UnaryOp::Pos => Ok(v),
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.codegen_expr(lhs)?;
                let r = self.codegen_expr(rhs)?;
                Ok(match op {
                    BinOp::Add => self
                        .builder
                        .build_int_add(
                            l.into_int_value(),
                            r.into_int_value(),
                            "add",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Sub => self
                        .builder
                        .build_int_sub(
                            l.into_int_value(),
                            r.into_int_value(),
                            "sub",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Mul => self
                        .builder
                        .build_int_mul(
                            l.into_int_value(),
                            r.into_int_value(),
                            "mul",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Div => self
                        .builder
                        .build_int_signed_div(
                            l.into_int_value(),
                            r.into_int_value(),
                            "div",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Mod => self
                        .builder
                        .build_int_signed_rem(
                            l.into_int_value(),
                            r.into_int_value(),
                            "mod",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Lt => self
                        .builder
                        .build_int_compare(
                            IntPredicate::SLT,
                            l.into_int_value(),
                            r.into_int_value(),
                            "lt",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Le => self
                        .builder
                        .build_int_compare(
                            IntPredicate::SLE,
                            l.into_int_value(),
                            r.into_int_value(),
                            "le",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Gt => self
                        .builder
                        .build_int_compare(
                            IntPredicate::SGT,
                            l.into_int_value(),
                            r.into_int_value(),
                            "gt",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Ge => self
                        .builder
                        .build_int_compare(
                            IntPredicate::SGE,
                            l.into_int_value(),
                            r.into_int_value(),
                            "ge",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Is => self
                        .builder
                        .build_int_compare(
                            IntPredicate::EQ,
                            l.into_int_value(),
                            r.into_int_value(),
                            "is",
                        )
                        .unwrap()
                        .into(),
                    BinOp::IsNot => self
                        .builder
                        .build_int_compare(
                            IntPredicate::NE,
                            l.into_int_value(),
                            r.into_int_value(),
                            "isnot",
                        )
                        .unwrap()
                        .into(),
                    BinOp::And => self
                        .builder
                        .build_and(
                            l.into_int_value(),
                            r.into_int_value(),
                            "and",
                        )
                        .unwrap()
                        .into(),
                    BinOp::Or => self
                        .builder
                        .build_or(l.into_int_value(), r.into_int_value(), "or")
                        .unwrap()
                        .into(),
                })
            }
            ExprKind::Assign { lhs, value } => {
                let val = self.codegen_expr(value)?;
                match &lhs.kind {
                    ExprKind::Ident(name) => {
                        let (ptr, _) =
                            self.lookup_var(name).ok_or(CodegenError {
                                message: format!("undefined var {name}"),
                                span: lhs.span,
                            })?;
                        self.builder.build_store(ptr, val).unwrap();
                        Ok(val)
                    }
                    ExprKind::MemberAccess { object, field, .. } => {
                        let field_ptr =
                            self.codegen_field_ptr(object, field)?;
                        self.builder.build_store(field_ptr, val).unwrap();
                        Ok(val)
                    }
                    ExprKind::Index { object, index } => {
                        // arr[idx] = val  -> GEP store
                        let idx_val =
                            self.codegen_expr(index)?.into_int_value();
                        // Handle Ident array
                        if let ExprKind::Ident(name) = &object.kind {
                            if let Some((ptr, ty)) = self.lookup_var(name) {
                                if ty.is_array_type() {
                                    let arr_ty = ty.into_array_type();
                                    let elem_ptr = unsafe {
                                        self.builder
                                            .build_gep(
                                                arr_ty,
                                                ptr,
                                                &[
                                                    self.context
                                                        .i64_type()
                                                        .const_int(0, false),
                                                    idx_val,
                                                ],
                                                "idx.store",
                                            )
                                            .unwrap()
                                    };
                                    self.builder
                                        .build_store(elem_ptr, val)
                                        .unwrap();
                                    return Ok(val);
                                } else if ty.is_pointer_type() {
                                    let loaded = self
                                        .builder
                                        .build_load(ty, ptr, "ptr.load")
                                        .unwrap()
                                        .into_pointer_value();
                                    let elem_ptr = unsafe {
                                        self.builder
                                            .build_gep(
                                                self.context.i64_type(),
                                                loaded,
                                                &[idx_val],
                                                "idx.ptr.store",
                                            )
                                            .unwrap()
                                    };
                                    self.builder
                                        .build_store(elem_ptr, val)
                                        .unwrap();
                                    return Ok(val);
                                }
                            }
                        }
                        return Err(CodegenError{message: "unsupported indexing assignment base; only direct array variable indexing supported".into(), span: lhs.span});
                    }
                    _ => Err(CodegenError {
                        message: "invalid assignment target".into(),
                        span: lhs.span,
                    }),
                }
            }
            ExprKind::Call {
                callee,
                callee_span: _,
                args,
            } => {
                let (func, _info) =
                    self.funcs.get(callee).cloned().ok_or(CodegenError {
                        message: format!("undefined function {callee}"),
                        span: expr.span,
                    })?;
                let mut arg_vals: Vec<inkwell::values::BasicMetadataValueEnum> =
                    Vec::new();
                for a in args {
                    let v = self.codegen_expr(a)?;
                    arg_vals.push(v.into());
                }
                let call =
                    self.builder.build_call(func, &arg_vals, "call").unwrap();
                let vk = call.try_as_basic_value();
                if vk.is_basic() {
                    Ok(vk.basic().unwrap())
                } else {
                    Ok(self.context.i64_type().const_int(0, false).into())
                }
            }
            ExprKind::MemberAccess {
                object,
                field,
                field_span: _,
            } => {
                // rvalue field load: need field pointer then load
                let field_ptr = self.codegen_field_ptr(object, field)?;
                let obj_ty = self.infer_expr_ty(object)?;
                if let crate::sema::Ty::Struct(ref sname) = obj_ty {
                    let fields = self.struct_fields.get(sname).unwrap();
                    let idx = *fields.get(field).unwrap();
                    let st = self.struct_types.get(sname).unwrap();
                    let field_ty = st.get_field_type_at_index(idx).unwrap();
                    Ok(self
                        .builder
                        .build_load(field_ty, field_ptr, field)
                        .unwrap())
                } else {
                    Err(CodegenError {
                        message: format!("field access on non-struct"),
                        span: expr.span,
                    })
                }
            }
            ExprKind::Index { object, index } => {
                // a[i] rvalue: GEP on array or string
                let idx_val = self.codegen_expr(index)?.into_int_value();
                // Determine object type: if it's Ident array var, it's [16 x i64] alloca
                // For simplicity, handle Ident array and member access array (e.g., s.arr[i]) via GEP
                // Try lookup as variable first
                if let ExprKind::Ident(name) = &object.kind {
                    if let Some((ptr, ty)) = self.lookup_var(name) {
                        if ty.is_array_type() {
                            let arr_ty = ty.into_array_type();
                            let elem_ptr = unsafe {
                                self.builder
                                    .build_gep(
                                        arr_ty,
                                        ptr,
                                        &[
                                            self.context
                                                .i64_type()
                                                .const_int(0, false),
                                            idx_val,
                                        ],
                                        "idx",
                                    )
                                    .unwrap()
                            };
                            let elem_ty = arr_ty.get_element_type();
                            return Ok(self
                                .builder
                                .build_load(elem_ty, elem_ptr, "idx.load")
                                .unwrap());
                        } else if ty.is_pointer_type() {
                            let loaded = self
                                .builder
                                .build_load(ty, ptr, "ptr.load")
                                .unwrap()
                                .into_pointer_value();
                            let elem_ptr = unsafe {
                                self.builder
                                    .build_gep(
                                        self.context.i64_type(),
                                        loaded,
                                        &[idx_val],
                                        "idx.ptr",
                                    )
                                    .unwrap()
                            };
                            return Ok(self
                                .builder
                                .build_load(
                                    self.context.i64_type(),
                                    elem_ptr,
                                    "idx.load",
                                )
                                .unwrap());
                        } else if ty.is_struct_type() {
                            // string as {ptr,len} ? For now string as ptr: fallback to pointer case
                            // but string currently maps to ptr, not struct, so handle pointer case above
                        }
                    }
                }
                // Fallback: try codegen object as pointer value (if object is MemberAccess that yields pointer? For now handle general via loaded pointer)
                // For Phase 2, support a[i] where a is array variable; for other cases, error
                return Err(CodegenError{message: "unsupported indexing base; only direct array variable indexing supported in Phase 2".into(), span: expr.span});
            }
            ExprKind::StringLit(s) => {
                // Create global string pointer: build_global_string_ptr returns i8* to null-terminated string
                let ptr =
                    self.builder.build_global_string_ptr(s, "str.lit").unwrap();
                // string type is ptr (i8*), return ptr
                Ok(ptr.as_pointer_value().into())
            }
            ExprKind::CharLit(ch) => {
                // char as i32 Unicode scalar
                Ok(self.context.i32_type().const_int(*ch as u64, false).into())
            }
            ExprKind::StructLit { ty, fields } => {
                let sname = match ty {
                    Type::Named(n, _) => n.clone(),
                    _ => {
                        return Err(CodegenError {
                            message: "struct literal requires named type"
                                .into(),
                            span: expr.span,
                        });
                    }
                };
                let st =
                    *self.struct_types.get(&sname).ok_or(CodegenError {
                        message: format!("unknown struct {sname}"),
                        span: expr.span,
                    })?;
                let field_map = self.struct_fields.get(&sname).unwrap().clone();
                // Allocate temp struct on stack, fill fields, load aggregate value
                let tmp =
                    self.builder.build_alloca(st, "struct.lit.tmp").unwrap();
                for (fname, _fspan, fexpr) in fields {
                    let idx = *field_map.get(fname).ok_or(CodegenError {
                        message: format!("unknown field {fname}"),
                        span: expr.span,
                    })?;
                    let val = self.codegen_expr(fexpr)?;
                    let field_ptr = self
                        .builder
                        .build_struct_gep(st, tmp, idx, &format!("s.{}", fname))
                        .unwrap();
                    self.builder.build_store(field_ptr, val).unwrap();
                }
                let loaded = self
                    .builder
                    .build_load(st.as_basic_type_enum(), tmp, "struct.lit")
                    .unwrap();
                Ok(loaded)
            }
            ExprKind::Match(m) => self.codegen_match(m, expr.span),
        }
    }

    fn codegen_match(
        &mut self,
        m: &MatchExpr,
        span: Span,
    ) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        // Evaluate scrutinee once
        let scrut_val = self.codegen_expr(&m.scrutinee)?;
        let func = self.cur_fn.unwrap();
        let merge_bb = self.context.append_basic_block(func, "match.merge");
        // Determine result type lazily via first arm body; allocate after
        let mut result_alloc: Option<(
            PointerValue<'ctx>,
            BasicTypeEnum<'ctx>,
        )> = None;
        // start check block is current insertion block after scrutinee
        let mut cur_check_bb = self.builder.get_insert_block().unwrap();
        for (idx, arm) in m.arms.iter().enumerate() {
            let is_last = idx == m.arms.len() - 1;
            let arm_bb = self
                .context
                .append_basic_block(func, &format!("match.arm{}", idx));
            let next_bb = if is_last {
                merge_bb
            } else {
                self.context
                    .append_basic_block(func, &format!("match.next{}", idx))
            };
            // Emit pattern check in cur_check_bb
            self.builder.position_at_end(cur_check_bb);
            // pattern match value (i1)
            let pattern_is_wild = matches!(arm.pattern, Pattern::Wildcard(_));
            let pattern_val: inkwell::values::IntValue<'ctx> =
                if pattern_is_wild {
                    self.context.bool_type().const_int(1, false)
                } else {
                    match &arm.pattern {
                        Pattern::LitInt(v, _) => {
                            let lit = self
                                .context
                                .i64_type()
                                .const_int(*v as u64, true);
                            self.builder
                                .build_int_compare(
                                    IntPredicate::EQ,
                                    scrut_val.into_int_value(),
                                    lit,
                                    "match.pat",
                                )
                                .unwrap()
                        }
                        Pattern::LitBool(b, _) => {
                            let lit = self
                                .context
                                .bool_type()
                                .const_int(if *b { 1 } else { 0 }, false);
                            self.builder
                                .build_int_compare(
                                    IntPredicate::EQ,
                                    scrut_val.into_int_value(),
                                    lit,
                                    "match.pat",
                                )
                                .unwrap()
                        }
                        Pattern::Wildcard(_) => unreachable!(),
                    }
                };
            // Handle guard
            if let Some(guard_expr) = &arm.guard {
                // pattern matched -> check guard, else go next
                let guard_check_bb = self
                    .context
                    .append_basic_block(func, &format!("match.guard{}", idx));
                self.builder
                    .build_conditional_branch(
                        pattern_val,
                        guard_check_bb,
                        next_bb,
                    )
                    .unwrap();
                self.builder.position_at_end(guard_check_bb);
                let guard_val = self.codegen_expr(guard_expr)?;
                // guard must be bool
                self.builder
                    .build_conditional_branch(
                        guard_val.into_int_value(),
                        arm_bb,
                        next_bb,
                    )
                    .unwrap();
            } else {
                self.builder
                    .build_conditional_branch(pattern_val, arm_bb, next_bb)
                    .unwrap();
            }
            // Emit arm body
            self.builder.position_at_end(arm_bb);
            let body_val_opt: Option<BasicValueEnum<'ctx>> = match &arm.body {
                MatchArmBody::Expr(e) => Some(self.codegen_expr(e)?),
                MatchArmBody::Block(b) => {
                    let _ = self.codegen_block(b)?;
                    None
                }
            };
            if let Some(v) = body_val_opt {
                if result_alloc.is_none() {
                    let ty = v.get_type();
                    // allocate in entry for result
                    let alloc =
                        self.create_entry_block_alloca("match.result", ty);
                    result_alloc = Some((alloc, ty));
                }
                let (ptr, _) = result_alloc.unwrap();
                // Need to handle if current block already terminated (e.g., return inside arm)
                if self
                    .builder
                    .get_insert_block()
                    .unwrap()
                    .get_terminator()
                    .is_none()
                {
                    self.builder.build_store(ptr, v).unwrap();
                }
            }
            if self
                .builder
                .get_insert_block()
                .unwrap()
                .get_terminator()
                .is_none()
            {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
            }
            cur_check_bb = next_bb;
            // if last arm was wildcard and had no next, cur_check_bb is merge; no need to continue
            if is_last {
                break;
            }
        }
        // Position at merge for subsequent code
        self.builder.position_at_end(merge_bb);
        if let Some((ptr, ty)) = result_alloc {
            let loaded =
                self.builder.build_load(ty, ptr, "match.result").unwrap();
            Ok(loaded)
        } else {
            // void match (all arms blocks) — return dummy int 0 for expression context; caller in ExprStmt will discard
            Ok(self.context.i64_type().const_int(0, false).into())
        }
    }

    // Helper: compute GEP pointer to field `field` of object expression (object must be variable or member chain)
    fn codegen_field_ptr(
        &self,
        object: &Expr,
        field: &str,
    ) -> Result<PointerValue<'ctx>, CodegenError> {
        // Resolve base variable pointer and struct type chain
        // Simplify Phase 2: only support `ident` or `ident.field...` chains where base is a variable
        let (base_ptr, base_struct_name) =
            self.resolve_base_struct_ptr(object)?;
        let fields =
            self.struct_fields
                .get(&base_struct_name)
                .ok_or(CodegenError {
                    message: format!("unknown struct {base_struct_name}"),
                    span: object.span,
                })?;
        let field_idx = *fields.get(field).ok_or(CodegenError {
            message: format!("struct {base_struct_name} has no field {field}"),
            span: object.span,
        })?;
        let st = self.struct_types.get(&base_struct_name).unwrap();
        // If object is a simple ident, we directly GEP from base_ptr
        // If object is member access chain, we need to compute progressively. For chain like a.b.c, resolve_base already handles? Let's expand.
        // Our resolve_base only handles single level; for nested, we need progressive GEPs.
        // Instead, handle recursively: if object is MemberAccess, compute pointer to its field first, then GEP again
        if let ExprKind::MemberAccess {
            object: inner,
            field: inner_field,
            ..
        } = &object.kind
        {
            // recursively get pointer to inner.field, then GEP to `field`
            // inner.field pointer is the object for this access
            let inner_ptr = self.codegen_field_ptr(inner, inner_field)?;
            // inner_ptr points to intermediate struct field (which itself is a struct if nested)
            // Need to know type of inner field to GEP second field: intermediate field must be struct containing `field`
            // For simplicity Phase 2, we don't support nested struct field chains beyond one level (flat)
            // But we can attempt: derive intermediate struct name
            let inner_ty = self.infer_expr_ty(object)?; // this is actually type of inner.field? For chain, we want type of object itself
            // Instead use direct: if object is MemberAccess, its type is field type, which must be struct
            // We can infer inner field's type via sema-ty lookup
            let inner_lval_ty = self.infer_expr_ty(object)?;
            if let crate::sema::Ty::Struct(ref inner_sname) = inner_lval_ty {
                let inner_st = self.struct_types.get(inner_sname).unwrap();
                let parent_fields =
                    self.struct_fields.get(inner_sname).unwrap();
                let idx2 = *parent_fields.get(field).unwrap();
                // GEP from inner_ptr (which points to inner struct value storage) to its field
                // inner_ptr is pointer to struct (the field) — first index 0 is struct, second is field
                let ptr = self
                    .builder
                    .build_struct_gep(*inner_st, inner_ptr, idx2, field)
                    .unwrap();
                return Ok(ptr);
            } else {
                return Err(CodegenError {
                    message: "nested field access on non-struct".into(),
                    span: object.span,
                });
            }
        }
        // Simple case: base ident field
        let ptr = self
            .builder
            .build_struct_gep(*st, base_ptr, field_idx, field)
            .unwrap();
        Ok(ptr)
    }

    fn resolve_base_struct_ptr(
        &self,
        object: &Expr,
    ) -> Result<(PointerValue<'ctx>, String), CodegenError> {
        match &object.kind {
            ExprKind::Ident(name) => {
                let (ptr, ty) = self.lookup_var(name).ok_or(CodegenError{message: format!("undefined var {name}"), span: object.span})?;
                let sname = self.ty_to_struct_name(&ty)?;
                Ok((ptr, sname))
            }
            ExprKind::This => {
                let (ptr, ty) = self.lookup_var("this").ok_or(CodegenError{message: "`this` outside method".into(), span: object.span})?;
                let sname = self.cur_class.clone().ok_or(CodegenError{message: "`this` outside method".into(), span: object.span})?;
                let instance_ptr = self.builder.build_load(ty, ptr, "this.load").unwrap().into_pointer_value();
                Ok((instance_ptr, sname))
            }
            ExprKind::MemberAccess {
                object: inner,
                field,
                ..
            } => {
                // For `a.b` as base for `a.b.c`, we need pointer to `a.b` field which holds a struct
                let field_ptr = self.codegen_field_ptr(inner, field)?;
                // its pointee type is struct field's type
                let inner_ty = self.infer_expr_ty(object)?;
                if let crate::sema::Ty::Struct(ref n) = inner_ty {
                    // but for base resolution, the pointer's pointee is the struct of inner_ty? Actually a.b's type is field type, not outer
                    // For chain `a.b.c`, we need `a.b` pointer as base for `.c`, and its struct name is inner_ty
                    Ok((field_ptr, n.clone()))
                } else {
                    Err(CodegenError {
                        message: "resolve base not struct".into(),
                        span: object.span,
                    })
                }
            }
            _ => Err(CodegenError {
                message: "field access base must be variable or field".into(),
                span: object.span,
            }),
        }
    }

    fn ty_to_struct_name(
        &self,
        ty: &BasicTypeEnum<'ctx>,
    ) -> Result<String, CodegenError> {
        if let BasicTypeEnum::StructType(st) = ty {
            for (name, s) in &self.struct_types {
                if *s == *st {
                    return Ok(name.clone());
                }
                if s.as_basic_type_enum() == *ty {
                    return Ok(name.clone());
                }
            }
            Err(CodegenError {
                message: "struct type not found".into(),
                span: Span::new(0, 0),
            })
        } else {
            Err(CodegenError {
                message: "variable is not struct".into(),
                span: Span::new(0, 0),
            })
        }
    }

    fn infer_expr_ty(
        &self,
        expr: &Expr,
    ) -> Result<crate::sema::Ty, CodegenError> {
        match &expr.kind {
            ExprKind::Ident(name) => {
                for scope in self.vars.iter().rev() {
                    if let Some((_, ty)) = scope.get(name) {
                        if ty.is_struct_type() {
                            let sname = self.ty_to_struct_name(ty).unwrap();
                            return Ok(crate::sema::Ty::Struct(sname));
                        } else if ty.is_int_type() {
                            let bw = ty.into_int_type().get_bit_width();
                            if bw == 1 { return Ok(crate::sema::Ty::Bool); } else { return Ok(crate::sema::Ty::Int); }
                        } else if ty.is_pointer_type() {
                            return Ok(crate::sema::Ty::Pointer(Box::new(crate::sema::Ty::Int)));
                        } else if ty.is_array_type() {
                            return Ok(crate::sema::Ty::Array(Box::new(crate::sema::Ty::Int)));
                        }
                    }
                }
                Err(CodegenError{message: format!("cannot infer type of {name}"), span: expr.span})
            }
            ExprKind::This => {
                if let Some(cls) = &self.cur_class { return Ok(crate::sema::Ty::Struct(cls.clone())); }
                Err(CodegenError{message: "`this` outside method".into(), span: expr.span})
            }
            ExprKind::MemberAccess { object, field, .. } => {
                let obj_ty = self.infer_expr_ty(object)?;
                if let crate::sema::Ty::Struct(ref sname) = obj_ty {
                    let fields = self.struct_fields.get(sname).unwrap();
                    let idx = fields.get(field).unwrap();
                    let st = self.struct_types.get(sname).unwrap();
                    let fty = st.get_field_type_at_index(*idx).unwrap();
                    if fty.is_int_type() {
                        let bw = fty.into_int_type().get_bit_width();
                        if bw == 1 { return Ok(crate::sema::Ty::Bool); } else { return Ok(crate::sema::Ty::Int); }
                    } else if fty.is_struct_type() {
                        let sname2 = self.ty_to_struct_name(&fty).unwrap();
                        return Ok(crate::sema::Ty::Struct(sname2));
                    }
                    return Err(CodegenError{message: "unsupported field type inference".into(), span: expr.span});
                }
                Err(CodegenError{message: "member access inference on non-struct".into(), span: expr.span})
            }
            _ => Err(CodegenError{message: "cannot infer type of this expr for struct GEP".into(), span: expr.span}),
        }
    }

    fn lookup_var(
        &self,
        name: &str,
    ) -> Option<(PointerValue<'ctx>, BasicTypeEnum<'ctx>)> {
        for scope in self.vars.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(*v);
            }
        }
        None
    }
}

pub fn compile_to_object(
    program: &Program,
    obj_path: &Path,
) -> Result<(), String> {
    let context = Context::create();
    let mut cg = Codegen::new(&context, "holt");
    cg.compile_program(program).map_err(|e| {
        format!("{} at {}..{}", e.message, e.span.start, e.span.end)
    })?;
    cg.module.verify().map_err(|e| e.to_string())?;
    inkwell::targets::Target::initialize_all(
        &inkwell::targets::InitializationConfig::default(),
    );
    let triple = inkwell::targets::TargetMachine::get_default_triple();
    let target = inkwell::targets::Target::from_triple(&triple)
        .map_err(|e| e.to_string())?;
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            inkwell::OptimizationLevel::Default,
            inkwell::targets::RelocMode::Default,
            inkwell::targets::CodeModel::Default,
        )
        .ok_or("failed to create target machine")?;
    machine
        .write_to_file(&cg.module, inkwell::targets::FileType::Object, obj_path)
        .map_err(|e| e.to_string())?;
    Ok(())
}
