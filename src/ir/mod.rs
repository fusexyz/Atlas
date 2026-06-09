pub mod ir;
pub mod lower;

pub use ir::{
    BasicBlock, BinOpKind, BlockId, Function, Instr, IrType, Module, Terminator, UnaryOpKind, VReg,
    Value,
};
pub use lower::{LowerError, lower_module};
