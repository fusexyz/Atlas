#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VReg(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub enum IrType {
    Void,
    I8,
    I16,
    I32,
    I64,
    F32,
    F64,
    Ptr(Box<IrType>),
    Array(Box<IrType>, u64),
}

impl IrType {
    pub fn size_bytes(&self) -> u64 {
        match self {
            IrType::Void => 0,
            IrType::I8 => 1,
            IrType::I16 => 2,
            IrType::I32 => 4,
            IrType::I64 => 8,
            IrType::F32 => 4,
            IrType::F64 => 8,
            IrType::Ptr(_) => 8,
            IrType::Array(e, n) => e.size_bytes() * n,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, IrType::I8 | IrType::I16 | IrType::I32 | IrType::I64)
    }
}

impl std::fmt::Display for IrType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrType::Void => write!(f, "void"),
            IrType::I8 => write!(f, "i8"),
            IrType::I16 => write!(f, "i16"),
            IrType::I32 => write!(f, "i32"),
            IrType::I64 => write!(f, "i64"),
            IrType::F32 => write!(f, "f32"),
            IrType::F64 => write!(f, "f64"),
            IrType::Ptr(inner) => write!(f, "*{inner}"),
            IrType::Array(elem, n) => write!(f, "[{n} x {elem}]"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Reg(VReg),
    ImmI(i64),
    ImmF(f64),
    Global(String),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Reg(VReg(n)) => write!(f, "%{n}"),
            Value::ImmI(v) => write!(f, "{v}"),
            Value::ImmF(v) => write!(f, "{v}"),
            Value::Global(name) => write!(f, "@{name}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOpKind {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone)]
pub enum Instr {
    Alloca {
        dst: VReg,
        ty: IrType,
    },
    Load {
        dst: VReg,
        ty: IrType,
        ptr: Value,
    },
    Store {
        ty: IrType,
        val: Value,
        ptr: Value,
    },
    BinOp {
        dst: VReg,
        op: BinOpKind,
        ty: IrType,
        lhs: Value,
        rhs: Value,
    },
    UnaryOp {
        dst: VReg,
        op: UnaryOpKind,
        ty: IrType,
        val: Value,
    },
    Call {
        dst: Option<VReg>,
        func: Value,
        args: Vec<Value>,
        ret_ty: IrType,
    },
    Gep {
        dst: VReg,
        elem_ty: IrType,
        base: Value,
        idx: Value,
    },
    Cast {
        dst: VReg,
        from_ty: IrType,
        to_ty: IrType,
        val: Value,
    },
    Copy {
        dst: VReg,
        val: Value,
    },
}

#[derive(Debug, Clone)]
pub enum Terminator {
    Ret(Option<Value>),
    Br(BlockId),
    CondBr {
        cond: Value,
        then_bb: BlockId,
        else_bb: BlockId,
    },
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct BasicBlock {
    pub id: BlockId,
    pub instrs: Vec<Instr>,
    pub term: Terminator,
}

impl BasicBlock {
    pub fn new(id: BlockId) -> Self {
        Self {
            id,
            instrs: Vec::new(),
            term: Terminator::Unreachable,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub ty: IrType,
    pub reg: VReg,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_ty: IrType,
    pub blocks: Vec<BasicBlock>,
    pub is_extern: bool,
}

impl Function {
    pub fn entry(&self) -> BlockId {
        self.blocks[0].id
    }
}

#[derive(Debug, Clone)]
pub struct Global {
    pub name: String,
    pub ty: IrType,
    pub init: Option<Value>,
    pub is_extern: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Module {
    pub functions: Vec<Function>,
    pub globals: Vec<Global>,
    pub string_lits: Vec<(String, Vec<u8>)>,
}

impl std::fmt::Display for Module {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for g in &self.globals {
            if g.is_extern {
                writeln!(f, "extern @{}: {}", g.name, g.ty)?;
            } else {
                match &g.init {
                    Some(v) => writeln!(f, "global @{}: {} = {v}", g.name, g.ty)?,
                    None => writeln!(f, "global @{}: {}", g.name, g.ty)?,
                }
            }
        }
        for func in &self.functions {
            writeln!(f, "{func}")?;
        }
        Ok(())
    }
}

impl std::fmt::Display for Function {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_extern {
            write!(f, "extern ")?;
        }
        write!(f, "fn @{}(", self.name)?;
        for (i, p) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: %{}", p.ty, p.reg.0)?;
        }
        writeln!(f, ") -> {} {{", self.ret_ty)?;
        for bb in &self.blocks {
            writeln!(f, "{bb}")?;
        }
        writeln!(f, "}}")
    }
}

impl std::fmt::Display for BasicBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "  bb{}:", self.id.0)?;
        for instr in &self.instrs {
            writeln!(f, "    {instr}")?;
        }
        writeln!(f, "    {}", self.term)
    }
}

impl std::fmt::Display for Instr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Instr::Alloca { dst, ty } => write!(f, "%{} = alloca {ty}", dst.0),
            Instr::Load { dst, ty, ptr } => write!(f, "%{} = load {ty}, ptr {ptr}", dst.0),
            Instr::Store { ty, val, ptr } => write!(f, "store {ty} {val}, ptr {ptr}"),
            Instr::BinOp {
                dst,
                op,
                ty,
                lhs,
                rhs,
            } => write!(f, "%{} = {:?} {ty} {lhs}, {rhs}", dst.0, op),
            Instr::UnaryOp { dst, op, ty, val } => write!(f, "%{} = {:?} {ty} {val}", dst.0, op),
            Instr::Call {
                dst,
                func,
                args,
                ret_ty,
            } => {
                if let Some(d) = dst {
                    write!(f, "%{} = ", d.0)?;
                }
                write!(f, "call {ret_ty} {func}(")?;
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{a}")?;
                }
                write!(f, ")")
            }
            Instr::Gep {
                dst,
                elem_ty,
                base,
                idx,
            } => write!(f, "%{} = gep {elem_ty}, ptr {base}, i64 {idx}", dst.0),
            Instr::Cast {
                dst,
                from_ty,
                to_ty,
                val,
            } => write!(f, "%{} = cast {from_ty} {val} to {to_ty}", dst.0),
            Instr::Copy { dst, val } => write!(f, "%{} = copy {val}", dst.0),
        }
    }
}

impl std::fmt::Display for Terminator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Terminator::Ret(None) => write!(f, "ret void"),
            Terminator::Ret(Some(v)) => write!(f, "ret {v}"),
            Terminator::Br(bb) => write!(f, "br bb{}", bb.0),
            Terminator::CondBr {
                cond,
                then_bb,
                else_bb,
            } => write!(f, "condbr {cond}, bb{}, bb{}", then_bb.0, else_bb.0),
            Terminator::Unreachable => write!(f, "unreachable"),
        }
    }
}
