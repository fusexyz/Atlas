use super::ir::{self as ir, *};
use crate::parser::ast::*;
use std::collections::HashMap;

#[derive(Debug)]
pub struct LowerError(pub String);

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lowering error: {}", self.0)
    }
}

macro_rules! err {
    ($($t:tt)*) => { Err(LowerError(format!($($t)*))) };
}

type LR<T> = Result<T, LowerError>;

#[derive(Debug, Clone)]
pub struct StructLayout {
    pub size: u64,
    pub align: u64,
    pub field_offsets: HashMap<String, u64>,
    pub field_types: HashMap<String, TypeSpec>,
}

pub fn lower_module(tu: &TranslationUnit) -> LR<Module> {
    let mut ctx = ModuleCtx::new();
    ctx.enum_constants = tu.enum_constants.clone();
    for item in &tu.items {
        if let TopLevel::Typedef(ty, name, _) = item {
            ctx.typedefs.insert(name.clone(), ty.clone());
        }
    }
    let mut pending_structs: Vec<&StructDef> = tu
        .items
        .iter()
        .filter_map(|item| {
            if let TopLevel::StructDef(s) = item {
                Some(s)
            } else {
                None
            }
        })
        .collect();
    while !pending_structs.is_empty() {
        let before = pending_structs.len();
        let mut retry = Vec::new();
        for s in pending_structs {
            match ctx.lower_struct_def(s) {
                Ok(()) => {}
                Err(e)
                    if e.0.starts_with("undefined struct '")
                        || e.0.starts_with("undefined union '") =>
                {
                    retry.push(s);
                }
                Err(e) => return Err(e),
            }
        }
        if retry.len() == before {
            if let Err(e) = ctx.lower_struct_def(retry[0]) {
                return Err(e);
            }
        }
        pending_structs = retry;
    }
    for item in &tu.items {
        match item {
            TopLevel::StructDef(_) | TopLevel::Typedef(_, _, _) => {}
            _ => {
                ctx.lower_top_level(item)?;
            }
        }
    }
    Ok(ctx.module)
}

struct ModuleCtx {
    module: Module,
    func_sigs: HashMap<String, (Vec<IrType>, IrType)>,
    str_count: u32,
    typedefs: HashMap<String, TypeSpec>,
    struct_layouts: HashMap<String, StructLayout>,
    globals: HashMap<String, TypeSpec>,
    enum_constants: HashMap<String, i64>,
}

impl ModuleCtx {
    fn new() -> Self {
        Self {
            module: Module::default(),
            func_sigs: HashMap::new(),
            str_count: 0,
            typedefs: HashMap::new(),
            struct_layouts: HashMap::new(),
            globals: HashMap::new(),
            enum_constants: HashMap::new(),
        }
    }

    fn lower_top_level(&mut self, item: &TopLevel) -> LR<()> {
        match item {
            TopLevel::Function(f) => self
                .lower_function(f)
                .map_err(|e| LowerError(format!("in function '{}': {}", f.name, e.0))),
            TopLevel::FuncDecl(d) => self.lower_func_decl(d),
            TopLevel::Declaration(d) => self.lower_global_decl(d),
            TopLevel::StructDef(s) => self.lower_struct_def(s),
            TopLevel::Typedef(_, _, _) => Ok(()),
        }
    }

    fn lower_struct_def(&mut self, s: &StructDef) -> LR<()> {
        let mut size = 0u64;
        let mut align = 1u64;
        let mut field_offsets = HashMap::new();
        let mut field_types = HashMap::new();
        let pack = s.pack;

        if s.is_union {
            for field in &s.fields {
                let (f_sz, f_aln) = get_type_size_and_align(
                    &field.ty,
                    &self.typedefs,
                    &self.struct_layouts,
                    &self.enum_constants,
                )?;
                let packed_aln = f_aln.min(pack);
                align = align.max(packed_aln);
                size = size.max(f_sz);

                if field.name.starts_with("__anon_field_") {
                    if let Some(anon_name) = get_struct_or_union_name(&field.ty, &self.typedefs) {
                        if let Some(anon_layout) = self.struct_layouts.get(&anon_name).cloned() {
                            for (sub_name, sub_offset) in &anon_layout.field_offsets {
                                field_offsets.insert(sub_name.clone(), 0 + sub_offset);
                                if let Some(sub_ty) = anon_layout.field_types.get(sub_name) {
                                    field_types.insert(sub_name.clone(), sub_ty.clone());
                                }
                            }
                            continue;
                        }
                    }
                }

                field_offsets.insert(field.name.clone(), 0u64);
                field_types.insert(field.name.clone(), field.ty.clone());
            }
            size = (size + align - 1) / align * align;
        } else {
            for field in &s.fields {
                let (f_sz, f_aln) = get_type_size_and_align(
                    &field.ty,
                    &self.typedefs,
                    &self.struct_layouts,
                    &self.enum_constants,
                )?;
                let packed_aln = f_aln.min(pack);
                align = align.max(packed_aln);
                size = (size + packed_aln - 1) / packed_aln * packed_aln;

                if field.name.starts_with("__anon_field_") {
                    if let Some(anon_name) = get_struct_or_union_name(&field.ty, &self.typedefs) {
                        if let Some(anon_layout) = self.struct_layouts.get(&anon_name).cloned() {
                            for (sub_name, sub_offset) in &anon_layout.field_offsets {
                                field_offsets.insert(sub_name.clone(), size + sub_offset);
                                if let Some(sub_ty) = anon_layout.field_types.get(sub_name) {
                                    field_types.insert(sub_name.clone(), sub_ty.clone());
                                }
                            }
                            size += f_sz;
                            continue;
                        }
                    }
                }

                field_offsets.insert(field.name.clone(), size);
                field_types.insert(field.name.clone(), field.ty.clone());
                size += f_sz;
            }
            size = (size + align - 1) / align * align;
        }

        self.struct_layouts.insert(
            s.name.clone(),
            StructLayout {
                size,
                align,
                field_offsets,
                field_types,
            },
        );
        Ok(())
    }

