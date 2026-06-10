#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum Reg {
    Rax,
    Rcx,
    Rdx,
    Rbx,
    Rsp,
    Rbp,
    Rsi,
    Rdi,
    R8,
    R9,
    R10,
    R11,
    R12,
    R13,
    R14,
    R15,
}

impl Reg {
    pub fn num(self) -> u8 {
        use Reg::*;
        match self {
            Rax => 0,
            Rcx => 1,
            Rdx => 2,
            Rbx => 3,
            Rsp => 4,
            Rbp => 5,
            Rsi => 6,
            Rdi => 7,
            R8 => 8,
            R9 => 9,
            R10 => 10,
            R11 => 11,
            R12 => 12,
            R13 => 13,
            R14 => 14,
            R15 => 15,
        }
    }

    pub fn name64(self) -> &'static str {
        use Reg::*;
        match self {
            Rax => "rax",
            Rcx => "rcx",
            Rdx => "rdx",
            Rbx => "rbx",
            Rsp => "rsp",
            Rbp => "rbp",
            Rsi => "rsi",
            Rdi => "rdi",
            R8 => "r8",
            R9 => "r9",
            R10 => "r10",
            R11 => "r11",
            R12 => "r12",
            R13 => "r13",
            R14 => "r14",
            R15 => "r15",
        }
    }

    pub fn name8(self) -> &'static str {
        use Reg::*;
        match self {
            Rax => "al",
            Rcx => "cl",
            Rdx => "dl",
            Rbx => "bl",
            Rsp => "spl",
            Rbp => "bpl",
            Rsi => "sil",
            Rdi => "dil",
            R8 => "r8b",
            R9 => "r9b",
            R10 => "r10b",
            R11 => "r11b",
            R12 => "r12b",
            R13 => "r13b",
            R14 => "r14b",
            R15 => "r15b",
        }
    }

    pub fn name16(self) -> &'static str {
        use Reg::*;
        match self {
            Rax => "ax",
            Rcx => "cx",
            Rdx => "dx",
            Rbx => "bx",
            Rsp => "sp",
            Rbp => "bp",
            Rsi => "si",
            Rdi => "di",
            R8 => "r8w",
            R9 => "r9w",
            R10 => "r10w",
            R11 => "r11w",
            R12 => "r12w",
            R13 => "r13w",
            R14 => "r14w",
            R15 => "r15w",
        }
    }

    pub fn name32(self) -> &'static str {
        use Reg::*;
        match self {
            Rax => "eax",
            Rcx => "ecx",
            Rdx => "edx",
            Rbx => "ebx",
            Rsp => "esp",
            Rbp => "ebp",
            Rsi => "esi",
            Rdi => "edi",
            R8 => "r8d",
            R9 => "r9d",
            R10 => "r10d",
            R11 => "r11d",
            R12 => "r12d",
            R13 => "r13d",
            R14 => "r14d",
            R15 => "r15d",
        }
    }
}

pub const ARG_REGS: [Reg; 4] = [Reg::Rcx, Reg::Rdx, Reg::R8, Reg::R9];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    E,
    Ne,
    L,
    G,
    Le,
    Ge,
}

impl Cond {
    pub fn suffix(self) -> &'static str {
        match self {
            Cond::E => "e",
            Cond::Ne => "ne",
            Cond::L => "l",
            Cond::G => "g",
            Cond::Le => "le",
            Cond::Ge => "ge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Operand {
    Reg(Reg),
    Imm(i64),
    Mem { base: Reg, disp: i32 },
    RipSym(String),
}

impl Operand {
    pub fn mem(base: Reg, disp: i32) -> Operand {
        Operand::Mem { base, disp }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inst {
    Label(String),
    Push(Reg),
    Pop(Reg),
    Mov(Operand, Operand),
    Lea(Reg, Operand),
    Add(Reg, Operand),
    Sub(Reg, Operand),
    Imul(Reg, Operand),
    And(Reg, Operand),
    Or(Reg, Operand),
    Xor(Reg, Operand),
    ShlCl(Reg),
    SarCl(Reg),
    Neg(Reg),
    Not(Reg),
    Cqo,
    Idiv(Operand),
    Cmp(Operand, Operand),
    Setcc(Cond, Reg),
    Movzx8(Reg, Operand),
    Movzx16(Reg, Operand),
    Mov8(Operand, Operand),
    Mov16(Operand, Operand),
    Mov32(Operand, Operand),
    Call(String),
    CallImport(String),
    CallReg(Reg),
    Jmp(String),
    Jcc(Cond, String),
    Ret,
}

impl std::fmt::Display for Operand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Operand::Reg(r) => write!(f, "{}", r.name64()),
            Operand::Imm(v) => write!(f, "{v}"),
            Operand::Mem { base, disp } => {
                if *disp == 0 {
                    write!(f, "[{}]", base.name64())
                } else if *disp > 0 {
                    write!(f, "[{}+{}]", base.name64(), disp)
                } else {
                    write!(f, "[{}-{}]", base.name64(), -(*disp as i64))
                }
            }
            Operand::RipSym(s) => write!(f, "[rip+{s}]"),
        }
    }
}

