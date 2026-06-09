#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    IntLiteral(i64),
    FloatLiteral(f64),
    StringLiteral(String),
    CharLiteral(char),
    Identifier(String),

    Auto,
    Break,
    Case,
    Char,
    Const,
    Continue,
    Default,
    Do,
    Double,
    Else,
    Enum,
    Extern,
    Float,
    For,
    Goto,
    If,
    Inline,
    Int,
    Long,
    Register,
    Restrict,
    Return,
    Short,
    Signed,
    Sizeof,
    Static,
    Struct,
    Switch,
    Typedef,
    Union,
    Unsigned,
    Void,
    Volatile,
    While,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,

    Ampersand,
    Pipe,
    Caret,
    Tilde,
    LtLt,
    GtGt,

    AmpAmp,
    PipePipe,
    Bang,

    EqEq,
    BangEq,
    Lt,
    Gt,
    LtEq,
    GtEq,

    Eq,
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    LtLtEq,
    GtGtEq,

    PlusPlus,
    MinusMinus,

    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Semicolon,
    Colon,
    Comma,
    Dot,
    Arrow,
    Ellipsis,
    Question,
    Hash,

    Eof,
}

#[derive(Debug, Clone)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, line: u32, col: u32) -> Self {
        Self {
            kind,
            span: Span { line, col },
        }
    }
}
