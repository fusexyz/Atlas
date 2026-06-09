use super::machine::*;
use crate::ir::ir::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct CodegenError(pub String);

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codegen error: {}", self.0)
    }
}

macro_rules! err {
    ($($t:tt)*) => { Err(CodegenError(format!($($t)*))) };
}

type CR<T> = Result<T, CodegenError>;

const RAX: Reg = Reg::Rax;
const RCX: Reg = Reg::Rcx;
const RDX: Reg = Reg::Rdx;
const RBP: Reg = Reg::Rbp;
const RSP: Reg = Reg::Rsp;

pub fn compile_module(module: &Module) -> CR<Vec<MachineFunction>> {
    let extern_funcs: HashSet<String> = module
        .functions
        .iter()
        .filter(|f| f.is_extern)
        .map(|f| f.name.clone())
        .collect();
    let mut out = Vec::new();
    for func in &module.functions {
        if func.is_extern {
            continue;
        }
        out.push(compile_function(func, &extern_funcs)?);
    }
    Ok(out)
}

fn round_up(x: i64, align: i64) -> i64 {
    (x + align - 1) / align * align
}

fn vreg_count(func: &Function) -> u32 {
    let mut max = 0u32;
    let mut bump = |v: u32| {
        if v + 1 > max {
            max = v + 1;
        }
    };

    for p in &func.params {
        bump(p.reg.0);
    }
    let mut scan_val = |v: &Value, bump: &mut dyn FnMut(u32)| {
        if let Value::Reg(r) = v {
            bump(r.0);
        }
    };
    for bb in &func.blocks {
        for inst in &bb.instrs {
            match inst {
                Instr::Alloca { dst, .. } => bump(dst.0),
                Instr::Load { dst, ptr, .. } => {
                    bump(dst.0);
                    scan_val(ptr, &mut bump);
                }
                Instr::Store { val, ptr, .. } => {
                    scan_val(val, &mut bump);
                    scan_val(ptr, &mut bump);
                }
                Instr::BinOp { dst, lhs, rhs, .. } => {
                    bump(dst.0);
                    scan_val(lhs, &mut bump);
                    scan_val(rhs, &mut bump);
                }
                Instr::UnaryOp { dst, val, .. } => {
                    bump(dst.0);
                    scan_val(val, &mut bump);
                }
                Instr::Call {
                    dst, func, args, ..
                } => {
                    if let Some(d) = dst {
                        bump(d.0);
                    }
                    scan_val(func, &mut bump);
                    for a in args {
                        scan_val(a, &mut bump);
                    }
                }
                Instr::Gep { dst, base, idx, .. } => {
                    bump(dst.0);
                    scan_val(base, &mut bump);
                    scan_val(idx, &mut bump);
                }
                Instr::Cast { dst, val, .. } => {
                    bump(dst.0);
                    scan_val(val, &mut bump);
                }
                Instr::Copy { dst, val } => {
                    bump(dst.0);
                    scan_val(val, &mut bump);
                }
            }
        }
        match &bb.term {
            Terminator::Ret(Some(v)) => scan_val(v, &mut bump),
            Terminator::CondBr { cond, .. } => scan_val(cond, &mut bump),
            _ => {}
        }
    }
    max
}

struct FrameLayout {
    slot_off: Vec<i32>,
    alloca_off: HashMap<u32, i32>,
    frame_size: i64,
}

fn compute_layout(func: &Function) -> FrameLayout {
    let nslots = vreg_count(func) as i64;
    let slot_off: Vec<i32> = (0..nslots).map(|k| -((k + 1) * 8) as i32).collect();

    let mut next_local = -(nslots * 8);
    let mut alloca_off = HashMap::new();
    for bb in &func.blocks {
        for inst in &bb.instrs {
            if let Instr::Alloca { dst, ty } = inst {
                let size = round_up(ty.size_bytes() as i64, 8);
                next_local -= size;
                alloca_off.insert(dst.0, next_local as i32);
            }
        }
    }

    let named_bytes = -next_local;

    let mut max_call_bytes = 0i64;
    for bb in &func.blocks {
        for inst in &bb.instrs {
            if let Instr::Call { args, .. } = inst {
                let bytes = (8 * args.len() as i64).max(32);
                max_call_bytes = max_call_bytes.max(bytes);
            }
        }
    }
    let out_bytes = round_up(max_call_bytes, 16);
    let frame_size = round_up(named_bytes + out_bytes, 16);

    FrameLayout {
        slot_off,
        alloca_off,
        frame_size,
    }
}

struct Gen<'a> {
    func: &'a Function,
    layout: FrameLayout,
    insts: Vec<Inst>,
    epilogue: String,
    extern_funcs: &'a HashSet<String>,
}