impl std::fmt::Display for Inst {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Inst::Label(name) => write!(f, "{name}:"),
            Inst::Push(r) => write!(f, "    push {}", r.name64()),
            Inst::Pop(r) => write!(f, "    pop {}", r.name64()),
            Inst::Mov(d, s) => write!(f, "    mov {d}, {s}"),
            Inst::Lea(d, s) => write!(f, "    lea {}, {s}", d.name64()),
            Inst::Add(d, s) => write!(f, "    add {}, {s}", d.name64()),
            Inst::Sub(d, s) => write!(f, "    sub {}, {s}", d.name64()),
            Inst::Imul(d, s) => write!(f, "    imul {}, {s}", d.name64()),
            Inst::And(d, s) => write!(f, "    and {}, {s}", d.name64()),
            Inst::Or(d, s) => write!(f, "    or {}, {s}", d.name64()),
            Inst::Xor(d, s) => write!(f, "    xor {}, {s}", d.name64()),
            Inst::ShlCl(r) => write!(f, "    shl {}, cl", r.name64()),
            Inst::SarCl(r) => write!(f, "    sar {}, cl", r.name64()),
            Inst::Neg(r) => write!(f, "    neg {}", r.name64()),
            Inst::Not(r) => write!(f, "    not {}", r.name64()),
            Inst::Cqo => write!(f, "    cqo"),
            Inst::Idiv(s) => write!(f, "    idiv {s}"),
            Inst::Cmp(a, b) => write!(f, "    cmp {a}, {b}"),
            Inst::Setcc(c, r) => write!(f, "    set{} {}", c.suffix(), r.name8()),
            Inst::Movzx8(d, s) => match s {
                Operand::Reg(r) => write!(f, "    movzx {}, {}", d.name64(), r.name8()),
                mem => write!(f, "    movzx {}, byte ptr {}", d.name64(), mem),
            },
            Inst::Movzx16(d, s) => match s {
                Operand::Reg(r) => write!(f, "    movzx {}, {}", d.name64(), r.name16()),
                mem => write!(f, "    movzx {}, word ptr {}", d.name64(), mem),
            },
            Inst::Mov8(d, s) => match (d, s) {
                (dest, Operand::Reg(r)) => write!(f, "    mov byte ptr {}, {}", dest, r.name8()),
                (dest, src) => write!(f, "    mov byte ptr {}, {}", dest, src),
            },
            Inst::Mov16(d, s) => match (d, s) {
                (dest, Operand::Reg(r)) => write!(f, "    mov word ptr {}, {}", dest, r.name16()),
                (dest, src) => write!(f, "    mov word ptr {}, {}", dest, src),
            },
            Inst::Mov32(d, s) => match (d, s) {
                (Operand::Reg(r1), Operand::Reg(r2)) => {
                    write!(f, "    mov {}, {}", r1.name32(), r2.name32())
                }
                (Operand::Reg(r), mem) => write!(f, "    mov {}, dword ptr {}", r.name32(), mem),
                (mem, Operand::Reg(r)) => write!(f, "    mov dword ptr {}, {}", mem, r.name32()),
                (dest, src) => write!(f, "    mov {}, {}", dest, src),
            },
            Inst::Call(s) => write!(f, "    call {s}"),
            Inst::CallImport(s) => write!(f, "    call qword ptr [rip+{s}]"),
            Inst::CallReg(r) => write!(f, "    call {}", r.name64()),
            Inst::Jmp(s) => write!(f, "    jmp {s}"),
            Inst::Jcc(c, s) => write!(f, "    j{} {s}", c.suffix()),
            Inst::Ret => write!(f, "    ret"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MachineFunction {
    pub name: String,
    pub insts: Vec<Inst>,
}

impl std::fmt::Display for MachineFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for inst in &self.insts {
            writeln!(f, "{inst}")?;
        }
        Ok(())
    }
}