    fn lower_func_decl(&mut self, decl: &FuncDecl) -> LR<()> {
        let ret_ty = lower_type(
            &decl.ret_ty,
            &self.typedefs,
            &self.struct_layouts,
            &self.enum_constants,
        )?;
        let param_tys: Vec<IrType> = decl
            .params
            .iter()
            .map(|p| {
                lower_type(
                    &p.ty,
                    &self.typedefs,
                    &self.struct_layouts,
                    &self.enum_constants,
                )
            })
            .collect::<Result<_, _>>()?;
        self.func_sigs
            .insert(decl.name.clone(), (param_tys.clone(), ret_ty.clone()));
        let params: Vec<ir::Param> = param_tys
            .iter()
            .enumerate()
            .map(|(i, ty)| ir::Param {
                ty: ty.clone(),
                reg: VReg(i as u32),
            })
            .collect();
        self.module.functions.push(Function {
            name: decl.name.clone(),
            params,
            ret_ty,
            blocks: Vec::new(),
            is_extern: true,
            variadic: decl.variadic,
        });
        Ok(())
    }

    fn lower_global_decl(&mut self, decl: &Decl) -> LR<()> {
        self.globals.insert(decl.name.clone(), decl.ty.clone());
        let ty = lower_type(
            &decl.ty,
            &self.typedefs,
            &self.struct_layouts,
            &self.enum_constants,
        )?;
        let init = match &decl.init {
            Some(Expr {
                kind: ExprKind::IntLit(v),
                ..
            }) => Some(Value::ImmI(*v)),
            Some(Expr {
                kind: ExprKind::FloatLit(v),
                ..
            }) => Some(Value::ImmF(*v)),
            Some(_) => return err!("global initializer must be a constant"),
            None => None,
        };
        let is_extern = decl.storage == StorageClass::Extern;
        self.module.globals.push(Global {
            name: decl.name.clone(),
            ty,
            init,
            is_extern,
        });
        Ok(())
    }

    fn lower_function(&mut self, func: &FuncDef) -> LR<()> {
        let ret_ty = lower_type(
            &func.ret_ty,
            &self.typedefs,
            &self.struct_layouts,
            &self.enum_constants,
        )?;
        let mut params = Vec::new();
        let mut param_tys = Vec::new();
        let mut next_reg = 0u32;

        for p in &func.params {
            let ty = lower_type(
                &p.ty,
                &self.typedefs,
                &self.struct_layouts,
                &self.enum_constants,
            )?;
            param_tys.push(ty.clone());
            params.push(IrParam {
                ty,
                reg: VReg(next_reg),
            });
            next_reg += 1;
        }

        self.func_sigs
            .insert(func.name.clone(), (param_tys, ret_ty.clone()));

        let mut fb = FuncBuilder {
            name: func.name.clone(),
            ret_ty: ret_ty.clone(),
            params: params.clone(),
            variadic: func.variadic,
            blocks: Vec::new(),
            next_reg,
            next_block: 0,
            locals: HashMap::new(),
            func_sigs: &self.func_sigs,
            typedefs: &self.typedefs,
            struct_layouts: &self.struct_layouts,
            globals: &self.globals,
            enum_constants: &self.enum_constants,
            loop_exit: Vec::new(),
            loop_cond: Vec::new(),
            string_lits: Vec::new(),
            next_str: self.str_count,
        };

        let entry = fb.new_block();
        fb.seal_entry(entry);

        for p in &params {
            let slot = fb.emit_alloca(entry, p.ty.clone());
            fb.emit(
                entry,
                Instr::Store {
                    ty: p.ty.clone(),
                    val: Value::Reg(p.reg),
                    ptr: Value::Reg(slot),
                },
            );
            if let Some(name) = func.params[params.iter().position(|x| x.reg == p.reg).unwrap()]
                .name
                .as_deref()
            {
                let param_spec =
                    &func.params[params.iter().position(|x| x.reg == p.reg).unwrap()].ty;
                fb.locals
                    .insert(name.to_string(), (slot, param_spec.clone()));
            }
        }

        let last_bb = fb.lower_block(&func.body, entry)?;

        if !fb.is_terminated(last_bb) {
            fb.set_term(last_bb, Terminator::Ret(None));
        }

        let (func_ir, strings, next_str) = fb.finish()?;
        self.str_count = next_str;
        self.module.string_lits.extend(strings);
        self.module.functions.push(func_ir);
        Ok(())
    }
}

#[derive(Clone)]
struct IrParam {
    ty: IrType,
    reg: VReg,
}

struct FuncBuilder<'a> {
    name: String,
    ret_ty: IrType,
    params: Vec<IrParam>,
    variadic: bool,
    blocks: Vec<BasicBlock>,
    next_reg: u32,
    next_block: u32,
    locals: HashMap<String, (VReg, TypeSpec)>,
    func_sigs: &'a HashMap<String, (Vec<IrType>, IrType)>,
    typedefs: &'a HashMap<String, TypeSpec>,
    struct_layouts: &'a HashMap<String, StructLayout>,
    globals: &'a HashMap<String, TypeSpec>,
    enum_constants: &'a HashMap<String, i64>,
    loop_exit: Vec<BlockId>,
    loop_cond: Vec<BlockId>,
    string_lits: Vec<(String, Vec<u8>)>,
    next_str: u32,
}

impl<'a> FuncBuilder<'a> {
    fn new_block(&mut self) -> BlockId {
        let id = BlockId(self.next_block);
        self.next_block += 1;
        self.blocks.push(BasicBlock::new(id));
        id
    }

    fn seal_entry(&mut self, _bb: BlockId) {}

    fn block_mut(&mut self, id: BlockId) -> &mut BasicBlock {
        self.blocks.iter_mut().find(|b| b.id == id).unwrap()
    }

    fn is_terminated(&self, id: BlockId) -> bool {
        let bb = self.blocks.iter().find(|b| b.id == id).unwrap();
        !matches!(bb.term, Terminator::Unreachable)
    }

    fn set_term(&mut self, id: BlockId, term: Terminator) {
        self.block_mut(id).term = term;
    }

    fn emit(&mut self, bb: BlockId, instr: Instr) {
        self.block_mut(bb).instrs.push(instr);
    }

    fn fresh(&mut self) -> VReg {
        let r = VReg(self.next_reg);
        self.next_reg += 1;
        r
    }

    fn emit_alloca(&mut self, bb: BlockId, ty: IrType) -> VReg {
        let dst = self.fresh();
        self.emit(bb, Instr::Alloca { dst, ty });
        dst
    }

    fn lower_block(&mut self, stmts: &[Stmt], mut bb: BlockId) -> LR<BlockId> {
        for stmt in stmts {
            bb = self.lower_stmt(stmt, bb)?;
            if self.is_terminated(bb) {
                break;
            }
        }
        Ok(bb)
    }