impl<'a> Gen<'a> {
    fn block_label(&self, bb: BlockId) -> String {
        format!(".L{}_bb{}", self.func.name, bb.0)
    }

    fn slot(&self, reg: VReg) -> i32 {
        self.layout.slot_off[reg.0 as usize]
    }

    fn push(&mut self, inst: Inst) {
        self.insts.push(inst);
    }

    fn load_value(&mut self, val: &Value, dst: Reg) -> CR<()> {
        match val {
            Value::ImmI(n) => self.push(Inst::Mov(Operand::Reg(dst), Operand::Imm(*n))),
            Value::Reg(vreg) => self.push(Inst::Mov(
                Operand::Reg(dst),
                Operand::mem(RBP, self.slot(*vreg)),
            )),
            Value::Global(name) => self.push(Inst::Lea(dst, Operand::RipSym(name.clone()))),
            Value::ImmF(_) => return err!("floating-point not yet supported in codegen"),
        }
        Ok(())
    }

    fn store_reg(&mut self, dst: VReg, src: Reg) {
        let off = self.slot(dst);
        self.push(Inst::Mov(Operand::mem(RBP, off), Operand::Reg(src)));
    }

    fn gen_function(&mut self) -> CR<()> {
        self.push(Inst::Label(self.func.name.clone()));
        self.push(Inst::Push(RBP));
        self.push(Inst::Mov(Operand::Reg(RBP), Operand::Reg(RSP)));
        let fs = self.layout.frame_size;
        if fs > 0 {
            self.push(Inst::Sub(RSP, Operand::Imm(fs)));
        }

        if self.func.params.len() > 4 {
            return err!("functions with more than 4 parameters are not yet supported");
        }
        for (i, p) in self.func.params.iter().enumerate() {
            let off = self.slot(p.reg);
            self.push(Inst::Mov(Operand::mem(RBP, off), Operand::Reg(ARG_REGS[i])));
        }

        for bb in &self.func.blocks {
            let label = self.block_label(bb.id);
            self.push(Inst::Label(label));
            for inst in &bb.instrs {
                self.gen_inst(inst)?;
            }
            self.gen_term(&bb.term)?;
        }

        self.push(Inst::Label(self.epilogue.clone()));
        self.push(Inst::Mov(Operand::Reg(RSP), Operand::Reg(RBP)));
        self.push(Inst::Pop(RBP));
        self.push(Inst::Ret);
        Ok(())
    }

    fn gen_inst(&mut self, inst: &Instr) -> CR<()> {
        match inst {
            Instr::Alloca { dst, .. } => {
                let off = *self.layout.alloca_off.get(&dst.0).ok_or_else(|| {
                    CodegenError(format!("missing alloca storage for %{}", dst.0))
                })?;
                self.push(Inst::Lea(RAX, Operand::mem(RBP, off)));
                self.store_reg(*dst, RAX);
            }

            Instr::Load { dst, ptr, .. } => {
                self.load_value(ptr, RAX)?;
                self.push(Inst::Mov(Operand::Reg(RCX), Operand::mem(RAX, 0)));
                self.store_reg(*dst, RCX);
            }

            Instr::Store { val, ptr, .. } => {
                self.load_value(ptr, RAX)?;
                self.load_value(val, RCX)?;
                self.push(Inst::Mov(Operand::mem(RAX, 0), Operand::Reg(RCX)));
            }

            Instr::BinOp {
                dst, op, lhs, rhs, ..
            } => {
                self.load_value(lhs, RAX)?;
                self.load_value(rhs, RCX)?;
                self.gen_binop(*op);
                self.store_reg(*dst, RAX);
            }

            Instr::UnaryOp { dst, op, val, .. } => {
                self.load_value(val, RAX)?;
                match op {
                    UnaryOpKind::Neg => self.push(Inst::Neg(RAX)),
                    UnaryOpKind::BitNot => self.push(Inst::Not(RAX)),
                    UnaryOpKind::Not => {
                        self.push(Inst::Cmp(Operand::Reg(RAX), Operand::Imm(0)));
                        self.push(Inst::Setcc(Cond::E, RAX));
                        self.push(Inst::Movzx8(RAX, RAX));
                    }
                }
                self.store_reg(*dst, RAX);
            }

            Instr::Call {
                dst, func, args, ..
            } => {
                if args.len() > 4 {
                    for (i, a) in args.iter().enumerate().skip(4) {
                        self.load_value(a, RAX)?;
                        self.push(Inst::Mov(
                            Operand::mem(RSP, (8 * i) as i32),
                            Operand::Reg(RAX),
                        ));
                    }
                }
                for (i, a) in args.iter().enumerate().take(4) {
                    self.load_value(a, ARG_REGS[i])?;
                }
                match func {
                    Value::Global(name) if self.extern_funcs.contains(name.as_str()) => {
                        self.push(Inst::CallImport(name.clone()))
                    }
                    Value::Global(name) => self.push(Inst::Call(name.clone())),
                    other => {
                        self.load_value(other, RAX)?;
                        self.push(Inst::CallReg(RAX));
                    }
                }
                if let Some(d) = dst {
                    self.store_reg(*d, RAX);
                }
            }

            Instr::Gep {
                dst,
                elem_ty,
                base,
                idx,
            } => {
                self.load_value(base, RAX)?;
                self.load_value(idx, RCX)?;
                let size = elem_ty.size_bytes() as i64;
                if size != 1 {
                    self.push(Inst::Imul(RCX, Operand::Imm(size)));
                }
                self.push(Inst::Add(RAX, Operand::Reg(RCX)));
                self.store_reg(*dst, RAX);
            }

            Instr::Cast { dst, val, .. } => {
                self.load_value(val, RAX)?;
                self.store_reg(*dst, RAX);
            }

            Instr::Copy { dst, val } => {
                self.load_value(val, RAX)?;
                self.store_reg(*dst, RAX);
            }
        }
        Ok(())
    }

