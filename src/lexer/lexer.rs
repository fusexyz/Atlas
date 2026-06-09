use super::token::{Token, TokenKind};

pub struct Lexer {
    source: Vec<char>,
    pos: usize,
    line: u32,
    col: u32,
}

impl Lexer {
    pub fn new(source: &str) -> Self {
        Self {
            source: source.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.source.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.source.get(self.pos + 1).copied()
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.source.get(self.pos).copied()?;
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), LexError> {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\r') | Some('\n') => {
                    self.advance();
                }
                Some('/') if self.peek_next() == Some('/') => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.advance();
                    }
                }
                Some('/') if self.peek_next() == Some('*') => {
                    self.advance();
                    self.advance();
                    loop {
                        match self.advance() {
                            None => {
                                return Err(LexError::new(
                                    "unterminated block comment",
                                    self.line,
                                    self.col,
                                ));
                            }
                            Some('*') if self.peek() == Some('/') => {
                                self.advance();
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_whitespace_and_comments()?;

        let line = self.line;
        let col = self.col;

        let ch = match self.peek() {
            None => return Ok(Token::new(TokenKind::Eof, line, col)),
            Some(c) => c,
        };

        let kind = match ch {
            '0'..='9' => self.lex_number()?,
            '"' => self.lex_string()?,
            '\'' => self.lex_char()?,
            'a'..='z' | 'A'..='Z' | '_' => self.lex_ident_or_keyword(),
            '#' => {
                self.advance();
                TokenKind::Hash
            }
            '(' => {
                self.advance();
                TokenKind::LParen
            }
            ')' => {
                self.advance();
                TokenKind::RParen
            }
            '{' => {
                self.advance();
                TokenKind::LBrace
            }
            '}' => {
                self.advance();
                TokenKind::RBrace
            }
            '[' => {
                self.advance();
                TokenKind::LBracket
            }
            ']' => {
                self.advance();
                TokenKind::RBracket
            }
            ';' => {
                self.advance();
                TokenKind::Semicolon
            }
            ':' => {
                self.advance();
                TokenKind::Colon
            }
            ',' => {
                self.advance();
                TokenKind::Comma
            }
            '?' => {
                self.advance();
                TokenKind::Question
            }
            '~' => {
                self.advance();
                TokenKind::Tilde
            }
            '+' => {
                self.advance();
                match self.peek() {
                    Some('+') => {
                        self.advance();
                        TokenKind::PlusPlus
                    }
                    Some('=') => {
                        self.advance();
                        TokenKind::PlusEq
                    }
                    _ => TokenKind::Plus,
                }
            }
            '-' => {
                self.advance();
                match self.peek() {
                    Some('-') => {
                        self.advance();
                        TokenKind::MinusMinus
                    }
                    Some('=') => {
                        self.advance();
                        TokenKind::MinusEq
                    }
                    Some('>') => {
                        self.advance();
                        TokenKind::Arrow
                    }
                    _ => TokenKind::Minus,
                }
            }
            '*' => {
                self.advance();
                match self.peek() {
                    Some('=') => {
                        self.advance();
                        TokenKind::StarEq
                    }
                    _ => TokenKind::Star,
                }
            }
            '/' => {
                self.advance();
                match self.peek() {
                    Some('=') => {
                        self.advance();
                        TokenKind::SlashEq
                    }
                    _ => TokenKind::Slash,
                }
            }
            '%' => {
                self.advance();
                match self.peek() {
                    Some('=') => {
                        self.advance();
                        TokenKind::PercentEq
                    }
                    _ => TokenKind::Percent,
                }
            }
            '=' => {
                self.advance();
                match self.peek() {
                    Some('=') => {
                        self.advance();
                        TokenKind::EqEq
                    }
                    _ => TokenKind::Eq,
                }
            }
            '!' => {
                self.advance();
                match self.peek() {
                    Some('=') => {
                        self.advance();
                        TokenKind::BangEq
                    }
                    _ => TokenKind::Bang,
                }
            }
            '<' => {
                self.advance();
                match self.peek() {
                    Some('<') => {
                        self.advance();
                        match self.peek() {
                            Some('=') => {
                                self.advance();
                                TokenKind::LtLtEq
                            }
                            _ => TokenKind::LtLt,
                        }
                    }
                    Some('=') => {
                        self.advance();
                        TokenKind::LtEq
                    }
                    _ => TokenKind::Lt,
                }
            }
            '>' => {
                self.advance();
                match self.peek() {
                    Some('>') => {
                        self.advance();
                        match self.peek() {
                            Some('=') => {
                                self.advance();
                                TokenKind::GtGtEq
                            }
                            _ => TokenKind::GtGt,
                        }
                    }
                    Some('=') => {
                        self.advance();
                        TokenKind::GtEq
                    }
                    _ => TokenKind::Gt,
                }
            }
            '&' => {
                self.advance();
                match self.peek() {
                    Some('&') => {
                        self.advance();
                        TokenKind::AmpAmp
                    }
                    Some('=') => {
                        self.advance();
                        TokenKind::AmpEq
                    }
                    _ => TokenKind::Ampersand,
                }
            }
            '|' => {
                self.advance();
                match self.peek() {
                    Some('|') => {
                        self.advance();
                        TokenKind::PipePipe
                    }
                    Some('=') => {
                        self.advance();
                        TokenKind::PipeEq
                    }
                    _ => TokenKind::Pipe,
                }
            }
            '^' => {
                self.advance();
                match self.peek() {
                    Some('=') => {
                        self.advance();
                        TokenKind::CaretEq
                    }
                    _ => TokenKind::Caret,
                }
            }
            '.' => {
                self.advance();
                if self.peek() == Some('.') && self.peek_next() == Some('.') {
                    self.advance();
                    self.advance();
                    TokenKind::Ellipsis
                } else {
                    TokenKind::Dot
                }
            }
            c => {
                return Err(LexError::new(
                    &format!("unexpected character '{c}'"),
                    line,
                    col,
                ));
            }
        };

        Ok(Token::new(kind, line, col))
    }

    fn lex_number(&mut self) -> Result<TokenKind, LexError> {
        let start = self.pos;
        let mut is_float = false;

        if self.peek() == Some('0') && matches!(self.peek_next(), Some('x') | Some('X')) {
            self.advance();
            self.advance();
            while self
                .peek()
                .is_some_and(|c| c.is_ascii_hexdigit() || c == '_')
            {
                self.advance();
            }
            let s: String = self.source[start..self.pos].iter().collect();
            let val = i64::from_str_radix(&s[2..].replace('_', ""), 16)
                .map_err(|_| LexError::new("invalid hex literal", self.line, self.col))?;
            return Ok(TokenKind::IntLiteral(val));
        }

        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
            self.advance();
        }
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
            is_float = true;
            self.advance();
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.advance();
            }
        }
        if self.peek().is_some_and(|c| matches!(c, 'e' | 'E')) {
            is_float = true;
            self.advance();
            if self.peek().is_some_and(|c| matches!(c, '+' | '-')) {
                self.advance();
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.advance();
            }
        }
        while self
            .peek()
            .is_some_and(|c| matches!(c, 'u' | 'U' | 'l' | 'L' | 'f' | 'F'))
        {
            self.advance();
        }

        let s: String = self.source[start..self.pos]
            .iter()
            .filter(|&&c| c != '_')
            .collect();

        if is_float {
            let val: f64 = s
                .trim_end_matches(|c| matches!(c, 'f' | 'F' | 'l' | 'L'))
                .parse()
                .map_err(|_| LexError::new("invalid float literal", self.line, self.col))?;
            Ok(TokenKind::FloatLiteral(val))
        } else {
            let digits = s.trim_end_matches(|c| matches!(c, 'u' | 'U' | 'l' | 'L'));
            let val: i64 = digits
                .parse()
                .map_err(|_| LexError::new("invalid integer literal", self.line, self.col))?;
            Ok(TokenKind::IntLiteral(val))
        }
    }

    fn lex_string(&mut self) -> Result<TokenKind, LexError> {
        self.advance();
        let mut s = String::new();
        loop {
            match self.advance() {
                None | Some('\n') => {
                    return Err(LexError::new(
                        "unterminated string literal",
                        self.line,
                        self.col,
                    ));
                }
                Some('"') => break,
                Some('\\') => s.push(self.lex_escape()?),
                Some(c) => s.push(c),
            }
        }
        Ok(TokenKind::StringLiteral(s))
    }

    fn lex_char(&mut self) -> Result<TokenKind, LexError> {
        self.advance();
        let ch = match self.advance() {
            None => {
                return Err(LexError::new(
                    "unterminated char literal",
                    self.line,
                    self.col,
                ));
            }
            Some('\\') => self.lex_escape()?,
            Some(c) => c,
        };
        match self.advance() {
            Some('\'') => Ok(TokenKind::CharLiteral(ch)),
            _ => Err(LexError::new(
                "unterminated char literal",
                self.line,
                self.col,
            )),
        }
    }

    fn lex_escape(&mut self) -> Result<char, LexError> {
        match self.advance() {
            Some('n') => Ok('\n'),
            Some('t') => Ok('\t'),
            Some('r') => Ok('\r'),
            Some('0') => Ok('\0'),
            Some('\\') => Ok('\\'),
            Some('\'') => Ok('\''),
            Some('"') => Ok('"'),
            _ => Err(LexError::new(
                "unknown escape sequence",
                self.line,
                self.col,
            )),
        }
    }

    fn lex_ident_or_keyword(&mut self) -> TokenKind {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_') {
            self.advance();
        }
        let ident: String = self.source[start..self.pos].iter().collect();
        match ident.as_str() {
            "auto" => TokenKind::Auto,
            "break" => TokenKind::Break,
            "case" => TokenKind::Case,
            "char" => TokenKind::Char,
            "const" => TokenKind::Const,
            "continue" => TokenKind::Continue,
            "default" => TokenKind::Default,
            "do" => TokenKind::Do,
            "double" => TokenKind::Double,
            "else" => TokenKind::Else,
            "enum" => TokenKind::Enum,
            "extern" => TokenKind::Extern,
            "float" => TokenKind::Float,
            "for" => TokenKind::For,
            "goto" => TokenKind::Goto,
            "if" => TokenKind::If,
            "inline" => TokenKind::Inline,
            "int" => TokenKind::Int,
            "long" => TokenKind::Long,
            "register" => TokenKind::Register,
            "restrict" => TokenKind::Restrict,
            "return" => TokenKind::Return,
            "short" => TokenKind::Short,
            "signed" => TokenKind::Signed,
            "sizeof" => TokenKind::Sizeof,
            "static" => TokenKind::Static,
            "struct" => TokenKind::Struct,
            "switch" => TokenKind::Switch,
            "typedef" => TokenKind::Typedef,
            "union" => TokenKind::Union,
            "unsigned" => TokenKind::Unsigned,
            "void" => TokenKind::Void,
            "volatile" => TokenKind::Volatile,
            "while" => TokenKind::While,
            _ => TokenKind::Identifier(ident),
        }
    }
}

#[derive(Debug)]
pub struct LexError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

impl LexError {
    fn new(message: &str, line: u32, col: u32) -> Self {
        Self {
            message: message.to_string(),
            line,
            col,
        }
    }
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "lex error at {}:{}: {}",
            self.line, self.col, self.message
        )
    }
}