    fn lower_stmt(&mut self, stmt: &Stmt, bb: BlockId) -> LR<BlockId> {
        match &stmt.kind {
            StmtKind::Decl(d) => self.lower_local_decl(d, bb),
            StmtKind::Expr(e) => {
                self.lower_expr(e, bb)?;
                Ok(bb)
            }
            StmtKind::Block(b) => self.lower_block(b, bb),
            StmtKind::Return(e) => {
                let val = match e {
                    Some(expr) => Some(self.lower_expr(expr, bb)?),
                    None => None,
                };
                self.set_term(bb, Terminator::Ret(val));
                Ok(bb)
            }
            StmtKind::If(cond, then_s, else_s) => {
                self.lower_if(cond, then_s, else_s.as_deref(), bb)
            }
            StmtKind::While(cond, body) => self.lower_while(cond, body, bb),
            StmtKind::DoWhile(body, cond) => self.lower_do_while(body, cond, bb),
            StmtKind::For(init, cond, post, body) => {
                self.lower_for(init.as_ref(), cond.as_ref(), post.as_ref(), body, bb)
            }
            StmtKind::Break => {
                let exit = *self
                    .loop_exit
                    .last()
                    .ok_or_else(|| LowerError("break outside loop".into()))?;
                self.set_term(bb, Terminator::Br(exit));
                Ok(bb)
            }
            StmtKind::Continue => {
                let cont = *self
                    .loop_cond
                    .last()
                    .ok_or_else(|| LowerError("continue outside loop".into()))?;
                self.set_term(bb, Terminator::Br(cont));
                Ok(bb)
            }
            StmtKind::Label(_, s) => self.lower_stmt(s, bb),
            StmtKind::Goto(_) => err!("goto not supported in IR lowering"),
            StmtKind::Switch(_, _) => err!("switch not yet implemented"),
            StmtKind::Case(_, _) => err!("case not yet implemented"),
            StmtKind::Default(_) => err!("default not yet implemented"),
        }
    }

    fn lower_local_decl(&mut self, decl: &Decl, bb: BlockId) -> LR<BlockId> {
        let ty = lower_type(
            &decl.ty,
            self.typedefs,
            self.struct_layouts,
            &self.enum_constants,
        )?;
        let slot = self.emit_alloca(bb, ty.clone());
        self.locals
            .insert(decl.name.clone(), (slot, decl.ty.clone()));

        if let Some(init_expr) = &decl.init {
            let val = self.lower_expr(init_expr, bb)?;
            self.emit(
                bb,
                Instr::Store {
                    ty,
                    val,
                    ptr: Value::Reg(slot),
                },
            );
        }
        Ok(bb)
    }

    fn lower_if(
        &mut self,
        cond: &Expr,
        then_s: &Stmt,
        else_s: Option<&Stmt>,
        bb: BlockId,
    ) -> LR<BlockId> {
        let cond_val = self.lower_expr(cond, bb)?;
        let then_bb = self.new_block();
        let else_bb = self.new_block();
        let merge_bb = self.new_block();

        self.set_term(
            bb,
            Terminator::CondBr {
                cond: cond_val,
                then_bb,
                else_bb,
            },
        );

        let then_end = self.lower_stmt(then_s, then_bb)?;
        if !self.is_terminated(then_end) {
            self.set_term(then_end, Terminator::Br(merge_bb));
        }

        let else_end = if let Some(s) = else_s {
            let end = self.lower_stmt(s, else_bb)?;
            end
        } else {
            else_bb
        };
        if !self.is_terminated(else_end) {
            self.set_term(else_end, Terminator::Br(merge_bb));
        }

        Ok(merge_bb)
    }

    fn lower_while(&mut self, cond: &Expr, body: &Stmt, bb: BlockId) -> LR<BlockId> {
        let cond_bb = self.new_block();
        let body_bb = self.new_block();
        let exit_bb = self.new_block();

        self.set_term(bb, Terminator::Br(cond_bb));

        let cond_val = self.lower_expr(cond, cond_bb)?;
        self.set_term(
            cond_bb,
            Terminator::CondBr {
                cond: cond_val,
                then_bb: body_bb,
                else_bb: exit_bb,
            },
        );

        self.loop_exit.push(exit_bb);
        self.loop_cond.push(cond_bb);
        let body_end = self.lower_stmt(body, body_bb)?;
        self.loop_exit.pop();
        self.loop_cond.pop();

        if !self.is_terminated(body_end) {
            self.set_term(body_end, Terminator::Br(cond_bb));
        }

        Ok(exit_bb)
    }

    fn lower_do_while(&mut self, body: &Stmt, cond: &Expr, bb: BlockId) -> LR<BlockId> {
        let body_bb = self.new_block();
        let cond_bb = self.new_block();
        let exit_bb = self.new_block();

        self.set_term(bb, Terminator::Br(body_bb));

        self.loop_exit.push(exit_bb);
        self.loop_cond.push(cond_bb);
        let body_end = self.lower_stmt(body, body_bb)?;
        self.loop_exit.pop();
        self.loop_cond.pop();

        if !self.is_terminated(body_end) {
            self.set_term(body_end, Terminator::Br(cond_bb));
        }

        let cond_val = self.lower_expr(cond, cond_bb)?;
        self.set_term(
            cond_bb,
            Terminator::CondBr {
                cond: cond_val,
                then_bb: body_bb,
                else_bb: exit_bb,
            },
        );

        Ok(exit_bb)
    }

    fn lower_for(
        &mut self,
        init: Option<&ForInit>,
        cond: Option<&Expr>,
        post: Option<&Expr>,
        body: &Stmt,
        bb: BlockId,
    ) -> LR<BlockId> {
        let bb = if let Some(i) = init {
            match i {
                ForInit::Decl(d) => self.lower_local_decl(d, bb)?,
                ForInit::Expr(e) => {
                    self.lower_expr(e, bb)?;
                    bb
                }
            }
        } else {
            bb
        };

        let cond_bb = self.new_block();
        let body_bb = self.new_block();
        let post_bb = self.new_block();
        let exit_bb = self.new_block();

        self.set_term(bb, Terminator::Br(cond_bb));

        if let Some(c) = cond {
            let cond_val = self.lower_expr(c, cond_bb)?;
            self.set_term(
                cond_bb,
                Terminator::CondBr {
                    cond: cond_val,
                    then_bb: body_bb,
                    else_bb: exit_bb,
                },
            );
        } else {
            self.set_term(cond_bb, Terminator::Br(body_bb));
        }

        self.loop_exit.push(exit_bb);
        self.loop_cond.push(post_bb);
        let body_end = self.lower_stmt(body, body_bb)?;
        self.loop_exit.pop();
        self.loop_cond.pop();

        if !self.is_terminated(body_end) {
            self.set_term(body_end, Terminator::Br(post_bb));
        }

        if let Some(p) = post {
            self.lower_expr(p, post_bb)?;
        }
        if !self.is_terminated(post_bb) {
            self.set_term(post_bb, Terminator::Br(cond_bb));
        }

        Ok(exit_bb)
    }

