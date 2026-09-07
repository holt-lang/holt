//! Phase 1 codegen — LLVM via `inkwell` 0.10 (llvm21-1).
//! All locals/params are `alloca` in entry block; no JIT (object + clang).

use std::collections::HashMap;
use std::path::Path;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::BasicTypeEnum;
use inkwell::values::{BasicValueEnum, FunctionValue, PointerValue};
use inkwell::basic_block::BasicBlock;
use inkwell::IntPredicate;

use crate::ast::*;
use crate::token::Span;

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
    pub span: Span,
}

pub struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    vars: Vec<HashMap<String, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>>,
    funcs: HashMap<String, (FunctionValue<'ctx>, TyInfo)>,
    cur_fn: Option<FunctionValue<'ctx>>,
    cur_is_main: bool,
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
        Self { context, module, builder, vars: Vec::new(), funcs: HashMap::new(), cur_fn: None, cur_is_main: false }
    }

    pub fn get_module_ir(&self) -> String {
        self.module.print_to_string().to_string()
    }

    pub fn compile_program(&mut self, prog: &Program) -> Result<(), CodegenError> {
        // Declare all functions first (for forward calls)
        for item in &prog.items {
            if let Item::Function(f) = item {
                self.declare_function(f)?;
            }
        }
        // Define bodies
        for item in &prog.items {
            if let Item::Function(f) = item {
                self.codegen_function(f)?;
            }
        }
        // Verify
        if let Err(e) = self.module.verify() {
            return Err(CodegenError{message: format!("LLVM verify failed: {e}"), span: prog.span});
        }
        Ok(())
    }

    fn llvm_ty_for(&self, ty: &Type) -> BasicTypeEnum<'ctx> {
        match ty {
            Type::Int(_) => self.context.i64_type().into(),
            Type::Bool(_) => self.context.bool_type().into(),
            Type::Void(_) => panic!("void not a first-class type in llvm_ty_for"),
        }
    }
    fn llvm_ty_for_sema(&self, ty: &crate::sema::Ty) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            crate::sema::Ty::Int => Some(self.context.i64_type().into()),
            crate::sema::Ty::Bool => Some(self.context.bool_type().into()),
            crate::sema::Ty::Void => None,
        }
    }

    fn declare_function(&mut self, f: &Function) -> Result<(), CodegenError> {
        let ret_sema: crate::sema::Ty = (&f.ret_ty).into();
        let param_semas: Vec<crate::sema::Ty> = f.params.iter().map(|p| (&p.ty).into()).collect();

        let param_types: Vec<inkwell::types::BasicMetadataTypeEnum> = param_semas.iter()
            .filter_map(|t| self.llvm_ty_for_sema(t).map(|bt| bt.into()))
            .collect();

        // Special ABI for `main`: C `int main()` is always i32, regardless of Holt `void`/`int`.
        let fn_ty = if f.name == "main" {
            self.context.i32_type().fn_type(&param_types, false)
        } else {
            match ret_sema {
                crate::sema::Ty::Void => self.context.void_type().fn_type(&param_types, false),
                crate::sema::Ty::Int => self.context.i64_type().fn_type(&param_types, false),
                crate::sema::Ty::Bool => self.context.bool_type().fn_type(&param_types, false),
            }
        };

        let func = self.module.add_function(&f.name, fn_ty, None);
        self.funcs.insert(f.name.clone(), (func, TyInfo{ret: ret_sema, params: param_semas}));
        Ok(())
    }

    fn codegen_function(&mut self, f: &Function) -> Result<(), CodegenError> {
        let (func, info) = self.funcs.get(&f.name).cloned().ok_or(CodegenError{message: format!("undeclared func {}", f.name), span: f.name_span})?;
        self.cur_fn = Some(func);
        self.cur_is_main = f.name == "main";
        let entry = self.context.append_basic_block(func, "entry");
        self.builder.position_at_end(entry);

        // Entry allocas for params
        self.vars.push(HashMap::new());
        for (i, param) in f.params.iter().enumerate() {
            let llvm_ty = self.llvm_ty_for(&param.ty);
            let alloca = self.create_entry_block_alloca(&param.name, llvm_ty);
            // param value: func.get_nth_param
            let param_val = func.get_nth_param(i as u32).unwrap();
            self.builder.build_store(alloca, param_val).unwrap();
            self.vars.last_mut().unwrap().insert(param.name.clone(), (alloca, llvm_ty));
        }

        // Codegen body block
        let always_returns = self.codegen_block(&f.body)?;

        // Handle implicit return for functions that didn't always return
        if !always_returns && self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            if self.cur_is_main {
                // main is always i32 in LLVM: return 0 for void main or if fallthrough
                let zero = self.context.i32_type().const_int(0, false);
                self.builder.build_return(Some(&zero)).unwrap();
            } else if info.ret == crate::sema::Ty::Void {
                self.builder.build_return(None).unwrap();
            } else {
                let zero: BasicValueEnum = match info.ret {
                    crate::sema::Ty::Int => self.context.i64_type().const_int(0, false).into(),
                    crate::sema::Ty::Bool => self.context.bool_type().const_int(0, false).into(),
                    crate::sema::Ty::Void => unreachable!(),
                };
                self.builder.build_return(Some(&zero)).unwrap();
            }
        }

        self.vars.pop();
        self.cur_fn = None;
        self.cur_is_main = false;
        // Verify function
        if !func.verify(true) {
            return Err(CodegenError{message: format!("function {} failed verification", f.name), span: f.span});
        }
        Ok(())
    }

    // Helper: alloca at entry (even if currently in other block)
    fn create_entry_block_alloca(&self, name: &str, ty: BasicTypeEnum<'ctx>) -> PointerValue<'ctx> {
        let func = self.cur_fn.unwrap();
        let entry = func.get_first_basic_block().unwrap();
        let builder = self.context.create_builder();
        // position before first instruction if any, else at end
        if let Some(first) = entry.get_first_instruction() {
            builder.position_before(&first);
        } else {
            builder.position_at_end(entry);
        }
        match ty {
            BasicTypeEnum::IntType(t) => builder.build_alloca(t, name).unwrap(),
            BasicTypeEnum::FloatType(t) => builder.build_alloca(t, name).unwrap(),
            BasicTypeEnum::PointerType(t) => builder.build_alloca(t, name).unwrap(),
            BasicTypeEnum::ArrayType(t) => builder.build_alloca(t, name).unwrap(),
            BasicTypeEnum::StructType(t) => builder.build_alloca(t, name).unwrap(),
            BasicTypeEnum::VectorType(t) => builder.build_alloca(t, name).unwrap(),
            BasicTypeEnum::ScalableVectorType(t) => builder.build_alloca(t, name).unwrap(),
        }
    }

    fn codegen_block(&mut self, block: &Block) -> Result<bool, CodegenError> {
        // Nested scope
        self.vars.push(HashMap::new());
        let mut always_returns = false;
        for stmt in &block.stmts {
            // If prior stmt already terminated (return), then current block is after terminator:
            // Need new basic block? Our stmts are sequential in same block; after terminator we should create unreachable block.
            // Simplest: if current block terminated, create a new dead block.
            if self.builder.get_insert_block().unwrap().get_terminator().is_some() {
                let dead = self.context.append_basic_block(self.cur_fn.unwrap(), "dead");
                self.builder.position_at_end(dead);
                // dead block is unreachable but we still codegen for verification (won't be reached).
            }
            let stmt_returns = self.codegen_stmt(stmt)?;
            if stmt_returns { always_returns = true; }
        }
        self.vars.pop();
        Ok(always_returns)
    }

    fn codegen_stmt(&mut self, stmt: &Stmt) -> Result<bool, CodegenError> {
        match stmt {
            Stmt::VarDecl(d) => {
                let ty = self.llvm_ty_for(&d.ty);
                let alloca = self.create_entry_block_alloca(&d.name, ty);
                self.vars.last_mut().unwrap().insert(d.name.clone(), (alloca, ty));
                if let Some(init) = &d.init {
                    let val = self.codegen_expr(init)?;
                    self.builder.build_store(alloca, val).unwrap();
                } else {
                    // zero-init to avoid undef
                    let zero: BasicValueEnum = match &d.ty {
                        Type::Int(_) => self.context.i64_type().const_int(0, false).into(),
                        Type::Bool(_) => self.context.bool_type().const_int(0, false).into(),
                        Type::Void(_) => unreachable!(),
                    };
                    self.builder.build_store(alloca, zero).unwrap();
                }
                Ok(false)
            }
            Stmt::Expr(e) => { let _ = self.codegen_expr(&e.expr)?; Ok(false) }
            Stmt::Block(b) => self.codegen_block(b),
            Stmt::Return(r) => {
                if self.cur_is_main {
                    // main: Holt void -> i32 0, Holt int (i64) -> truncate to i32
                    if let Some(expr) = &r.value {
                        let val = self.codegen_expr(expr)?;
                        // trunc i64 -> i32 if needed
                        let ret_val = if val.is_int_value() && val.into_int_value().get_type().get_bit_width() == 64 {
                            self.builder.build_int_truncate(val.into_int_value(), self.context.i32_type(), "main.trunc").unwrap().into()
                        } else if val.is_int_value() && val.into_int_value().get_type().get_bit_width() == 1 {
                            // bool main not spec, zero-extend to i32
                            self.builder.build_int_z_extend(val.into_int_value(), self.context.i32_type(), "main.zext").unwrap().into()
                        } else {
                            val
                        };
                        self.builder.build_return(Some(&ret_val)).unwrap();
                    } else {
                        // void main: return 0
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
                let else_bb = if s.else_block.is_some() { Some(self.context.append_basic_block(func, "if.else")) } else { None };
                let merge_bb = self.context.append_basic_block(func, "if.merge");

                if let Some(else_bb) = else_bb {
                    self.builder.build_conditional_branch(cond_bool, then_bb, else_bb).unwrap();
                } else {
                    self.builder.build_conditional_branch(cond_bool, then_bb, merge_bb).unwrap();
                }

                // then
                self.builder.position_at_end(then_bb);
                let then_ret = self.codegen_block(&s.then_block)?;
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }

                // else
                let else_ret = if let (Some(else_bb), Some(else_block)) = (else_bb, &s.else_block) {
                    self.builder.position_at_end(else_bb);
                    let r = self.codegen_block(else_block)?;
                    if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                        self.builder.build_unconditional_branch(merge_bb).unwrap();
                    }
                    r
                } else { false };

                // merge
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

                self.builder.position_at_end(body_bb);
                let _ = self.codegen_block(&s.body)?;
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(cond_bb).unwrap();
                }

                self.builder.position_at_end(exit_bb);
                Ok(false)
            }
        }
    }

    fn codegen_expr(&mut self, expr: &Expr) -> Result<BasicValueEnum<'ctx>, CodegenError> {
        match &expr.kind {
            ExprKind::IntLit(v) => Ok(self.context.i64_type().const_int(*v as u64, true).into()),
            ExprKind::BoolLit(b) => Ok(self.context.bool_type().const_int(if *b {1} else {0}, false).into()),
            ExprKind::Ident(name) => {
                let (ptr, ty) = self.lookup_var(name).ok_or(CodegenError{message: format!("undefined var {name}"), span: expr.span})?;
                let val = self.builder.build_load(ty, ptr, name).unwrap();
                Ok(val)
            }
            ExprKind::Paren(inner) => self.codegen_expr(inner),
            ExprKind::Unary{op, expr: inner} => {
                let v = self.codegen_expr(inner)?;
                match op {
                    UnaryOp::Not => {
                        let b = v.into_int_value();
                        let res = self.builder.build_xor(b, self.context.bool_type().const_int(1, false), "not").unwrap();
                        Ok(res.into())
                    }
                    UnaryOp::Neg => {
                        let i = v.into_int_value();
                        let zero = self.context.i64_type().const_int(0, false);
                        let res = self.builder.build_int_sub(zero, i, "neg").unwrap();
                        Ok(res.into())
                    }
                    UnaryOp::Pos => Ok(v),
                }
            }
            ExprKind::Binary{op, lhs, rhs} => {
                let l = self.codegen_expr(lhs)?;
                let r = self.codegen_expr(rhs)?;
                let res: BasicValueEnum = match op {
                    BinOp::Add => self.builder.build_int_add(l.into_int_value(), r.into_int_value(), "add").unwrap().into(),
                    BinOp::Sub => self.builder.build_int_sub(l.into_int_value(), r.into_int_value(), "sub").unwrap().into(),
                    BinOp::Mul => self.builder.build_int_mul(l.into_int_value(), r.into_int_value(), "mul").unwrap().into(),
                    BinOp::Div => self.builder.build_int_signed_div(l.into_int_value(), r.into_int_value(), "div").unwrap().into(),
                    BinOp::Mod => self.builder.build_int_signed_rem(l.into_int_value(), r.into_int_value(), "mod").unwrap().into(),
                    BinOp::Lt => self.builder.build_int_compare(IntPredicate::SLT, l.into_int_value(), r.into_int_value(), "lt").unwrap().into(),
                    BinOp::Le => self.builder.build_int_compare(IntPredicate::SLE, l.into_int_value(), r.into_int_value(), "le").unwrap().into(),
                    BinOp::Gt => self.builder.build_int_compare(IntPredicate::SGT, l.into_int_value(), r.into_int_value(), "gt").unwrap().into(),
                    BinOp::Ge => self.builder.build_int_compare(IntPredicate::SGE, l.into_int_value(), r.into_int_value(), "ge").unwrap().into(),
                    BinOp::Is => self.builder.build_int_compare(IntPredicate::EQ, l.into_int_value(), r.into_int_value(), "is").unwrap().into(),
                    BinOp::IsNot => self.builder.build_int_compare(IntPredicate::NE, l.into_int_value(), r.into_int_value(), "isnot").unwrap().into(),
                    BinOp::And => self.builder.build_and(l.into_int_value(), r.into_int_value(), "and").unwrap().into(),
                    BinOp::Or => self.builder.build_or(l.into_int_value(), r.into_int_value(), "or").unwrap().into(),
                };
                Ok(res)
            }
            ExprKind::Assign{target, target_span: _, value} => {
                let (ptr, _) = self.lookup_var(target).ok_or(CodegenError{message: format!("undefined var {target}"), span: expr.span})?;
                let val = self.codegen_expr(value)?;
                self.builder.build_store(ptr, val).unwrap();
                Ok(val)
            }
            ExprKind::Call{callee, callee_span: _, args} => {
                let (func, _info) = self.funcs.get(callee).cloned().ok_or(CodegenError{message: format!("undefined function {callee}"), span: expr.span})?;
                let mut arg_vals: Vec<inkwell::values::BasicMetadataValueEnum> = Vec::new();
                for a in args {
                    let v = self.codegen_expr(a)?;
                    arg_vals.push(v.into());
                }
                let call = self.builder.build_call(func, &arg_vals, "call").unwrap();
                let vk = call.try_as_basic_value();
                if vk.is_basic() {
                    Ok(vk.basic().unwrap())
                } else {
                    // void call — return dummy i64 0 for expression context (should not be used as value)
                    Ok(self.context.i64_type().const_int(0, false).into())
                }
            }
        }
    }

    fn lookup_var(&self, name: &str) -> Option<(PointerValue<'ctx>, BasicTypeEnum<'ctx>)> {
        for scope in self.vars.iter().rev() {
            if let Some(v) = scope.get(name) { return Some(*v); }
        }
        None
    }
}

pub fn compile_to_object(program: &Program, obj_path: &Path) -> Result<(), String> {
    let context = Context::create();
    let mut cg = Codegen::new(&context, "holt");
    cg.compile_program(program).map_err(|e| format!("{} at {}..{}", e.message, e.span.start, e.span.end))?;
    cg.module.verify().map_err(|e| e.to_string())?;

    // Initialize target for object emission
    inkwell::targets::Target::initialize_all(&inkwell::targets::InitializationConfig::default());

    let triple = inkwell::targets::TargetMachine::get_default_triple();
    let target = inkwell::targets::Target::from_triple(&triple).map_err(|e| e.to_string())?;
    let machine = target.create_target_machine(
        &triple,
        "generic",
        "",
        inkwell::OptimizationLevel::Default,
        inkwell::targets::RelocMode::Default,
        inkwell::targets::CodeModel::Default,
    ).ok_or("failed to create target machine")?;

    machine.write_to_file(&cg.module, inkwell::targets::FileType::Object, obj_path).map_err(|e| e.to_string())?;
    // Optionally also print IR for debug
    // eprintln!("{}", cg.module.print_to_string().to_string());
    Ok(())
}
