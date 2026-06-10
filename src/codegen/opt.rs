use super::machine::{Inst, Operand, Reg};
use std::collections::HashMap;

pub fn optimize_insts(insts: Vec<Inst>) -> Vec<Inst> {
    let mut current = insts;
    loop {
        let next = peephole(&current);
        if next == current {
            return next;
        }
        current = next;
    }
}

fn peephole(insts: &[Inst]) -> Vec<Inst> {
    let mut out = Vec::with_capacity(insts.len());
    let mut stack_values: HashMap<i32, Operand> = HashMap::new();

    for inst in insts.iter().cloned() {
        match inst {
            Inst::Mov(Operand::Reg(dst), Operand::Reg(src)) if dst == src => {}
            Inst::Mov32(Operand::Reg(dst), Operand::Reg(src)) if dst == src => {}
            Inst::Mov(
                Operand::Reg(dst),
                Operand::Mem {
                    base: Reg::Rbp,
                    disp,
                },
            ) => {
                if let Some(src) = stack_values.get(&disp).cloned() {
                    invalidate_reg(&mut stack_values, dst);
                    if src != Operand::Reg(dst) {
                        out.push(Inst::Mov(Operand::Reg(dst), src));
                    }
                } else {
                    invalidate_reg(&mut stack_values, dst);
                    out.push(Inst::Mov(
                        Operand::Reg(dst),
                        Operand::Mem {
                            base: Reg::Rbp,
                            disp,
                        },
                    ));
                }
            }
            Inst::Mov(
                Operand::Mem {
                    base: Reg::Rbp,
                    disp,
                },
                src,
            ) => {
                stack_values.insert(disp, src.clone());
                out.push(Inst::Mov(
                    Operand::Mem {
                        base: Reg::Rbp,
                        disp,
                    },
                    src,
                ));
            }
            other => {
                update_state(&mut stack_values, &other);
                out.push(other);
            }
        }
    }

    out
}

fn update_state(stack_values: &mut HashMap<i32, Operand>, inst: &Inst) {
    match inst {
        Inst::Label(_)
        | Inst::Jmp(_)
        | Inst::Jcc(_, _)
        | Inst::Call(_)
        | Inst::CallImport(_)
        | Inst::CallReg(_) => {
            stack_values.clear();
        }
        Inst::Lea(reg, _)
        | Inst::Add(reg, _)
        | Inst::Sub(reg, _)
        | Inst::Imul(reg, _)
        | Inst::And(reg, _)
        | Inst::Or(reg, _)
        | Inst::Xor(reg, _)
        | Inst::ShlCl(reg)
        | Inst::SarCl(reg)
        | Inst::Neg(reg)
        | Inst::Not(reg)
        | Inst::Setcc(_, reg)
        | Inst::Movzx8(reg, _)
        | Inst::Movzx16(reg, _) => invalidate_reg(stack_values, *reg),
        Inst::Mov(Operand::Reg(reg), _)
        | Inst::Mov8(Operand::Reg(reg), _)
        | Inst::Mov16(Operand::Reg(reg), _)
        | Inst::Mov32(Operand::Reg(reg), _) => invalidate_reg(stack_values, *reg),
        Inst::Mov(
            Operand::Mem {
                base: Reg::Rbp,
                disp,
            },
            _,
        )
        | Inst::Mov8(
            Operand::Mem {
                base: Reg::Rbp,
                disp,
            },
            _,
        )
        | Inst::Mov16(
            Operand::Mem {
                base: Reg::Rbp,
                disp,
            },
            _,
        )
        | Inst::Mov32(
            Operand::Mem {
                base: Reg::Rbp,
                disp,
            },
            _,
        ) => {
            stack_values.remove(disp);
        }
        Inst::Cqo | Inst::Idiv(_) => {
            invalidate_reg(stack_values, Reg::Rax);
            invalidate_reg(stack_values, Reg::Rdx);
        }
        Inst::Cmp(_, _) | Inst::Push(_) | Inst::Pop(_) | Inst::Ret => {}
        _ => {}
    }
}

fn invalidate_reg(stack_values: &mut HashMap<i32, Operand>, reg: Reg) {
    stack_values.retain(|_, value| value != &Operand::Reg(reg));
}