    fn lower_expr(&mut self, expr: &Expr, bb: BlockId) -> LR<Value> {
        match &expr.kind {
            ExprKind::IntLit(v) => Ok(Value::ImmI(*v)),
            ExprKind::FloatLit(v) => Ok(Value::ImmF(*v)),
            ExprKind::CharLit(c) => Ok(Value::ImmI(*c as i64)),
            ExprKind::StringLit(s) => {
                let name = format!("__str_{}", self.next_str);
                self.next_str += 1;
                let mut bytes = s.as_bytes().to_vec();
                bytes.push(0);
                self.string_lits.push((name.clone(), bytes));
                Ok(Value::Global(name))
            }

            ExprKind::Ident(name) => {
                if let Some((slot, ty_spec)) = self.locals.get(name).cloned() {
                    let dst = self.fresh();
                    let ty = lower_type(
                        &ty_spec,
                        self.typedefs,
                        self.struct_layouts,
                        &self.enum_constants,
                    )?;
                    self.emit(
                        bb,
                        Instr::Load {
                            dst,
                            ty,
                            ptr: Value::Reg(slot),
                        },
                    );
                    Ok(Value::Reg(dst))
                } else if let Some(ty_spec) = self.globals.get(name).cloned() {
                    let dst = self.fresh();
                    let ty = lower_type(
                        &ty_spec,
                        self.typedefs,
                        self.struct_layouts,
                        &self.enum_constants,
                    )?;
                    self.emit(
                        bb,
                        Instr::Load {
                            dst,
                            ty,
                            ptr: Value::Global(name.clone()),
                        },
                    );
                    Ok(Value::Reg(dst))
                } else {
                    Ok(Value::Global(name.clone()))
                }
            }

            ExprKind::Assign(op, lhs, rhs) => {
                let rval = self.lower_expr(rhs, bb)?;
                let rval = if *op != AssignOp::Assign {
                    let lval = self.lower_expr(lhs, bb)?;
                    let bin_op = assign_to_binop(op);
                    let ty = IrType::I64;
                    let dst = self.fresh();
                    self.emit(
                        bb,
                        Instr::BinOp {
                            dst,
                            op: bin_op,
                            ty,
                            lhs: lval,
                            rhs: rval,
                        },
                    );
                    Value::Reg(dst)
                } else {
                    rval
                };

                let ptr = self.lvalue_ptr(lhs, bb)?;
                let ty = self.lvalue_type(lhs)?;
                self.emit(
                    bb,
                    Instr::Store {
                        ty,
                        val: rval.clone(),
                        ptr,
                    },
                );
                Ok(rval)
            }

            ExprKind::Binary(op, lhs, rhs) => {
                let l = self.lower_expr(lhs, bb)?;
                let r = self.lower_expr(rhs, bb)?;
                let ir_op = bin_op_to_ir(op);
                let ty = IrType::I64;
                let dst = self.fresh();
                self.emit(
                    bb,
                    Instr::BinOp {
                        dst,
                        op: ir_op,
                        ty,
                        lhs: l,
                        rhs: r,
                    },
                );
                Ok(Value::Reg(dst))
            }

            ExprKind::Unary(op, inner) => {
                let val = self.lower_expr(inner, bb)?;
                let (ir_op, ty) = match op {
                    UnaryOp::Neg => (UnaryOpKind::Neg, IrType::I64),
                    UnaryOp::Not => (UnaryOpKind::Not, IrType::I8),
                    UnaryOp::BitNot => (UnaryOpKind::BitNot, IrType::I64),
                    UnaryOp::PreInc | UnaryOp::PostInc => {
                        let ptr = self.lvalue_ptr(inner, bb)?;
                        let ty = self.lvalue_type(inner)?;
                        let old = val;
                        let one = Value::ImmI(1);
                        let new_dst = self.fresh();
                        self.emit(
                            bb,
                            Instr::BinOp {
                                dst: new_dst,
                                op: BinOpKind::Add,
                                ty: ty.clone(),
                                lhs: old.clone(),
                                rhs: one,
                            },
                        );
                        self.emit(
                            bb,
                            Instr::Store {
                                ty,
                                val: Value::Reg(new_dst),
                                ptr,
                            },
                        );
                        return Ok(if *op == UnaryOp::PreInc {
                            Value::Reg(new_dst)
                        } else {
                            old
                        });
                    }
                    UnaryOp::PreDec | UnaryOp::PostDec => {
                        let ptr = self.lvalue_ptr(inner, bb)?;
                        let ty = self.lvalue_type(inner)?;
                        let old = val;
                        let one = Value::ImmI(1);
                        let new_dst = self.fresh();
                        self.emit(
                            bb,
                            Instr::BinOp {
                                dst: new_dst,
                                op: BinOpKind::Sub,
                                ty: ty.clone(),
                                lhs: old.clone(),
                                rhs: one,
                            },
                        );
                        self.emit(
                            bb,
                            Instr::Store {
                                ty,
                                val: Value::Reg(new_dst),
                                ptr,
                            },
                        );
                        return Ok(if *op == UnaryOp::PreDec {
                            Value::Reg(new_dst)
                        } else {
                            old
                        });
                    }
                };
                let dst = self.fresh();
                self.emit(
                    bb,
                    Instr::UnaryOp {
                        dst,
                        op: ir_op,
                        ty,
                        val,
                    },
                );
                Ok(Value::Reg(dst))
            }

            ExprKind::AddrOf(inner) => self.lvalue_ptr(inner, bb),

            ExprKind::Deref(inner) => {
                let ptr = self.lower_expr(inner, bb)?;
                let dst = self.fresh();
                self.emit(
                    bb,
                    Instr::Load {
                        dst,
                        ty: IrType::I64,
                        ptr,
                    },
                );
                Ok(Value::Reg(dst))
            }

            ExprKind::Call(func_expr, args) => {
                let func_val = match &func_expr.kind {
                    ExprKind::Ident(name) => Value::Global(name.clone()),
                    _ => self.lower_expr(func_expr, bb)?,
                };
                let mut arg_vals = Vec::new();
                for a in args {
                    arg_vals.push(self.lower_expr(a, bb)?);
                }
                let ret_ty = match &func_expr.kind {
                    ExprKind::Ident(name) => self
                        .func_sigs
                        .get(name)
                        .map(|(_, r)| r.clone())
                        .unwrap_or(IrType::I32),
                    _ => IrType::I32,
                };
                let dst = if ret_ty == IrType::Void {
                    None
                } else {
                    let r = self.fresh();
                    Some(r)
                };
                self.emit(
                    bb,
                    Instr::Call {
                        dst,
                        func: func_val,
                        args: arg_vals,
                        ret_ty,
                    },
                );
                Ok(dst.map(Value::Reg).unwrap_or(Value::ImmI(0)))
            }

            ExprKind::Ternary(cond, then_e, else_e) => {
                let cond_val = self.lower_expr(cond, bb)?;
                let then_bb = self.new_block();
                let else_bb = self.new_block();
                let merge_bb = self.new_block();
                self.set_term(
                    bb,
                    Terminator::CondBr {
                        cond: cond_val,
                        then_bb,
                        else_bb,
                    },
                );

                let then_val = self.lower_expr(then_e, then_bb)?;
                let then_end = then_bb;
                if !self.is_terminated(then_end) {
                    self.set_term(then_end, Terminator::Br(merge_bb));
                }

                let else_val = self.lower_expr(else_e, else_bb)?;
                let else_end = else_bb;
                if !self.is_terminated(else_end) {
                    self.set_term(else_end, Terminator::Br(merge_bb));
                }

                let dst = self.fresh();
                self.emit(merge_bb, Instr::Copy { dst, val: then_val });
                let dst2 = self.fresh();
                self.emit(
                    merge_bb,
                    Instr::Copy {
                        dst: dst2,
                        val: else_val,
                    },
                );

                Ok(Value::Reg(dst))
            }

            ExprKind::Cast(ty, inner) => {
                let val = self.lower_expr(inner, bb)?;
                let to_ty =
                    lower_type(ty, self.typedefs, self.struct_layouts, &self.enum_constants)?;
                let dst = self.fresh();
                self.emit(
                    bb,
                    Instr::Cast {
                        dst,
                        from_ty: IrType::I64,
                        to_ty,
                        val,
                    },
                );
                Ok(Value::Reg(dst))
            }

            ExprKind::Sizeof(arg) => {
                let size = match arg {
                    SizeofArg::Type(ty) => {
                        lower_type(ty, self.typedefs, self.struct_layouts, &self.enum_constants)?
                            .size_bytes()
                    }
                    SizeofArg::Expr(e) => {
                        let ty_spec = expr_type(
                            e,
                            &self.locals,
                            self.typedefs,
                            self.struct_layouts,
                            self.globals,
                        )?;
                        let (sz, _) = get_type_size_and_align(
                            &ty_spec,
                            self.typedefs,
                            self.struct_layouts,
                            &self.enum_constants,
                        )?;
                        sz
                    }
                };
                Ok(Value::ImmI(size as i64))
            }

            ExprKind::Alignof(arg) => {
                let align = match arg {
                    SizeofArg::Type(ty) => {
                        let (_, aln) = get_type_size_and_align(
                            ty,
                            self.typedefs,
                            self.struct_layouts,
                            &self.enum_constants,
                        )?;
                        aln
                    }
                    SizeofArg::Expr(e) => {
                        let ty_spec = expr_type(
                            e,
                            &self.locals,
                            self.typedefs,
                            self.struct_layouts,
                            self.globals,
                        )?;
                        let (_, aln) = get_type_size_and_align(
                            &ty_spec,
                            self.typedefs,
                            self.struct_layouts,
                            &self.enum_constants,
                        )?;
                        aln
                    }
                };
                Ok(Value::ImmI(align as i64))
            }

            ExprKind::Index(base, idx) => {
                let ptr = self.gep_ptr(base, idx, bb)?;
                let ty_spec = expr_type(
                    expr,
                    &self.locals,
                    self.typedefs,
                    self.struct_layouts,
                    self.globals,
                )?;
                let ty = lower_type(
                    &ty_spec,
                    self.typedefs,
                    self.struct_layouts,
                    &self.enum_constants,
                )?;
                let dst = self.fresh();
                self.emit(bb, Instr::Load { dst, ty, ptr });
                Ok(Value::Reg(dst))
            }

            ExprKind::Member(_, _) | ExprKind::Arrow(_, _) => {
                let ptr = self.lvalue_ptr(expr, bb)?;
                let ty_spec = expr_type(
                    expr,
                    &self.locals,
                    self.typedefs,
                    self.struct_layouts,
                    self.globals,
                )?;
                let ty = lower_type(
                    &ty_spec,
                    self.typedefs,
                    self.struct_layouts,
                    &self.enum_constants,
                )?;
                let dst = self.fresh();
                self.emit(bb, Instr::Load { dst, ty, ptr });
                Ok(Value::Reg(dst))
            }
        }
    }