    fn gen_binop(&mut self, op: BinOpKind) {
        match op {
            BinOpKind::Add => self.push(Inst::Add(RAX, Operand::Reg(RCX))),
            BinOpKind::Sub => self.push(Inst::Sub(RAX, Operand::Reg(RCX))),
            BinOpKind::Mul => self.push(Inst::Imul(RAX, Operand::Reg(RCX))),
            BinOpKind::Div => {
                self.push(Inst::Cqo);
                self.push(Inst::Idiv(Operand::Reg(RCX)));
            }
            BinOpKind::Rem => {
                self.push(Inst::Cqo);
                self.push(Inst::Idiv(Operand::Reg(RCX)));
                self.push(Inst::Mov(Operand::Reg(RAX), Operand::Reg(RDX)));
            }
            BinOpKind::And => self.push(Inst::And(RAX, Operand::Reg(RCX))),
            BinOpKind::Or => self.push(Inst::Or(RAX, Operand::Reg(RCX))),
            BinOpKind::Xor => self.push(Inst::Xor(RAX, Operand::Reg(RCX))),
            BinOpKind::Shl => self.push(Inst::ShlCl(RAX)),
            BinOpKind::Shr => self.push(Inst::SarCl(RAX)),
            BinOpKind::Eq
            | BinOpKind::Ne
            | BinOpKind::Lt
            | BinOpKind::Gt
            | BinOpKind::Le
            | BinOpKind::Ge => {
                let cond = match op {
                    BinOpKind::Eq => Cond::E,
                    BinOpKind::Ne => Cond::Ne,
                    BinOpKind::Lt => Cond::L,
                    BinOpKind::Gt => Cond::G,
                    BinOpKind::Le => Cond::Le,
                    BinOpKind::Ge => Cond::Ge,
                    _ => unreachable!(),
                };
                self.push(Inst::Cmp(Operand::Reg(RAX), Operand::Reg(RCX)));
                self.push(Inst::Setcc(cond, RAX));
                self.push(Inst::Movzx8(RAX, RAX));
            }
        }
    }

    fn gen_term(&mut self, term: &Terminator) -> CR<()> {
        match term {
            Terminator::Ret(v) => {
                if let Some(val) = v {
                    self.load_value(val, RAX)?;
                }
                self.push(Inst::Jmp(self.epilogue.clone()));
            }
            Terminator::Br(bb) => {
                self.push(Inst::Jmp(self.block_label(*bb)));
            }
            Terminator::CondBr {
                cond,
                then_bb,
                else_bb,
            } => {
                self.load_value(cond, RAX)?;
                self.push(Inst::Cmp(Operand::Reg(RAX), Operand::Imm(0)));
                self.push(Inst::Jcc(Cond::Ne, self.block_label(*then_bb)));
                self.push(Inst::Jmp(self.block_label(*else_bb)));
            }
            Terminator::Unreachable => {}
        }
        Ok(())
    }
}

fn compile_function(func: &Function, extern_funcs: &HashSet<String>) -> CR<MachineFunction> {
    let layout = compute_layout(func);
    let epilogue = format!(".L{}_epilogue", func.name);
    let mut g = Gen {
        func,
        layout,
        insts: Vec::new(),
        epilogue,
        extern_funcs,
    };
    g.gen_function()?;
    Ok(MachineFunction {
        name: func.name.clone(),
        insts: g.insts,
    })
}
