use crate::lexer::token::Span;

#[derive(Debug, Clone)]
pub enum TypeSpec {
    Void,
    Char,
    Short,
    Int,
    Long,
    LongLong,
    Float,
    Double,
    Unsigned(Box<TypeSpec>),
    Signed(Box<TypeSpec>),
    Pointer(Box<TypeSpec>),
    Array(Box<TypeSpec>, Option<Box<Expr>>),
    Struct(String),
    Union(String),
    Enum(String),
    Named(String),
}

#[derive(Debug, Clone)]
pub struct TranslationUnit {
    pub items: Vec<TopLevel>,
    pub enum_constants: std::collections::HashMap<String, i64>,
}

#[derive(Debug, Clone)]
pub enum TopLevel {
    Function(FuncDef),
    FuncDecl(FuncDecl),
    Declaration(Decl),
    StructDef(StructDef),
    Typedef(TypeSpec, String, Span),
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<FieldDecl>,
    pub is_union: bool,
    pub span: Span,
    pub pack: u64,
}

#[derive(Debug, Clone)]
pub struct FieldDecl {
    pub ty: TypeSpec,
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FuncDef {
    pub ret_ty: TypeSpec,
    pub name: String,
    pub params: Vec<Param>,
    pub variadic: bool,
    pub body: Block,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub name: String,
    pub ret_ty: TypeSpec,
    pub params: Vec<Param>,
    pub variadic: bool,
    pub storage: StorageClass,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub ty: TypeSpec,
    pub name: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub storage: StorageClass,
    pub ty: TypeSpec,
    pub name: String,
    pub init: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StorageClass {
    None,
    Extern,
    Static,
    Auto,
    Register,
}

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Decl(Decl),
    Expr(Expr),
    Block(Block),
    Return(Option<Expr>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    DoWhile(Box<Stmt>, Expr),
    For(Option<ForInit>, Option<Expr>, Option<Expr>, Box<Stmt>),
    Break,
    Continue,
    Goto(String),
    Label(String, Box<Stmt>),
    Switch(Expr, Box<Stmt>),
    Case(Expr, Box<Stmt>),
    Default(Box<Stmt>),
}

#[derive(Debug, Clone)]
pub enum ForInit {
    Decl(Decl),
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    CharLit(char),
    Ident(String),

    Unary(UnaryOp, Box<Expr>),
    Binary(BinaryOp, Box<Expr>, Box<Expr>),
    Assign(AssignOp, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),

    Call(Box<Expr>, Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String),
    Arrow(Box<Expr>, String),

    Cast(TypeSpec, Box<Expr>),
    Sizeof(SizeofArg),
    Alignof(SizeofArg),
    AddrOf(Box<Expr>),
    Deref(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum SizeofArg {
    Expr(Box<Expr>),
    Type(TypeSpec),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    And,
    Or,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Comma,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignOp {
    Assign,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    ModAssign,
    AndAssign,
    OrAssign,
    XorAssign,
    ShlAssign,
    ShrAssign,
}