    fn lvalue_ptr(&mut self, expr: &Expr, bb: BlockId) -> LR<Value> {
        match &expr.kind {
            ExprKind::Ident(name) => {
                if let Some((slot, _)) = self.locals.get(name) {
                    Ok(Value::Reg(*slot))
                } else {
                    Ok(Value::Global(name.clone()))
                }
            }
            ExprKind::Deref(inner) => self.lower_expr(inner, bb),
            ExprKind::Index(base, idx) => self.gep_ptr(base, idx, bb),
            ExprKind::Member(base, field) => {
                let base_ptr = self.lvalue_ptr(base, bb)?;
                let base_ty = expr_type(
                    base,
                    &self.locals,
                    self.typedefs,
                    self.struct_layouts,
                    self.globals,
                )?;
                let resolved = resolve_typedefs(&base_ty, self.typedefs)?;
                let struct_name = match resolved {
                    TypeSpec::Struct(name) | TypeSpec::Union(name) => name,
                    _ => return err!("member access on non-struct-or-union type"),
                };
                let layout = self
                    .struct_layouts
                    .get(&struct_name)
                    .ok_or_else(|| LowerError(format!("undefined struct '{}'", struct_name)))?;
                let offset = *layout.field_offsets.get(field).ok_or_else(|| {
                    LowerError(format!("no field '{}' in struct '{}'", field, struct_name))
                })?;

                let dst = self.fresh();
                self.emit(
                    bb,
                    Instr::Gep {
                        dst,
                        elem_ty: IrType::I8,
                        base: base_ptr,
                        idx: Value::ImmI(offset as i64),
                    },
                );
                Ok(Value::Reg(dst))
            }
            ExprKind::Arrow(base, field) => {
                let base_ptr = self.lower_expr(base, bb)?;
                let base_ty = expr_type(
                    base,
                    &self.locals,
                    self.typedefs,
                    self.struct_layouts,
                    self.globals,
                )?;
                let resolved = resolve_typedefs(&base_ty, self.typedefs)?;
                let inner_ty = match resolved {
                    TypeSpec::Pointer(inner) => inner,
                    _ => return err!("arrow access on non-pointer type"),
                };
                let resolved_inner = resolve_typedefs(&inner_ty, self.typedefs)?;
                let struct_name = match resolved_inner {
                    TypeSpec::Struct(name) | TypeSpec::Union(name) => name,
                    _ => return err!("arrow access on pointer to non-struct-or-union type"),
                };
                let layout = self
                    .struct_layouts
                    .get(&struct_name)
                    .ok_or_else(|| LowerError(format!("undefined struct '{}'", struct_name)))?;
                let offset = *layout.field_offsets.get(field).ok_or_else(|| {
                    LowerError(format!("no field '{}' in struct '{}'", field, struct_name))
                })?;

                let dst = self.fresh();
                self.emit(
                    bb,
                    Instr::Gep {
                        dst,
                        elem_ty: IrType::I8,
                        base: base_ptr,
                        idx: Value::ImmI(offset as i64),
                    },
                );
                Ok(Value::Reg(dst))
            }
            _ => err!("not a valid lvalue"),
        }
    }

