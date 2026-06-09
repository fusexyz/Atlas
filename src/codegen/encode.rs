use super::machine::{Cond, Inst, Operand, Reg};
use std::collections::HashMap;

#[derive(Debug)]
pub struct EncodedFunction {
    pub name: String,
    pub bytes: Vec<u8>,
    pub labels: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct Reloc {
    pub offset: usize,
    pub symbol: String,
}

#[derive(Debug)]
pub struct EncodeError(pub String);
impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "encode error: {}", self.0)
    }
}
macro_rules! err { ($($t:tt)*) => { Err(EncodeError(format!($($t)*))) } }
type ER<T> = Result<T, EncodeError>;

struct Enc {
    buf: Vec<u8>,
    labels: HashMap<String, usize>,
    fixups: Vec<(usize, String)>,
    pub relocs: Vec<Reloc>,
}

impl Enc {
    fn new() -> Self {
        Self {
            buf: Vec::new(),
            labels: HashMap::new(),
            fixups: Vec::new(),
            relocs: Vec::new(),
        }
    }

    fn pos(&self) -> usize {
        self.buf.len()
    }
    fn emit(&mut self, byte: u8) {
        self.buf.push(byte);
    }
    fn emit_slice(&mut self, s: &[u8]) {
        self.buf.extend_from_slice(s);
    }
    fn emit_i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn emit_i64(&mut self, v: i64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn define_label(&mut self, name: &str) {
        self.labels.insert(name.to_string(), self.pos());
    }

    fn emit_label_ref(&mut self, label: &str) {
        let off = self.pos();
        self.fixups.push((off, label.to_string()));
        self.emit_i32(0);
    }

    fn emit_ripsym(&mut self, sym: &str) {
        let off = self.pos();
        self.relocs.push(Reloc {
            offset: off,
            symbol: sym.to_string(),
        });
        self.emit_i32(0);
    }

    fn patch_fixups(&mut self) -> ER<()> {
        for (off, label) in &self.fixups {
            let target = *self
                .labels
                .get(label.as_str())
                .ok_or_else(|| EncodeError(format!("undefined label '{label}'")))?;
            let rel32: i32 = (target as i64 - (*off as i64 + 4)) as i32;
            self.buf[*off..off + 4].copy_from_slice(&rel32.to_le_bytes());
        }
        Ok(())
    }
}

fn rex_w(r_ext: bool, b_ext: bool) -> u8 {
    0x48 | ((r_ext as u8) << 2) | (b_ext as u8)
}

fn rex_w_full(r_ext: bool, x_ext: bool, b_ext: bool) -> u8 {
    0x48 | ((r_ext as u8) << 2) | ((x_ext as u8) << 1) | (b_ext as u8)
}

fn modrm(mod_: u8, reg: u8, rm: u8) -> u8 {
    (mod_ << 6) | ((reg & 7) << 3) | (rm & 7)
}

fn encode_rr(enc: &mut Enc, opcode: u8, dst: Reg, src: Reg) {
    enc.emit(rex_w(dst.num() >= 8, src.num() >= 8));
    enc.emit(opcode);
    enc.emit(modrm(0b11, dst.num(), src.num()));
}

fn encode_rm_mem(enc: &mut Enc, opcode: u8, reg: Reg, base: Reg, disp: i32) {
    enc.emit(rex_w(reg.num() >= 8, base.num() >= 8));
    enc.emit(opcode);

    let needs_sib = (base.num() & 7) == 4;
    let needs_disp = (base.num() & 7) == 5 || disp != 0;
    let mod_ = if needs_disp && disp >= -128 && disp <= 127 {
        0b01
    } else if needs_disp {
        0b10
    } else {
        0b00
    };

    if needs_sib {
        enc.emit(modrm(mod_, reg.num(), 4));
        enc.emit(0x24);
    } else {
        enc.emit(modrm(mod_, reg.num(), base.num()));
    }

    if mod_ == 0b01 {
        enc.emit(disp as i8 as u8);
    } else if mod_ == 0b10 {
        enc.emit_i32(disp);
    }
}

fn encode_rm_rip(enc: &mut Enc, opcode: u8, reg: Reg, sym: &str) {
    enc.emit(rex_w(reg.num() >= 8, false));
    enc.emit(opcode);
    enc.emit(modrm(0b00, reg.num(), 5));
    enc.emit_ripsym(sym);
}

fn fits_i8(v: i64) -> bool {
    v >= -128 && v <= 127
}

fn encode_alu_reg_op(
    enc: &mut Enc,
    reg: Reg,
    src: &Operand,
    opcode_rr_mr: u8,
    slash_imm: u8,
) -> ER<()> {
    match src {
        Operand::Reg(sr) => encode_rr(enc, opcode_rr_mr, reg, *sr),
        Operand::Mem { base, disp } => encode_rm_mem(enc, opcode_rr_mr, reg, *base, *disp),
        Operand::Imm(v) => {
            enc.emit(rex_w(reg.num() >= 8, false));
            if fits_i8(*v) {
                enc.emit(0x83);
                enc.emit(modrm(0b11, slash_imm, reg.num()));
                enc.emit(*v as i8 as u8);
            } else {
                enc.emit(0x81);
                enc.emit(modrm(0b11, slash_imm, reg.num()));
                enc.emit_i32(*v as i32);
            }
        }
        Operand::RipSym(_) => return err!("RIP-sym not supported as ALU source"),
    }
    Ok(())
}

pub fn encode_function(name: &str, insts: &[Inst]) -> ER<(EncodedFunction, Vec<Reloc>)> {
    let mut enc = Enc::new();
    for inst in insts {
        encode_inst(&mut enc, inst)?;
    }
    enc.patch_fixups()?;
    let relocs = enc.relocs.clone();
    Ok((
        EncodedFunction {
            name: name.to_string(),
            bytes: enc.buf,
            labels: enc.labels,
        },
        relocs,
    ))
}

fn encode_inst(enc: &mut Enc, inst: &Inst) -> ER<()> {
    match inst {
        Inst::Label(name) => enc.define_label(name),

        Inst::Push(r) => {
            if r.num() >= 8 {
                enc.emit(0x41);
            }
            enc.emit(0x50 | (r.num() & 7));
        }
        Inst::Pop(r) => {
            if r.num() >= 8 {
                enc.emit(0x41);
            }
            enc.emit(0x58 | (r.num() & 7));
        }

        Inst::Mov(dst, src) => encode_mov(enc, dst, src)?,

        Inst::Lea(dst, src) => match src {
            Operand::Mem { base, disp } => encode_rm_mem(enc, 0x8D, *dst, *base, *disp),
            Operand::RipSym(sym) => encode_rm_rip(enc, 0x8D, *dst, sym),
            _ => return err!("lea: invalid source operand"),
        },

        Inst::Add(dst, src) => encode_alu_reg_op(enc, *dst, src, 0x03, 0)?,
        Inst::Sub(dst, src) => encode_alu_reg_op(enc, *dst, src, 0x2B, 5)?,
        Inst::And(dst, src) => encode_alu_reg_op(enc, *dst, src, 0x23, 4)?,
        Inst::Or(dst, src) => encode_alu_reg_op(enc, *dst, src, 0x0B, 1)?,
        Inst::Xor(dst, src) => encode_alu_reg_op(enc, *dst, src, 0x33, 6)?,

        Inst::Imul(dst, src) => match src {
            Operand::Reg(sr) => {
                enc.emit(rex_w(dst.num() >= 8, sr.num() >= 8));
                enc.emit(0x0F);
                enc.emit(0xAF);
                enc.emit(modrm(0b11, dst.num(), sr.num()));
            }
            Operand::Imm(v) => {
                enc.emit(rex_w(dst.num() >= 8, dst.num() >= 8));
                if fits_i8(*v) {
                    enc.emit(0x6B);
                    enc.emit(modrm(0b11, dst.num(), dst.num()));
                    enc.emit(*v as i8 as u8);
                } else {
                    enc.emit(0x69);
                    enc.emit(modrm(0b11, dst.num(), dst.num()));
                    enc.emit_i32(*v as i32);
                }
            }
            _ => return err!("imul: unsupported source"),
        },

        Inst::Neg(r) => {
            enc.emit(rex_w(r.num() >= 8, false));
            enc.emit(0xF7);
            enc.emit(modrm(0b11, 3, r.num()));
        }
        Inst::Not(r) => {
            enc.emit(rex_w(r.num() >= 8, false));
            enc.emit(0xF7);
            enc.emit(modrm(0b11, 2, r.num()));
        }
        Inst::Cqo => {
            enc.emit(0x48);
            enc.emit(0x99);
        }
        Inst::Idiv(src) => match src {
            Operand::Reg(r) => {
                enc.emit(rex_w(r.num() >= 8, false));
                enc.emit(0xF7);
                enc.emit(modrm(0b11, 7, r.num()));
            }
            _ => return err!("idiv: only register operand supported"),
        },

        Inst::ShlCl(r) => {
            enc.emit(rex_w(r.num() >= 8, false));
            enc.emit(0xD3);
            enc.emit(modrm(0b11, 4, r.num()));
        }
        Inst::SarCl(r) => {
            enc.emit(rex_w(r.num() >= 8, false));
            enc.emit(0xD3);
            enc.emit(modrm(0b11, 7, r.num()));
        }

        Inst::Cmp(lhs, rhs) => match (lhs, rhs) {
            (Operand::Reg(a), Operand::Reg(b)) => encode_rr(enc, 0x3B, *a, *b),
            (Operand::Reg(a), Operand::Imm(v)) => {
                enc.emit(rex_w(a.num() >= 8, false));
                if fits_i8(*v) {
                    enc.emit(0x83);
                    enc.emit(modrm(0b11, 7, a.num()));
                    enc.emit(*v as i8 as u8);
                } else {
                    enc.emit(0x81);
                    enc.emit(modrm(0b11, 7, a.num()));
                    enc.emit_i32(*v as i32);
                }
            }
            _ => return err!("cmp: unsupported operand combo"),
        },

        Inst::Setcc(cond, r) => {
            if r.num() >= 4 {
                enc.emit(0x40 | if r.num() >= 8 { 1 } else { 0 });
            }
            enc.emit(0x0F);
            enc.emit(setcc_opcode(*cond));
            enc.emit(modrm(0b11, 0, r.num()));
        }

        Inst::Movzx8(dst, src) => {
            enc.emit(rex_w_full(dst.num() >= 8, false, src.num() >= 8));
            enc.emit(0x0F);
            enc.emit(0xB6);
            enc.emit(modrm(0b11, dst.num(), src.num()));
        }

        Inst::Call(label) => {
            enc.emit(0xE8);
            enc.emit_ripsym(label);
        }
        Inst::CallImport(sym) => {
            enc.emit(0xFF);
            enc.emit(0x15);
            enc.emit_ripsym(sym);
        }
        Inst::CallReg(r) => {
            if r.num() >= 8 {
                enc.emit(0x41);
            }
            enc.emit(0xFF);
            enc.emit(modrm(0b11, 2, r.num()));
        }
        Inst::Jmp(label) => {
            enc.emit(0xE9);
            enc.emit_label_ref(label);
        }
        Inst::Jcc(cond, label) => {
            enc.emit(0x0F);
            enc.emit(jcc_opcode(*cond));
            enc.emit_label_ref(label);
        }
        Inst::Ret => enc.emit(0xC3),
    }
    Ok(())
}

fn encode_mov(enc: &mut Enc, dst: &Operand, src: &Operand) -> ER<()> {
    match (dst, src) {
        (Operand::Reg(d), Operand::Reg(s)) => encode_rr(enc, 0x8B, *d, *s),
        (Operand::Reg(d), Operand::Imm(v)) => {
            enc.emit(rex_w(false, d.num() >= 8));
            enc.emit(0xB8 | (d.num() & 7));
            enc.emit_i64(*v);
        }
        (Operand::Reg(d), Operand::Mem { base, disp }) => {
            encode_rm_mem(enc, 0x8B, *d, *base, *disp)
        }
        (Operand::Mem { base, disp }, Operand::Reg(s)) => {
            encode_rm_mem(enc, 0x89, *s, *base, *disp)
        }
        (Operand::Mem { base, disp }, Operand::Imm(v)) => {
            encode_rm_mem(enc, 0xC7, Reg::Rax, *base, *disp);
            enc.emit_i32(*v as i32);
        }
        (Operand::Reg(d), Operand::RipSym(sym)) => encode_rm_rip(enc, 0x8B, *d, sym),
        (Operand::RipSym(sym), Operand::Reg(s)) => encode_rm_rip(enc, 0x89, *s, sym),
        _ => return err!("mov: unsupported operand combination {dst:?}, {src:?}"),
    }
    Ok(())
}

fn setcc_opcode(c: Cond) -> u8 {
    match c {
        Cond::E => 0x94,
        Cond::Ne => 0x95,
        Cond::L => 0x9C,
        Cond::G => 0x9F,
        Cond::Le => 0x9E,
        Cond::Ge => 0x9D,
    }
}

fn jcc_opcode(c: Cond) -> u8 {
    match c {
        Cond::E => 0x84,
        Cond::Ne => 0x85,
        Cond::L => 0x8C,
        Cond::G => 0x8F,
        Cond::Le => 0x8E,
        Cond::Ge => 0x8D,
    }
}
