pub mod codegen;
pub mod encode;
pub mod machine;
pub mod opt;

pub use codegen::compile_module;
pub use encode::{EncodedFunction, Reloc, encode_function};