    fn lvalue_type(&self, expr: &Expr) -> LR<IrType> {
        let ty_spec = expr_type(
            expr,
            &self.locals,
            self.typedefs,
            self.struct_layouts,
            self.globals,
        )?;
        lower_type(
            &ty_spec,
            self.typedefs,
            self.struct_layouts,
            &self.enum_constants,
        )
    }

    fn gep_ptr(&mut self, base: &Expr, idx: &Expr, bb: BlockId) -> LR<Value> {
        let base_ptr = self.lvalue_ptr(base, bb)?;
        let idx_val = self.lower_expr(idx, bb)?;
        let base_ty = expr_type(
            base,
            &self.locals,
            self.typedefs,
            self.struct_layouts,
            self.globals,
        )?;
        let resolved = resolve_typedefs(&base_ty, self.typedefs)?;
        let elem_ty = match resolved {
            TypeSpec::Pointer(inner) => lower_type(
                &inner,
                self.typedefs,
                self.struct_layouts,
                &self.enum_constants,
            )?,
            TypeSpec::Array(elem, _) => lower_type(
                &elem,
                self.typedefs,
                self.struct_layouts,
                &self.enum_constants,
            )?,
            _ => IrType::I64,
        };
        let dst = self.fresh();
        self.emit(
            bb,
            Instr::Gep {
                dst,
                elem_ty,
                base: base_ptr,
                idx: idx_val,
            },
        );
        Ok(Value::Reg(dst))
    }

    fn finish(self) -> LR<(Function, Vec<(String, Vec<u8>)>, u32)> {
        let params = self
            .params
            .iter()
            .map(|p| ir::Param {
                ty: p.ty.clone(),
                reg: p.reg,
            })
            .collect();
        let func = Function {
            name: self.name,
            params,
            ret_ty: self.ret_ty,
            blocks: self.blocks,
            is_extern: false,
            variadic: self.variadic,
        };
        Ok((func, self.string_lits, self.next_str))
    }
}

pub fn lower_type(
    ty: &TypeSpec,
    typedefs: &HashMap<String, TypeSpec>,
    struct_layouts: &HashMap<String, StructLayout>,
    enum_constants: &HashMap<String, i64>,
) -> LR<IrType> {
    match ty {
        TypeSpec::Void => Ok(IrType::Void),
        TypeSpec::Char => Ok(IrType::I8),
        TypeSpec::Short => Ok(IrType::I16),
        TypeSpec::Int => Ok(IrType::I32),
        TypeSpec::Long => Ok(IrType::I32),
        TypeSpec::LongLong => Ok(IrType::I64),
        TypeSpec::Float => Ok(IrType::F32),
        TypeSpec::Double => Ok(IrType::F64),
        TypeSpec::Unsigned(inner) => lower_type(inner, typedefs, struct_layouts, enum_constants),
        TypeSpec::Signed(inner) => lower_type(inner, typedefs, struct_layouts, enum_constants),
        TypeSpec::Pointer(inner) => {
            let pointee =
                lower_type(inner, typedefs, struct_layouts, enum_constants).unwrap_or(IrType::I8);
            Ok(IrType::Ptr(Box::new(pointee)))
        }
        TypeSpec::Array(elem, Some(size_expr)) => {
            let elem_ty = lower_type(elem, typedefs, struct_layouts, enum_constants)?;
            let n = eval_const_expr_lower(size_expr, typedefs, struct_layouts, enum_constants)
                .ok_or_else(|| {
                    LowerError(format!(
                        "array size must be a constant integer expression: {:?}",
                        size_expr
                    ))
                })? as u64;
            Ok(IrType::Array(Box::new(elem_ty), n))
        }
        TypeSpec::Array(elem, None) => Ok(IrType::Ptr(Box::new(lower_type(
            elem,
            typedefs,
            struct_layouts,
            enum_constants,
        )?))),
        TypeSpec::Struct(name) => {
            let layout = struct_layouts
                .get(name)
                .ok_or_else(|| LowerError(format!("undefined struct '{}'", name)))?;
            Ok(IrType::Array(Box::new(IrType::I8), layout.size))
        }
        TypeSpec::Union(name) => {
            let layout = struct_layouts
                .get(name)
                .ok_or_else(|| LowerError(format!("undefined union '{}'", name)))?;
            Ok(IrType::Array(Box::new(IrType::I8), layout.size))
        }
        TypeSpec::Enum(_) => Ok(IrType::I32),
        TypeSpec::Named(name) => {
            let resolved = typedefs
                .get(name)
                .ok_or_else(|| LowerError(format!("undefined typedef type '{}'", name)))?;
            lower_type(resolved, typedefs, struct_layouts, enum_constants)
        }
    }
}

