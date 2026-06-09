use super::ir::{self as ir, *};
use crate::parser::ast::{self, *};
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

pub fn lower_module(tu: &TranslationUnit) -> LR<Module> {
    let mut ctx = ModuleCtx::new();
    for item in &tu.items {
        ctx.lower_top_level(item)?;
    }
    Ok(ctx.module)
}

struct ModuleCtx {
    module: Module,
    func_sigs: HashMap<String, (Vec<IrType>, IrType)>,
    str_count: u32,
}

impl ModuleCtx {
    fn new() -> Self {
        Self {
            module: Module::default(),
            func_sigs: HashMap::new(),
            str_count: 0,
        }
    }

    fn lower_top_level(&mut self, item: &TopLevel) -> LR<()> {
        match item {
            TopLevel::Function(f) => self.lower_function(f),
            TopLevel::FuncDecl(d) => self.lower_func_decl(d),
            TopLevel::Declaration(d) => self.lower_global_decl(d),
            TopLevel::StructDef(_) => Ok(()),
            TopLevel::Typedef(_, _, _) => Ok(()),
        }
    }

    fn lower_func_decl(&mut self, decl: &FuncDecl) -> LR<()> {
        let ret_ty = lower_type(&decl.ret_ty)?;
        let param_tys: Vec<IrType> = decl
            .params
            .iter()
            .map(|p| lower_type(&p.ty))
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
        });
        Ok(())
    }

    fn lower_global_decl(&mut self, decl: &Decl) -> LR<()> {
        let ty = lower_type(&decl.ty)?;
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
        let ret_ty = lower_type(&func.ret_ty)?;
        let mut params = Vec::new();
        let mut param_tys = Vec::new();
        let mut next_reg = 0u32;

        for p in &func.params {
            let ty = lower_type(&p.ty)?;
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
            blocks: Vec::new(),
            next_reg,
            next_block: 0,
            locals: HashMap::new(),
            func_sigs: &self.func_sigs,
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
                fb.locals.insert(name.to_string(), (slot, p.ty.clone()));
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
    blocks: Vec<BasicBlock>,
    next_reg: u32,
    next_block: u32,
    locals: HashMap<String, (VReg, IrType)>,
    func_sigs: &'a HashMap<String, (Vec<IrType>, IrType)>,
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
        let ty = lower_type(&decl.ty)?;
        let slot = self.emit_alloca(bb, ty.clone());
        self.locals.insert(decl.name.clone(), (slot, ty.clone()));

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
                if let Some((slot, ty)) = self.locals.get(name).cloned() {
                    let dst = self.fresh();
                    self.emit(
                        bb,
                        Instr::Load {
                            dst,
                            ty,
                            ptr: Value::Reg(slot),
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
                let to_ty = lower_type(ty)?;
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
                    SizeofArg::Type(ty) => lower_type(ty)?.size_bytes(),
                    SizeofArg::Expr(e) => match &e.kind {
                        ExprKind::Ident(name) => self
                            .locals
                            .get(name)
                            .map(|(_, ty)| ty.size_bytes())
                            .unwrap_or(8),
                        _ => 8,
                    },
                };
                Ok(Value::ImmI(size as i64))
            }

            ExprKind::Index(base, idx) => {
                let ptr = self.gep_ptr(base, idx, bb)?;
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

            ExprKind::Member(_, _) | ExprKind::Arrow(_, _) => {
                err!("struct member access not yet implemented")
            }

            ExprKind::Binary(_, _, _) => unreachable!(),
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
            _ => err!("not a valid lvalue"),
        }
    }

    fn lvalue_type(&self, expr: &Expr) -> LR<IrType> {
        match &expr.kind {
            ExprKind::Ident(name) => {
                if let Some((_, ty)) = self.locals.get(name) {
                    Ok(ty.clone())
                } else {
                    Ok(IrType::I64)
                }
            }
            ExprKind::Deref(_) => Ok(IrType::I64),
            ExprKind::Index(_, _) => Ok(IrType::I64),
            _ => Ok(IrType::I64),
        }
    }

    fn gep_ptr(&mut self, base: &Expr, idx: &Expr, bb: BlockId) -> LR<Value> {
        let base_ptr = self.lvalue_ptr(base, bb)?;
        let idx_val = self.lower_expr(idx, bb)?;
        let dst = self.fresh();
        self.emit(
            bb,
            Instr::Gep {
                dst,
                elem_ty: IrType::I64,
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
        };
        Ok((func, self.string_lits, self.next_str))
    }
}

pub fn lower_type(ty: &TypeSpec) -> LR<IrType> {
    match ty {
        TypeSpec::Void => Ok(IrType::Void),
        TypeSpec::Char => Ok(IrType::I8),
        TypeSpec::Short => Ok(IrType::I16),
        TypeSpec::Int => Ok(IrType::I32),
        TypeSpec::Long | TypeSpec::LongLong => Ok(IrType::I64),
        TypeSpec::Float => Ok(IrType::F32),
        TypeSpec::Double => Ok(IrType::F64),
        TypeSpec::Unsigned(inner) => lower_type(inner),
        TypeSpec::Signed(inner) => lower_type(inner),
        TypeSpec::Pointer(inner) => Ok(IrType::Ptr(Box::new(lower_type(inner)?))),
        TypeSpec::Array(elem, Some(size_expr)) => {
            let elem_ty = lower_type(elem)?;
            let n = match &size_expr.kind {
                ExprKind::IntLit(v) => *v as u64,
                _ => return err!("array size must be a constant integer"),
            };
            Ok(IrType::Array(Box::new(elem_ty), n))
        }
        TypeSpec::Array(elem, None) => Ok(IrType::Ptr(Box::new(lower_type(elem)?))),
        TypeSpec::Struct(name) => err!("struct type '{name}' not yet fully lowered"),
        TypeSpec::Union(name) => err!("union type '{name}' not yet fully lowered"),
        TypeSpec::Enum(_) => Ok(IrType::I32),
        TypeSpec::Named(name) => err!("typedef '{name}' not resolved"),
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
