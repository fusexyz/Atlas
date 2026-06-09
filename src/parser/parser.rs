use super::ast::*;
use crate::lexer::token::{Span, Token, TokenKind};

pub struct Parser {
    pub tokens: Vec<Token>,
    pub pos: usize,
    pub typedef_names: std::collections::HashSet<String>,
    pub struct_defs: Vec<StructDef>,
    pub pack_stack: Vec<u64>,
    pub current_pack: u64,
    pub enum_constants: std::collections::HashMap<String, i64>,
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl ParseError {
    fn new(msg: impl Into<String>, span: Span) -> Self {
        Self {
            message: msg.into(),
            span,
        }
    }
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "parse error at {}:{}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

type PR<T> = Result<T, ParseError>;

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            typedef_names: std::collections::HashSet::new(),
            struct_defs: Vec::new(),
            pack_stack: Vec::new(),
            current_pack: 8,
            enum_constants: std::collections::HashMap::new(),
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek2(&self) -> &Token {
        let i = (self.pos + 1).min(self.tokens.len() - 1);
        &self.tokens[i]
    }

    fn span(&self) -> Span {
        self.peek().span.clone()
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos];
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn eat(&mut self, kind: &TokenKind) -> PR<Span> {
        if &self.peek().kind == kind {
            Ok(self.advance().span.clone())
        } else {
            Err(ParseError::new(
                format!("expected {:?}, got {:?}", kind, self.peek().kind),
                self.span(),
            ))
        }
    }

    fn eat_ident(&mut self) -> PR<(String, Span)> {
        let span = self.span();
        if let TokenKind::Identifier(name) = self.peek().kind.clone() {
            self.advance();
            Ok((name, span))
        } else {
            Err(ParseError::new(
                format!("expected identifier, got {:?}", self.peek().kind),
                span,
            ))
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        &self.peek().kind == kind
    }

    fn at_eof(&self) -> bool {
        self.peek().kind == TokenKind::Eof
    }

    fn try_eat(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    pub fn check_pragma_pack(&mut self) -> PR<bool> {
        let is_pragma = if let TokenKind::Identifier(ref name) = self.peek().kind {
            name.starts_with("__pragma_pack_")
        } else {
            false
        };

        if is_pragma {
            if let TokenKind::Identifier(name) = self.advance().kind.clone() {
                if name.starts_with("__pragma_pack_push_") {
                    let val_str = &name["__pragma_pack_push_".len()..];
                    let val = if val_str == "default" {
                        8
                    } else {
                        val_str.parse::<u64>().unwrap_or(8)
                    };
                    self.pack_stack.push(self.current_pack);
                    self.current_pack = val;
                } else if name == "__pragma_pack_pop" {
                    self.current_pack = self.pack_stack.pop().unwrap_or(8);
                } else if name.starts_with("__pragma_pack_") {
                    let val_str = &name["__pragma_pack_".len()..];
                    let val = if val_str == "default" {
                        8
                    } else {
                        val_str.parse::<u64>().unwrap_or(8)
                    };
                    self.current_pack = val;
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn check_pragma_inline(&mut self) -> PR<bool> {
        if let TokenKind::Identifier(name) = &self.peek().kind {
            if name == "__pragma" {
                self.advance();
                self.eat(&TokenKind::LParen)?;

                let mut is_pack = false;
                if let TokenKind::Identifier(pname) = &self.peek().kind {
                    if pname == "pack" {
                        is_pack = true;
                    }
                }

                if is_pack {
                    self.advance();
                    self.eat(&TokenKind::LParen)?;

                    match &self.peek().kind {
                        TokenKind::Identifier(arg) if arg == "push" => {
                            self.advance();
                            let mut val = 8;
                            if self.try_eat(&TokenKind::Comma) {
                                if let TokenKind::IntLiteral(n) = self.peek().kind {
                                    val = n as u64;
                                    self.advance();
                                }
                            }
                            self.pack_stack.push(self.current_pack);
                            self.current_pack = val;
                        }
                        TokenKind::Identifier(arg) if arg == "pop" => {
                            self.advance();
                            self.current_pack = self.pack_stack.pop().unwrap_or(8);
                        }
                        TokenKind::IntLiteral(n) => {
                            let val = *n as u64;
                            self.advance();
                            self.current_pack = val;
                        }
                        _ => {}
                    }
                    self.eat(&TokenKind::RParen)?;
                } else {
                    let mut depth = 1;
                    while depth > 0 && !self.at_eof() {
                        if self.check(&TokenKind::LParen) {
                            depth += 1;
                        } else if self.check(&TokenKind::RParen) {
                            depth -= 1;
                        }
                        self.advance();
                    }
                    return Ok(true);
                }

                self.eat(&TokenKind::RParen)?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn skip_msvc_attributes(&mut self) -> PR<()> {
        loop {
            match &self.peek().kind {
                TokenKind::Identifier(name)
                    if matches!(
                        name.as_str(),
                        "__stdcall"
                            | "__cdecl"
                            | "__fastcall"
                            | "__thiscall"
                            | "__vectorcall"
                            | "__unaligned"
                            | "__inline"
                            | "__forceinline"
                            | "__restrict"
                            | "__w64"
                            | "__ptr32"
                            | "__ptr64"
                            | "__sptr"
                            | "__uptr"
                            | "inline"
                            | "restrict"
                    ) =>
                {
                    self.advance();
                }
                TokenKind::Identifier(name) if name == "__declspec" => {
                    self.advance();
                    self.eat_balanced_parens()?;
                }
                TokenKind::Identifier(name) if name == "__attribute__" => {
                    self.advance();
                    self.eat_balanced_parens()?;
                }
                TokenKind::Identifier(name) if name == "__pragma" => {
                    self.advance();
                    self.eat_balanced_parens()?;
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn eat_balanced_parens(&mut self) -> PR<()> {
        self.eat(&TokenKind::LParen)?;
        let mut depth = 1;
        while depth > 0 && !self.at_eof() {
            if self.check(&TokenKind::LParen) {
                depth += 1;
            } else if self.check(&TokenKind::RParen) {
                depth -= 1;
            }
            self.advance();
        }
        Ok(())
    }
}

impl Parser {
    pub fn parse(&mut self) -> PR<TranslationUnit> {
        let mut items = Vec::new();
        while !self.at_eof() {
            if self.check_pragma_pack()? {
                continue;
            }
            if self.check_pragma_inline()? {
                continue;
            }
            items.push(self.parse_top_level()?);
        }
        for s in self.struct_defs.drain(..) {
            items.push(TopLevel::StructDef(s));
        }
        Ok(TranslationUnit {
            items,
            enum_constants: self.enum_constants.clone(),
        })
    }

    fn parse_top_level(&mut self) -> PR<TopLevel> {
        let span = self.span();

        if self.check(&TokenKind::Typedef) {
            self.advance();
            let ty = self.parse_type_spec()?;
            self.skip_msvc_attributes()?;
            let (name, _) = self.eat_ident()?;
            self.eat(&TokenKind::Semicolon)?;
            self.typedef_names.insert(name.clone());
            return Ok(TopLevel::Typedef(ty, name, span));
        }

        if (self.check(&TokenKind::Struct) || self.check(&TokenKind::Union)) && self.is_struct_def()
        {
            self.parse_type_spec()?;
            self.try_eat(&TokenKind::Semicolon);
            if let Some(s) = self.struct_defs.pop() {
                return Ok(TopLevel::StructDef(s));
            }
        }

        let storage = self.parse_storage_class();
        let ty = self.parse_type_spec()?;
        self.skip_msvc_attributes()?;
        let (name, name_span) = self.eat_ident()?;

        if self.check(&TokenKind::LParen) {
            let (params, variadic) = self.parse_param_list()?;
            if self.check(&TokenKind::LBrace) {
                let body = self.parse_block()?;
                return Ok(TopLevel::Function(FuncDef {
                    ret_ty: ty,
                    name,
                    params,
                    variadic,
                    body,
                    span,
                }));
            }

            self.eat(&TokenKind::Semicolon)?;
            return Ok(TopLevel::FuncDecl(FuncDecl {
                name,
                ret_ty: ty,
                params,
                storage,
                span: name_span,
            }));
        }

        let init = if self.try_eat(&TokenKind::Eq) {
            Some(self.parse_assign_expr()?)
        } else {
            None
        };
        self.eat(&TokenKind::Semicolon)?;
        Ok(TopLevel::Declaration(Decl {
            storage,
            ty,
            name,
            init,
            span: name_span,
        }))
    }

    fn is_struct_def(&self) -> bool {
        let mut i = self.pos + 1;
        while i < self.tokens.len() {
            match &self.tokens[i].kind {
                TokenKind::LBrace => return true,
                TokenKind::Semicolon | TokenKind::Eof => return false,
                _ => i += 1,
            }
        }
        false
    }
}

impl Parser {
    fn parse_storage_class(&mut self) -> StorageClass {
        match &self.peek().kind {
            TokenKind::Extern => {
                self.advance();
                StorageClass::Extern
            }
            TokenKind::Static => {
                self.advance();
                StorageClass::Static
            }
            TokenKind::Auto => {
                self.advance();
                StorageClass::Auto
            }
            TokenKind::Register => {
                self.advance();
                StorageClass::Register
            }
            _ => StorageClass::None,
        }
    }

    fn parse_type_spec(&mut self) -> PR<TypeSpec> {
        let mut unsigned = false;
        let mut signed = false;
        let mut long_count = 0usize;

        loop {
            self.skip_msvc_attributes()?;
            match &self.peek().kind {
                TokenKind::Const
                | TokenKind::Volatile
                | TokenKind::Restrict
                | TokenKind::Inline => {
                    self.advance();
                }
                TokenKind::Unsigned => {
                    self.advance();
                    unsigned = true;
                }
                TokenKind::Signed => {
                    self.advance();
                    signed = true;
                }
                TokenKind::Long => {
                    self.advance();
                    long_count += 1;
                }
                _ => break,
            }
        }

        let base = match &self.peek().kind {
            TokenKind::Void => {
                self.advance();
                TypeSpec::Void
            }
            TokenKind::Char => {
                self.advance();
                TypeSpec::Char
            }
            TokenKind::Short => {
                self.advance();
                TypeSpec::Short
            }
            TokenKind::Int => {
                self.advance();
                TypeSpec::Int
            }
            TokenKind::Long => {
                self.advance();
                long_count += 1;
                TypeSpec::Long
            }
            TokenKind::Float => {
                self.advance();
                TypeSpec::Float
            }
            TokenKind::Double => {
                self.advance();
                TypeSpec::Double
            }
            TokenKind::Struct | TokenKind::Union => {
                let is_union = self.check(&TokenKind::Union);
                let span = self.span();
                self.advance();
                self.skip_msvc_attributes()?;
                let name = if let TokenKind::Identifier(ref n) = self.peek().kind {
                    let n = n.clone();
                    self.advance();
                    Some(n)
                } else {
                    None
                };
                if self.check(&TokenKind::LBrace) {
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                        if self.check_pragma_pack()? {
                            continue;
                        }
                        if self.check_pragma_inline()? {
                            continue;
                        }
                        let fspan = self.span();
                        let base_ty = self.parse_type_spec()?;
                        self.skip_msvc_attributes()?;
                        if self.check(&TokenKind::Semicolon) {
                            self.advance();
                            fields.push(FieldDecl {
                                ty: base_ty,
                                name: format!("__anon_field_{}", fields.len()),
                                span: fspan,
                            });
                        } else {
                            loop {
                                let ty = self.parse_pointer_suffix(base_ty.clone())?;
                                self.skip_msvc_attributes()?;
                                let (fname, name_span) = self.eat_ident()?;
                                let ty = if self.check(&TokenKind::LBracket) {
                                    self.advance();
                                    let size = if !self.check(&TokenKind::RBracket) {
                                        Some(Box::new(self.parse_assign_expr()?))
                                    } else {
                                        None
                                    };
                                    self.eat(&TokenKind::RBracket)?;
                                    TypeSpec::Array(Box::new(ty), size)
                                } else {
                                    ty
                                };
                                fields.push(FieldDecl {
                                    ty,
                                    name: fname,
                                    span: name_span,
                                });
                                if !self.try_eat(&TokenKind::Comma) {
                                    break;
                                }
                            }
                            self.eat(&TokenKind::Semicolon)?;
                        }
                    }
                    self.eat(&TokenKind::RBrace)?;
                    let final_name =
                        name.unwrap_or_else(|| format!("__anon_struct_{}", self.struct_defs.len()));
                    self.struct_defs.push(StructDef {
                        name: final_name.clone(),
                        fields,
                        is_union,
                        span,
                        pack: self.current_pack,
                    });
                    if is_union {
                        TypeSpec::Union(final_name)
                    } else {
                        TypeSpec::Struct(final_name)
                    }
                } else {
                    let final_name = name.ok_or_else(|| {
                        ParseError::new("expected struct/union name or definition", span.clone())
                    })?;
                    if is_union {
                        TypeSpec::Union(final_name)
                    } else {
                        TypeSpec::Struct(final_name)
                    }
                }
            }
            TokenKind::Enum => {
                self.advance();
                self.skip_msvc_attributes()?;
                let name = if let TokenKind::Identifier(ref n) = self.peek().kind {
                    let n = n.clone();
                    self.advance();
                    Some(n)
                } else {
                    None
                };
                if self.check(&TokenKind::LBrace) {
                    self.advance();
                    let mut val = 0i64;
                    while !self.check(&TokenKind::RBrace) && !self.at_eof() {
                        let (item_name, _) = self.eat_ident()?;
                        if self.try_eat(&TokenKind::Eq) {
                            let expr = self.parse_assign_expr()?;
                            if let Some(num) = eval_const_expr(&expr, &self.enum_constants) {
                                val = num;
                            }
                        }
                        self.enum_constants.insert(item_name, val);
                        val += 1;
                        self.try_eat(&TokenKind::Comma);
                    }
                    self.eat(&TokenKind::RBrace)?;
                    let final_name = name
                        .unwrap_or_else(|| format!("__anon_enum_{}", self.enum_constants.len()));
                    TypeSpec::Enum(final_name)
                } else {
                    let final_name = name.ok_or_else(|| {
                        ParseError::new("expected enum name or definition", self.span())
                    })?;
                    TypeSpec::Enum(final_name)
                }
            }
            TokenKind::Identifier(name)
                if name == "__int8"
                    || name == "__int16"
                    || name == "__int32"
                    || name == "__int64" =>
            {
                let name = name.clone();
                self.advance();
                match name.as_str() {
                    "__int8" => TypeSpec::Char,
                    "__int16" => TypeSpec::Short,
                    "__int32" => TypeSpec::Int,
                    _ => TypeSpec::LongLong,
                }
            }
            TokenKind::Identifier(name) if !(long_count > 0 || unsigned || signed) => {
                let name = name.clone();
                self.advance();
                TypeSpec::Named(name)
            }
            _ if long_count > 0 || unsigned || signed => TypeSpec::Int,
            _ => {
                return Err(ParseError::new(
                    format!("expected type, got {:?}", self.peek().kind),
                    self.span(),
                ));
            }
        };

        let base = if long_count >= 2 {
            TypeSpec::LongLong
        } else if long_count == 1 {
            TypeSpec::Long
        } else {
            base
        };
        let base = if unsigned {
            TypeSpec::Unsigned(Box::new(base))
        } else if signed {
            TypeSpec::Signed(Box::new(base))
        } else {
            base
        };

        loop {
            self.skip_msvc_attributes()?;
            match &self.peek().kind {
                TokenKind::Const | TokenKind::Volatile | TokenKind::Restrict => {
                    self.advance();
                }
                _ => break,
            }
        }

        self.parse_pointer_suffix(base)
    }

    fn parse_pointer_suffix(&mut self, mut ty: TypeSpec) -> PR<TypeSpec> {
        while self.check(&TokenKind::Star) {
            self.advance();
            self.skip_msvc_attributes()?;
            while matches!(&self.peek().kind, TokenKind::Const | TokenKind::Volatile) {
                self.advance();
            }
            self.skip_msvc_attributes()?;
            ty = TypeSpec::Pointer(Box::new(ty));
        }
        Ok(ty)
    }
}

impl Parser {
    fn parse_param_list(&mut self) -> PR<(Vec<Param>, bool)> {
        self.eat(&TokenKind::LParen)?;
        let mut params = Vec::new();
        let mut variadic = false;

        if self.check(&TokenKind::RParen) {
            self.advance();
            return Ok((params, variadic));
        }

        loop {
            if self.check(&TokenKind::Ellipsis) {
                self.advance();
                variadic = true;
                break;
            }
            params.push(self.parse_param()?);
            if !self.try_eat(&TokenKind::Comma) {
                break;
            }
        }

        self.eat(&TokenKind::RParen)?;
        Ok((params, variadic))
    }

    fn parse_param(&mut self) -> PR<Param> {
        let span = self.span();
        let ty = self.parse_type_spec()?;
        self.skip_msvc_attributes()?;
        let name = if let TokenKind::Identifier(n) = &self.peek().kind {
            let n = n.clone();
            self.advance();
            Some(n)
        } else {
            None
        };
        Ok(Param { ty, name, span })
    }
}

impl Parser {
    fn parse_block(&mut self) -> PR<Block> {
        self.eat(&TokenKind::LBrace)?;
        let mut stmts = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.at_eof() {
            if self.check_pragma_pack()? {
                continue;
            }
            if self.check_pragma_inline()? {
                continue;
            }
            stmts.push(self.parse_stmt()?);
        }
        self.eat(&TokenKind::RBrace)?;
        Ok(stmts)
    }

    fn parse_stmt(&mut self) -> PR<Stmt> {
        let span = self.span();
        let kind = match &self.peek().kind {
            TokenKind::Typedef => {
                self.advance();
                let _ty = self.parse_type_spec()?;
                let (name, _) = self.eat_ident()?;
                self.eat(&TokenKind::Semicolon)?;
                self.typedef_names.insert(name.clone());
                StmtKind::Block(vec![])
            }
            TokenKind::LBrace => StmtKind::Block(self.parse_block()?),
            TokenKind::Return => self.parse_return()?,
            TokenKind::If => self.parse_if()?,
            TokenKind::While => self.parse_while()?,
            TokenKind::Do => self.parse_do_while()?,
            TokenKind::For => self.parse_for()?,
            TokenKind::Break => {
                self.advance();
                self.eat(&TokenKind::Semicolon)?;
                StmtKind::Break
            }
            TokenKind::Continue => {
                self.advance();
                self.eat(&TokenKind::Semicolon)?;
                StmtKind::Continue
            }
            TokenKind::Goto => {
                self.advance();
                let (label, _) = self.eat_ident()?;
                self.eat(&TokenKind::Semicolon)?;
                StmtKind::Goto(label)
            }
            TokenKind::Switch => self.parse_switch()?,
            TokenKind::Case => self.parse_case()?,
            TokenKind::Default => {
                self.advance();
                self.eat(&TokenKind::Colon)?;
                let s = self.parse_stmt()?;
                StmtKind::Default(Box::new(s))
            }
            _ if self.is_type_start() => {
                let decl = self.parse_local_decl()?;
                StmtKind::Decl(decl)
            }
            _ if self.is_label() => {
                let (name, _) = self.eat_ident()?;
                self.eat(&TokenKind::Colon)?;
                let s = self.parse_stmt()?;
                StmtKind::Label(name, Box::new(s))
            }
            _ => {
                let expr = self.parse_expr()?;
                self.eat(&TokenKind::Semicolon)?;
                StmtKind::Expr(expr)
            }
        };
        Ok(Stmt { kind, span })
    }

    fn is_type_start(&self) -> bool {
        match &self.peek().kind {
            TokenKind::Void
            | TokenKind::Char
            | TokenKind::Short
            | TokenKind::Int
            | TokenKind::Long
            | TokenKind::Float
            | TokenKind::Double
            | TokenKind::Unsigned
            | TokenKind::Signed
            | TokenKind::Struct
            | TokenKind::Union
            | TokenKind::Enum
            | TokenKind::Const
            | TokenKind::Volatile
            | TokenKind::Restrict
            | TokenKind::Extern
            | TokenKind::Static
            | TokenKind::Auto
            | TokenKind::Register => true,
            TokenKind::Identifier(name) => {
                name == "__int8"
                    || name == "__int16"
                    || name == "__int32"
                    || name == "__int64"
                    || name == "__declspec"
                    || name == "__attribute__"
                    || name == "__stdcall"
                    || name == "__cdecl"
                    || name == "__fastcall"
                    || name == "__thiscall"
                    || name == "__vectorcall"
                    || self.typedef_names.contains(name)
            }
            _ => false,
        }
    }

    fn is_label(&self) -> bool {
        matches!(&self.peek().kind, TokenKind::Identifier(_))
            && matches!(&self.peek2().kind, TokenKind::Colon)
    }

    fn parse_local_decl(&mut self) -> PR<Decl> {
        let span = self.span();
        let storage = self.parse_storage_class();
        let ty = self.parse_type_spec()?;
        self.skip_msvc_attributes()?;
        let (name, _) = self.eat_ident()?;

        let ty = if self.check(&TokenKind::LBracket) {
            self.advance();
            let size = if !self.check(&TokenKind::RBracket) {
                Some(Box::new(self.parse_assign_expr()?))
            } else {
                None
            };
            self.eat(&TokenKind::RBracket)?;
            TypeSpec::Array(Box::new(ty), size)
        } else {
            ty
        };
        let init = if self.try_eat(&TokenKind::Eq) {
            Some(self.parse_assign_expr()?)
        } else {
            None
        };
        self.eat(&TokenKind::Semicolon)?;
        Ok(Decl {
            storage,
            ty,
            name,
            init,
            span,
        })
    }

    fn parse_return(&mut self) -> PR<StmtKind> {
        self.advance();
        if self.check(&TokenKind::Semicolon) {
            self.advance();
            Ok(StmtKind::Return(None))
        } else {
            let expr = self.parse_expr()?;
            self.eat(&TokenKind::Semicolon)?;
            Ok(StmtKind::Return(Some(expr)))
        }
    }

    fn parse_if(&mut self) -> PR<StmtKind> {
        self.advance();
        self.eat(&TokenKind::LParen)?;
        let cond = self.parse_expr()?;
        self.eat(&TokenKind::RParen)?;
        let then = Box::new(self.parse_stmt()?);
        let else_ = if self.try_eat(&TokenKind::Else) {
            Some(Box::new(self.parse_stmt()?))
        } else {
            None
        };
        Ok(StmtKind::If(cond, then, else_))
    }

    fn parse_while(&mut self) -> PR<StmtKind> {
        self.advance();
        self.eat(&TokenKind::LParen)?;
        let cond = self.parse_expr()?;
        self.eat(&TokenKind::RParen)?;
        let body = Box::new(self.parse_stmt()?);
        Ok(StmtKind::While(cond, body))
    }

    fn parse_do_while(&mut self) -> PR<StmtKind> {
        self.advance();
        let body = Box::new(self.parse_stmt()?);
        self.eat(&TokenKind::While)?;
        self.eat(&TokenKind::LParen)?;
        let cond = self.parse_expr()?;
        self.eat(&TokenKind::RParen)?;
        self.eat(&TokenKind::Semicolon)?;
        Ok(StmtKind::DoWhile(body, cond))
    }

    fn parse_for(&mut self) -> PR<StmtKind> {
        self.advance();
        self.eat(&TokenKind::LParen)?;

        let init = if self.check(&TokenKind::Semicolon) {
            self.advance();
            None
        } else if self.is_type_start() {
            Some(ForInit::Decl(self.parse_local_decl()?))
        } else {
            let e = self.parse_expr()?;
            self.eat(&TokenKind::Semicolon)?;
            Some(ForInit::Expr(e))
        };

        let cond = if !self.check(&TokenKind::Semicolon) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.eat(&TokenKind::Semicolon)?;

        let post = if !self.check(&TokenKind::RParen) {
            Some(self.parse_expr()?)
        } else {
            None
        };
        self.eat(&TokenKind::RParen)?;

        let body = Box::new(self.parse_stmt()?);
        Ok(StmtKind::For(init, cond, post, body))
    }

    fn parse_switch(&mut self) -> PR<StmtKind> {
        self.advance();
        self.eat(&TokenKind::LParen)?;
        let expr = self.parse_expr()?;
        self.eat(&TokenKind::RParen)?;
        let body = Box::new(self.parse_stmt()?);
        Ok(StmtKind::Switch(expr, body))
    }

    fn parse_case(&mut self) -> PR<StmtKind> {
        self.advance();
        let val = self.parse_assign_expr()?;
        self.eat(&TokenKind::Colon)?;
        let body = Box::new(self.parse_stmt()?);
        Ok(StmtKind::Case(val, body))
    }
}

impl Parser {
    fn parse_expr(&mut self) -> PR<Expr> {
        let span = self.span();
        let mut lhs = self.parse_assign_expr()?;
        while self.check(&TokenKind::Comma) {
            self.advance();
            let rhs = self.parse_assign_expr()?;
            lhs = Expr {
                kind: ExprKind::Binary(BinaryOp::Comma, Box::new(lhs), Box::new(rhs)),
                span: span.clone(),
            };
        }
        Ok(lhs)
    }

    fn parse_assign_expr(&mut self) -> PR<Expr> {
        let span = self.span();
        let lhs = self.parse_ternary()?;

        let op = match &self.peek().kind {
            TokenKind::Eq => AssignOp::Assign,
            TokenKind::PlusEq => AssignOp::AddAssign,
            TokenKind::MinusEq => AssignOp::SubAssign,
            TokenKind::StarEq => AssignOp::MulAssign,
            TokenKind::SlashEq => AssignOp::DivAssign,
            TokenKind::PercentEq => AssignOp::ModAssign,
            TokenKind::AmpEq => AssignOp::AndAssign,
            TokenKind::PipeEq => AssignOp::OrAssign,
            TokenKind::CaretEq => AssignOp::XorAssign,
            TokenKind::LtLtEq => AssignOp::ShlAssign,
            TokenKind::GtGtEq => AssignOp::ShrAssign,
            _ => return Ok(lhs),
        };
        self.advance();
        let rhs = self.parse_assign_expr()?;
        Ok(Expr {
            kind: ExprKind::Assign(op, Box::new(lhs), Box::new(rhs)),
            span,
        })
    }

    fn parse_ternary(&mut self) -> PR<Expr> {
        let span = self.span();
        let cond = self.parse_binary(0)?;
        if self.try_eat(&TokenKind::Question) {
            let then = self.parse_expr()?;
            self.eat(&TokenKind::Colon)?;
            let else_ = self.parse_ternary()?;
            return Ok(Expr {
                kind: ExprKind::Ternary(Box::new(cond), Box::new(then), Box::new(else_)),
                span,
            });
        }
        Ok(cond)
    }

    fn parse_binary(&mut self, min_prec: u8) -> PR<Expr> {
        let span = self.span();
        let mut lhs = self.parse_unary()?;

        loop {
            let (op, prec, right_assoc) = match self.binary_op() {
                Some(x) => x,
                None => break,
            };
            if prec < min_prec {
                break;
            }
            self.advance();
            let next_prec = if right_assoc { prec } else { prec + 1 };
            let rhs = self.parse_binary(next_prec)?;
            lhs = Expr {
                kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
                span: span.clone(),
            };
        }
        Ok(lhs)
    }

    fn binary_op(&self) -> Option<(BinaryOp, u8, bool)> {
        Some(match &self.peek().kind {
            TokenKind::PipePipe => (BinaryOp::Or, 1, false),
            TokenKind::AmpAmp => (BinaryOp::And, 2, false),
            TokenKind::Pipe => (BinaryOp::BitOr, 3, false),
            TokenKind::Caret => (BinaryOp::BitXor, 4, false),
            TokenKind::Ampersand => (BinaryOp::BitAnd, 5, false),
            TokenKind::EqEq => (BinaryOp::Eq, 6, false),
            TokenKind::BangEq => (BinaryOp::Ne, 6, false),
            TokenKind::Lt => (BinaryOp::Lt, 7, false),
            TokenKind::Gt => (BinaryOp::Gt, 7, false),
            TokenKind::LtEq => (BinaryOp::Le, 7, false),
            TokenKind::GtEq => (BinaryOp::Ge, 7, false),
            TokenKind::LtLt => (BinaryOp::Shl, 8, false),
            TokenKind::GtGt => (BinaryOp::Shr, 8, false),
            TokenKind::Plus => (BinaryOp::Add, 9, false),
            TokenKind::Minus => (BinaryOp::Sub, 9, false),
            TokenKind::Star => (BinaryOp::Mul, 10, false),
            TokenKind::Slash => (BinaryOp::Div, 10, false),
            TokenKind::Percent => (BinaryOp::Mod, 10, false),
            _ => return None,
        })
    }

    fn parse_unary(&mut self) -> PR<Expr> {
        let span = self.span();
        let kind = match &self.peek().kind {
            TokenKind::Minus => {
                self.advance();
                ExprKind::Unary(UnaryOp::Neg, Box::new(self.parse_unary()?))
            }
            TokenKind::Bang => {
                self.advance();
                ExprKind::Unary(UnaryOp::Not, Box::new(self.parse_unary()?))
            }
            TokenKind::Tilde => {
                self.advance();
                ExprKind::Unary(UnaryOp::BitNot, Box::new(self.parse_unary()?))
            }
            TokenKind::PlusPlus => {
                self.advance();
                ExprKind::Unary(UnaryOp::PreInc, Box::new(self.parse_unary()?))
            }
            TokenKind::MinusMinus => {
                self.advance();
                ExprKind::Unary(UnaryOp::PreDec, Box::new(self.parse_unary()?))
            }
            TokenKind::Ampersand => {
                self.advance();
                ExprKind::AddrOf(Box::new(self.parse_unary()?))
            }
            TokenKind::Star => {
                self.advance();
                ExprKind::Deref(Box::new(self.parse_unary()?))
            }
            TokenKind::Sizeof => {
                self.advance();
                if self.check(&TokenKind::LParen) && self.is_type_ahead() {
                    self.advance();
                    let ty = self.parse_type_spec()?;
                    self.eat(&TokenKind::RParen)?;
                    ExprKind::Sizeof(SizeofArg::Type(ty))
                } else {
                    ExprKind::Sizeof(SizeofArg::Expr(Box::new(self.parse_unary()?)))
                }
            }
            TokenKind::Identifier(name)
                if name == "__alignof" || name == "_Alignof" || name == "alignof" =>
            {
                self.advance();
                self.eat(&TokenKind::LParen)?;
                let ty = self.parse_type_spec()?;
                self.eat(&TokenKind::RParen)?;
                ExprKind::Alignof(SizeofArg::Type(ty))
            }
            TokenKind::LParen if self.is_cast_ahead() => {
                self.advance();
                let ty = self.parse_type_spec()?;
                self.eat(&TokenKind::RParen)?;
                ExprKind::Cast(ty, Box::new(self.parse_unary()?))
            }
            _ => return self.parse_postfix(),
        };
        Ok(Expr { kind, span })
    }

    fn is_type_ahead(&self) -> bool {
        match &self.peek2().kind {
            TokenKind::Void
            | TokenKind::Char
            | TokenKind::Short
            | TokenKind::Int
            | TokenKind::Long
            | TokenKind::Float
            | TokenKind::Double
            | TokenKind::Unsigned
            | TokenKind::Signed
            | TokenKind::Struct
            | TokenKind::Union
            | TokenKind::Enum => true,
            TokenKind::Identifier(name) => {
                name == "__int8"
                    || name == "__int16"
                    || name == "__int32"
                    || name == "__int64"
                    || self.typedef_names.contains(name)
            }
            _ => false,
        }
    }

    fn is_cast_ahead(&self) -> bool {
        self.is_type_ahead()
    }

    fn parse_postfix(&mut self) -> PR<Expr> {
        let span = self.span();
        let mut expr = self.parse_primary()?;

        loop {
            expr = match &self.peek().kind {
                TokenKind::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(&TokenKind::RParen) {
                        loop {
                            args.push(self.parse_assign_expr()?);
                            if !self.try_eat(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    self.eat(&TokenKind::RParen)?;
                    Expr {
                        kind: ExprKind::Call(Box::new(expr), args),
                        span: span.clone(),
                    }
                }
                TokenKind::LBracket => {
                    self.advance();
                    let idx = self.parse_expr()?;
                    self.eat(&TokenKind::RBracket)?;
                    Expr {
                        kind: ExprKind::Index(Box::new(expr), Box::new(idx)),
                        span: span.clone(),
                    }
                }
                TokenKind::Dot => {
                    self.advance();
                    let (field, _) = self.eat_ident()?;
                    Expr {
                        kind: ExprKind::Member(Box::new(expr), field),
                        span: span.clone(),
                    }
                }
                TokenKind::Arrow => {
                    self.advance();
                    let (field, _) = self.eat_ident()?;
                    Expr {
                        kind: ExprKind::Arrow(Box::new(expr), field),
                        span: span.clone(),
                    }
                }
                TokenKind::PlusPlus => {
                    self.advance();
                    Expr {
                        kind: ExprKind::Unary(UnaryOp::PostInc, Box::new(expr)),
                        span: span.clone(),
                    }
                }
                TokenKind::MinusMinus => {
                    self.advance();
                    Expr {
                        kind: ExprKind::Unary(UnaryOp::PostDec, Box::new(expr)),
                        span: span.clone(),
                    }
                }
                _ => break,
            };
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> PR<Expr> {
        let span = self.span();
        let kind = match self.peek().kind.clone() {
            TokenKind::IntLiteral(v) => {
                self.advance();
                ExprKind::IntLit(v)
            }
            TokenKind::FloatLiteral(v) => {
                self.advance();
                ExprKind::FloatLit(v)
            }
            TokenKind::StringLiteral(s) => {
                self.advance();
                ExprKind::StringLit(s)
            }
            TokenKind::CharLiteral(c) => {
                self.advance();
                ExprKind::CharLit(c)
            }
            TokenKind::Identifier(name) => {
                self.advance();
                if let Some(val) = self.enum_constants.get(&name) {
                    ExprKind::IntLit(*val)
                } else {
                    ExprKind::Ident(name)
                }
            }
            TokenKind::LParen => {
                self.advance();
                let e = self.parse_expr()?;
                self.eat(&TokenKind::RParen)?;
                return Ok(e);
            }
            _ => {
                return Err(ParseError::new(
                    format!("unexpected token in expression: {:?}", self.peek().kind),
                    span,
                ));
            }
        };
        Ok(Expr { kind, span })
    }
}

fn eval_const_expr(
    expr: &Expr,
    enum_constants: &std::collections::HashMap<String, i64>,
) -> Option<i64> {
    match &expr.kind {
        ExprKind::IntLit(val) => Some(*val),
        ExprKind::Ident(name) => enum_constants.get(name).copied(),
        ExprKind::Unary(UnaryOp::Neg, inner) => eval_const_expr(inner, enum_constants).map(|v| -v),
        ExprKind::Unary(UnaryOp::Not, inner) => {
            eval_const_expr(inner, enum_constants).map(|v| if v == 0 { 1 } else { 0 })
        }
        ExprKind::Unary(UnaryOp::BitNot, inner) => {
            eval_const_expr(inner, enum_constants).map(|v| !v)
        }
        ExprKind::Binary(op, lhs, rhs) => {
            let l = eval_const_expr(lhs, enum_constants)?;
            let r = eval_const_expr(rhs, enum_constants)?;
            match op {
                BinaryOp::Add => Some(l + r),
                BinaryOp::Sub => Some(l - r),
                BinaryOp::Mul => Some(l * r),
                BinaryOp::Div => {
                    if r != 0 {
                        Some(l / r)
                    } else {
                        None
                    }
                }
                BinaryOp::Mod => {
                    if r != 0 {
                        Some(l % r)
                    } else {
                        None
                    }
                }
                BinaryOp::BitAnd => Some(l & r),
                BinaryOp::BitOr => Some(l | r),
                BinaryOp::BitXor => Some(l ^ r),
                BinaryOp::Shl => Some(l << r),
                BinaryOp::Shr => Some(l >> r),
                _ => None,
            }
        }
        _ => None,
    }
}