fn bin_op_to_ir(op: &BinaryOp) -> BinOpKind {
    match op {
        BinaryOp::Add => BinOpKind::Add,
        BinaryOp::Sub => BinOpKind::Sub,
        BinaryOp::Mul => BinOpKind::Mul,
        BinaryOp::Div => BinOpKind::Div,
        BinaryOp::Mod => BinOpKind::Rem,
        BinaryOp::BitAnd => BinOpKind::And,
        BinaryOp::BitOr => BinOpKind::Or,
        BinaryOp::BitXor => BinOpKind::Xor,
        BinaryOp::Shl => BinOpKind::Shl,
        BinaryOp::Shr => BinOpKind::Shr,
        BinaryOp::And => BinOpKind::And,
        BinaryOp::Or => BinOpKind::Or,
        BinaryOp::Eq => BinOpKind::Eq,
        BinaryOp::Ne => BinOpKind::Ne,
        BinaryOp::Lt => BinOpKind::Lt,
        BinaryOp::Gt => BinOpKind::Gt,
        BinaryOp::Le => BinOpKind::Le,
        BinaryOp::Ge => BinOpKind::Ge,
        BinaryOp::Comma => BinOpKind::Add,
    }
}

fn assign_to_binop(op: &AssignOp) -> BinOpKind {
    match op {
        AssignOp::AddAssign => BinOpKind::Add,
        AssignOp::SubAssign => BinOpKind::Sub,
        AssignOp::MulAssign => BinOpKind::Mul,
        AssignOp::DivAssign => BinOpKind::Div,
        AssignOp::ModAssign => BinOpKind::Rem,
        AssignOp::AndAssign => BinOpKind::And,
        AssignOp::OrAssign => BinOpKind::Or,
        AssignOp::XorAssign => BinOpKind::Xor,
        AssignOp::ShlAssign => BinOpKind::Shl,
        AssignOp::ShrAssign => BinOpKind::Shr,
        AssignOp::Assign => unreachable!(),
    }
}

fn get_type_size_and_align(
    ty: &TypeSpec,
    typedefs: &HashMap<String, TypeSpec>,
    struct_layouts: &HashMap<String, StructLayout>,
    enum_constants: &HashMap<String, i64>,
) -> LR<(u64, u64)> {
    match ty {
        TypeSpec::Void => Ok((0, 1)),
        TypeSpec::Char => Ok((1, 1)),
        TypeSpec::Short => Ok((2, 2)),
        TypeSpec::Int => Ok((4, 4)),
        TypeSpec::Long => Ok((4, 4)),
        TypeSpec::LongLong => Ok((8, 8)),
        TypeSpec::Float => Ok((4, 4)),
        TypeSpec::Double => Ok((8, 8)),
        TypeSpec::Unsigned(inner) | TypeSpec::Signed(inner) => {
            get_type_size_and_align(inner, typedefs, struct_layouts, enum_constants)
        }
        TypeSpec::Pointer(_) => Ok((8, 8)),
        TypeSpec::Array(elem, Some(size_expr)) => {
            let (elem_sz, elem_aln) =
                get_type_size_and_align(elem, typedefs, struct_layouts, enum_constants)?;
            let n = eval_const_expr_lower(size_expr, typedefs, struct_layouts, enum_constants)
                .ok_or_else(|| {
                    LowerError(format!(
                        "array size must be a constant integer expression: {:?}",
                        size_expr
                    ))
                })? as u64;
            Ok((elem_sz * n, elem_aln))
        }
        TypeSpec::Array(_elem, None) => Ok((8, 8)),
        TypeSpec::Struct(name) => {
            let layout = struct_layouts
                .get(name)
                .ok_or_else(|| LowerError(format!("undefined struct '{}'", name)))?;
            Ok((layout.size, layout.align))
        }
        TypeSpec::Union(name) => {
            let layout = struct_layouts
                .get(name)
                .ok_or_else(|| LowerError(format!("undefined union '{}'", name)))?;
            Ok((layout.size, layout.align))
        }
        TypeSpec::Enum(_) => Ok((4, 4)),
        TypeSpec::Named(name) => {
            let resolved = typedefs
                .get(name)
                .ok_or_else(|| LowerError(format!("undefined typedef '{}'", name)))?;
            get_type_size_and_align(resolved, typedefs, struct_layouts, enum_constants)
        }
    }
}

fn expr_type(
    expr: &Expr,
    locals: &HashMap<String, (VReg, TypeSpec)>,
    typedefs: &HashMap<String, TypeSpec>,
    struct_layouts: &HashMap<String, StructLayout>,
    globals: &HashMap<String, TypeSpec>,
) -> LR<TypeSpec> {
    match &expr.kind {
        ExprKind::IntLit(_) => Ok(TypeSpec::Int),
        ExprKind::FloatLit(_) => Ok(TypeSpec::Double),
        ExprKind::CharLit(_) => Ok(TypeSpec::Char),
        ExprKind::StringLit(_) => Ok(TypeSpec::Pointer(Box::new(TypeSpec::Char))),
        ExprKind::Ident(name) => {
            if let Some((_, ty)) = locals.get(name) {
                Ok(ty.clone())
            } else if let Some(ty) = globals.get(name) {
                Ok(ty.clone())
            } else {
                Ok(TypeSpec::Int)
            }
        }
        ExprKind::Unary(_, inner) => expr_type(inner, locals, typedefs, struct_layouts, globals),
        ExprKind::Binary(_, lhs, _) => expr_type(lhs, locals, typedefs, struct_layouts, globals),
        ExprKind::Assign(_, lhs, _) => expr_type(lhs, locals, typedefs, struct_layouts, globals),
        ExprKind::Ternary(_, then_e, _) => {
            expr_type(then_e, locals, typedefs, struct_layouts, globals)
        }
        ExprKind::Call(_, _) => Ok(TypeSpec::Int),
        ExprKind::Index(base, _) => {
            let base_ty = expr_type(base, locals, typedefs, struct_layouts, globals)?;
            let resolved = resolve_typedefs(&base_ty, typedefs)?;
            match resolved {
                TypeSpec::Pointer(inner) => Ok(*inner.clone()),
                TypeSpec::Array(elem, _) => Ok(*elem.clone()),
                _ => err!("indexing non-pointer type"),
            }
        }
        ExprKind::Member(base, field) => {
            let base_ty = expr_type(base, locals, typedefs, struct_layouts, globals)?;
            let resolved = resolve_typedefs(&base_ty, typedefs)?;
            let struct_name = match resolved {
                TypeSpec::Struct(name) | TypeSpec::Union(name) => name,
                _ => return err!("member access on non-struct-or-union type"),
            };
            let layout = struct_layouts
                .get(&struct_name)
                .ok_or_else(|| LowerError(format!("undefined struct '{}'", struct_name)))?;
            let f_ty = layout.field_types.get(field).ok_or_else(|| {
                LowerError(format!("no field '{}' in struct '{}'", field, struct_name))
            })?;
            Ok(f_ty.clone())
        }
        ExprKind::Arrow(base, field) => {
            let base_ty = expr_type(base, locals, typedefs, struct_layouts, globals)?;
            let resolved = resolve_typedefs(&base_ty, typedefs)?;
            let inner_ty = match resolved {
                TypeSpec::Pointer(inner) => inner,
                _ => return err!("arrow access on non-pointer type"),
            };
            let resolved_inner = resolve_typedefs(&inner_ty, typedefs)?;
            let struct_name = match resolved_inner {
                TypeSpec::Struct(name) | TypeSpec::Union(name) => name,
                _ => return err!("arrow access on pointer to non-struct-or-union type"),
            };
            let layout = struct_layouts
                .get(&struct_name)
                .ok_or_else(|| LowerError(format!("undefined struct '{}'", struct_name)))?;
            let f_ty = layout.field_types.get(field).ok_or_else(|| {
                LowerError(format!("no field '{}' in struct '{}'", field, struct_name))
            })?;
            Ok(f_ty.clone())
        }
        ExprKind::Cast(ty, _) => Ok(ty.clone()),
        ExprKind::Sizeof(_) | ExprKind::Alignof(_) => Ok(TypeSpec::Int),
        ExprKind::AddrOf(inner) => {
            let inner_ty = expr_type(inner, locals, typedefs, struct_layouts, globals)?;
            Ok(TypeSpec::Pointer(Box::new(inner_ty)))
        }
        ExprKind::Deref(inner) => {
            let inner_ty = expr_type(inner, locals, typedefs, struct_layouts, globals)?;
            let resolved = resolve_typedefs(&inner_ty, typedefs)?;
            match resolved {
                TypeSpec::Pointer(inner) => Ok(*inner.clone()),
                _ => err!("dereferencing non-pointer type"),
            }
        }
    }
}

fn resolve_typedefs(
    ty: &TypeSpec,
    typedefs: &HashMap<String, TypeSpec>,
) -> Result<TypeSpec, LowerError> {
    let mut current = ty.clone();
    let mut depth = 0;
    while let TypeSpec::Named(name) = current {
        if depth > 100 {
            return err!("typedef loop detected for '{}'", name);
        }
        current = typedefs
            .get(&name)
            .ok_or_else(|| LowerError(format!("undefined typedef '{}'", name)))?
            .clone();
        depth += 1;
    }
    Ok(current)
}

fn get_struct_or_union_name(ty: &TypeSpec, typedefs: &HashMap<String, TypeSpec>) -> Option<String> {
    let mut current = ty.clone();
    let mut depth = 0;
    while let TypeSpec::Named(name) = current {
        if depth > 100 {
            return None;
        }
        if let Some(resolved) = typedefs.get(&name) {
            current = resolved.clone();
        } else {
            return None;
        }
        depth += 1;
    }
    match current {
        TypeSpec::Struct(name) | TypeSpec::Union(name) => Some(name),
        _ => None,
    }
}

fn eval_const_expr_lower(
    expr: &Expr,
    typedefs: &HashMap<String, TypeSpec>,
    struct_layouts: &HashMap<String, StructLayout>,
    enum_constants: &HashMap<String, i64>,
) -> Option<i64> {
    match &expr.kind {
        ExprKind::IntLit(val) => Some(*val),
        ExprKind::Ident(name) => enum_constants.get(name).copied(),
        ExprKind::Unary(UnaryOp::Neg, inner) => {
            eval_const_expr_lower(inner, typedefs, struct_layouts, enum_constants).map(|v| -v)
        }
        ExprKind::Unary(UnaryOp::Not, inner) => {
            eval_const_expr_lower(inner, typedefs, struct_layouts, enum_constants)
                .map(|v| if v == 0 { 1 } else { 0 })
        }
        ExprKind::Unary(UnaryOp::BitNot, inner) => {
            eval_const_expr_lower(inner, typedefs, struct_layouts, enum_constants).map(|v| !v)
        }
        ExprKind::Binary(op, lhs, rhs) => {
            let l = eval_const_expr_lower(lhs, typedefs, struct_layouts, enum_constants)?;
            let r = eval_const_expr_lower(rhs, typedefs, struct_layouts, enum_constants)?;
            match op {
                BinaryOp::Add => Some(l + r),
                BinaryOp::Sub => Some(l - r),
                BinaryOp::Mul => Some(l * r),
                BinaryOp::Div => {
                    if r != 0 {
                        Some(l / r)
                    } else {
                        None
                    }
                }
                BinaryOp::Mod => {
                    if r != 0 {
                        Some(l % r)
                    } else {
                        None
                    }
                }
                BinaryOp::BitAnd => Some(l & r),
                BinaryOp::BitOr => Some(l | r),
                BinaryOp::BitXor => Some(l ^ r),
                BinaryOp::Shl => Some(l << r),
                BinaryOp::Shr => Some(l >> r),
                _ => None,
            }
        }
        ExprKind::Sizeof(arg) => match arg {
            SizeofArg::Type(ts) => {
                let (sz, _) =
                    get_type_size_and_align(ts, typedefs, struct_layouts, enum_constants).ok()?;
                Some(sz as i64)
            }
            SizeofArg::Expr(_) => None,
        },
        ExprKind::Alignof(arg) => match arg {
            SizeofArg::Type(ts) => {
                let (_, aln) =
                    get_type_size_and_align(ts, typedefs, struct_layouts, enum_constants).ok()?;
                Some(aln as i64)
            }
            SizeofArg::Expr(_) => None,
        },
        _ => None,
    }
}
