use crate::lexer::token::Span;
use crate::parser::ast::{
    AssignOp, BinaryOp, Decl, Expr, ExprKind, FieldDecl, ForInit, FuncDecl, FuncDef, Param,
    SizeofArg, Stmt, StmtKind, StorageClass, StructDef, TopLevel, TranslationUnit, TypeSpec,
    UnaryOp,
};
use crate::preprocessor::preprocessor::{MacroDef, PpToken};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;

fn write_u8<W: Write>(w: &mut W, v: u8) -> std::io::Result<()> {
    w.write_all(&[v])
}
fn read_u8<R: Read>(r: &mut R) -> std::io::Result<u8> {
    let mut buf = [0; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn write_u32<W: Write>(w: &mut W, v: u32) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn read_u32<R: Read>(r: &mut R) -> std::io::Result<u32> {
    let mut buf = [0; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn write_u64<W: Write>(w: &mut W, v: u64) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn read_u64<R: Read>(r: &mut R) -> std::io::Result<u64> {
    let mut buf = [0; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

fn write_f64<W: Write>(w: &mut W, v: f64) -> std::io::Result<()> {
    w.write_all(&v.to_le_bytes())
}
fn read_f64<R: Read>(r: &mut R) -> std::io::Result<f64> {
    let mut buf = [0; 8];
    r.read_exact(&mut buf)?;
    Ok(f64::from_le_bytes(buf))
}

fn write_str<W: Write>(w: &mut W, s: &str) -> std::io::Result<()> {
    write_u32(w, s.len() as u32)?;
    w.write_all(s.as_bytes())
}
fn read_str<R: Read>(r: &mut R) -> std::io::Result<String> {
    let len = read_u32(r)? as usize;
    let mut buf = vec![0; len];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn write_pp_token<W: Write>(w: &mut W, t: &PpToken) -> std::io::Result<()> {
    match t {
        PpToken::Identifier(s) => {
            write_u8(w, 0)?;
            write_str(w, s)?;
        }
        PpToken::Number(s) => {
            write_u8(w, 1)?;
            write_str(w, s)?;
        }
        PpToken::StringLit(s) => {
            write_u8(w, 2)?;
            write_str(w, s)?;
        }
        PpToken::CharLit(s) => {
            write_u8(w, 3)?;
            write_str(w, s)?;
        }
        PpToken::Punct(c) => {
            write_u8(w, 4)?;
            write_u32(w, *c as u32)?;
        }
        PpToken::DoubleHash => {
            write_u8(w, 5)?;
        }
        PpToken::Space(s) => {
            write_u8(w, 6)?;
            write_str(w, s)?;
        }
        PpToken::Newline => {
            write_u8(w, 7)?;
        }
    }
    Ok(())
}

fn read_pp_token<R: Read>(r: &mut R) -> std::io::Result<PpToken> {
    let tag = read_u8(r)?;
    match tag {
        0 => Ok(PpToken::Identifier(read_str(r)?)),
        1 => Ok(PpToken::Number(read_str(r)?)),
        2 => Ok(PpToken::StringLit(read_str(r)?)),
        3 => Ok(PpToken::CharLit(read_str(r)?)),
        4 => {
            let c = read_u32(r)?;
            Ok(PpToken::Punct(std::char::from_u32(c).unwrap_or(' ')))
        }
        5 => Ok(PpToken::DoubleHash),
        6 => Ok(PpToken::Space(read_str(r)?)),
        7 => Ok(PpToken::Newline),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid PpToken tag",
        )),
    }
}

fn write_macro_def<W: Write>(w: &mut W, m: &MacroDef) -> std::io::Result<()> {
    write_str(w, &m.name)?;
    match &m.args {
        None => write_u8(w, 0)?,
        Some(args) => {
            write_u8(w, 1)?;
            write_u32(w, args.len() as u32)?;
            for arg in args {
                write_str(w, arg)?;
            }
        }
    }
    write_u32(w, m.body.len() as u32)?;
    for t in &m.body {
        write_pp_token(w, t)?;
    }
    Ok(())
}

fn read_macro_def<R: Read>(r: &mut R) -> std::io::Result<MacroDef> {
    let name = read_str(r)?;
    let args = match read_u8(r)? {
        0 => None,
        1 => {
            let len = read_u32(r)? as usize;
            let mut args = Vec::new();
            for _ in 0..len {
                args.push(read_str(r)?);
            }
            Some(args)
        }
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid MacroDef args tag",
            ));
        }
    };
    let body_len = read_u32(r)? as usize;
    let mut body = Vec::new();
    for _ in 0..body_len {
        body.push(read_pp_token(r)?);
    }
    Ok(MacroDef { name, args, body })
}

fn write_span<W: Write>(w: &mut W, s: &Span) -> std::io::Result<()> {
    write_u32(w, s.line)?;
    write_u32(w, s.col)
}
fn read_span<R: Read>(r: &mut R) -> std::io::Result<Span> {
    let line = read_u32(r)?;
    let col = read_u32(r)?;
    Ok(Span { line, col })
}

fn write_type_spec<W: Write>(w: &mut W, t: &TypeSpec) -> std::io::Result<()> {
    match t {
        TypeSpec::Void => write_u8(w, 0)?,
        TypeSpec::Char => write_u8(w, 1)?,
        TypeSpec::Short => write_u8(w, 2)?,
        TypeSpec::Int => write_u8(w, 3)?,
        TypeSpec::Long => write_u8(w, 4)?,
        TypeSpec::LongLong => write_u8(w, 5)?,
        TypeSpec::Float => write_u8(w, 6)?,
        TypeSpec::Double => write_u8(w, 7)?,
        TypeSpec::Unsigned(ty) => {
            write_u8(w, 8)?;
            write_type_spec(w, ty)?;
        }
        TypeSpec::Signed(ty) => {
            write_u8(w, 9)?;
            write_type_spec(w, ty)?;
        }
        TypeSpec::Pointer(ty) => {
            write_u8(w, 10)?;
            write_type_spec(w, ty)?;
        }
        TypeSpec::Array(ty, size) => {
            write_u8(w, 11)?;
            write_type_spec(w, ty)?;
            match size {
                None => write_u8(w, 0)?,
                Some(e) => {
                    write_u8(w, 1)?;
                    write_expr(w, e)?;
                }
            }
        }
        TypeSpec::Struct(s) => {
            write_u8(w, 12)?;
            write_str(w, s)?;
        }
        TypeSpec::Union(s) => {
            write_u8(w, 13)?;
            write_str(w, s)?;
        }
        TypeSpec::Enum(s) => {
            write_u8(w, 14)?;
            write_str(w, s)?;
        }
        TypeSpec::Named(s) => {
            write_u8(w, 15)?;
            write_str(w, s)?;
        }
    }
    Ok(())
}

fn read_type_spec<R: Read>(r: &mut R) -> std::io::Result<TypeSpec> {
    let tag = read_u8(r)?;
    match tag {
        0 => Ok(TypeSpec::Void),
        1 => Ok(TypeSpec::Char),
        2 => Ok(TypeSpec::Short),
        3 => Ok(TypeSpec::Int),
        4 => Ok(TypeSpec::Long),
        5 => Ok(TypeSpec::LongLong),
        6 => Ok(TypeSpec::Float),
        7 => Ok(TypeSpec::Double),
        8 => Ok(TypeSpec::Unsigned(Box::new(read_type_spec(r)?))),
        9 => Ok(TypeSpec::Signed(Box::new(read_type_spec(r)?))),
        10 => Ok(TypeSpec::Pointer(Box::new(read_type_spec(r)?))),
        11 => {
            let ty = read_type_spec(r)?;
            let size = match read_u8(r)? {
                0 => None,
                1 => Some(Box::new(read_expr(r)?)),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid TypeSpec array tag",
                    ));
                }
            };
            Ok(TypeSpec::Array(Box::new(ty), size))
        }
        12 => Ok(TypeSpec::Struct(read_str(r)?)),
        13 => Ok(TypeSpec::Union(read_str(r)?)),
        14 => Ok(TypeSpec::Enum(read_str(r)?)),
        15 => Ok(TypeSpec::Named(read_str(r)?)),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid TypeSpec tag",
        )),
    }
}

fn write_expr<W: Write>(w: &mut W, e: &Expr) -> std::io::Result<()> {
    write_span(w, &e.span)?;
    write_expr_kind(w, &e.kind)
}

fn read_expr<R: Read>(r: &mut R) -> std::io::Result<Expr> {
    let span = read_span(r)?;
    let kind = read_expr_kind(r)?;
    Ok(Expr { kind, span })
}

fn write_expr_kind<W: Write>(w: &mut W, k: &ExprKind) -> std::io::Result<()> {
    match k {
        ExprKind::IntLit(v) => {
            write_u8(w, 0)?;
            write_u64(w, *v as u64)?;
        }
        ExprKind::FloatLit(v) => {
            write_u8(w, 1)?;
            write_f64(w, *v)?;
        }
        ExprKind::StringLit(s) => {
            write_u8(w, 2)?;
            write_str(w, s)?;
        }
        ExprKind::CharLit(c) => {
            write_u8(w, 3)?;
            write_u32(w, *c as u32)?;
        }
        ExprKind::Ident(s) => {
            write_u8(w, 4)?;
            write_str(w, s)?;
        }
        ExprKind::Unary(op, e) => {
            write_u8(w, 5)?;
            write_unary_op(w, op)?;
            write_expr(w, e)?;
        }
        ExprKind::Binary(op, l, r) => {
            write_u8(w, 6)?;
            write_binary_op(w, op)?;
            write_expr(w, l)?;
            write_expr(w, r)?;
        }
        ExprKind::Assign(op, l, r) => {
            write_u8(w, 7)?;
            write_assign_op(w, op)?;
            write_expr(w, l)?;
            write_expr(w, r)?;
        }
        ExprKind::Ternary(c, t, e) => {
            write_u8(w, 8)?;
            write_expr(w, c)?;
            write_expr(w, t)?;
            write_expr(w, e)?;
        }
        ExprKind::Call(f, args) => {
            write_u8(w, 9)?;
            write_expr(w, f)?;
            write_u32(w, args.len() as u32)?;
            for arg in args {
                write_expr(w, arg)?;
            }
        }
        ExprKind::Index(b, i) => {
            write_u8(w, 10)?;
            write_expr(w, b)?;
            write_expr(w, i)?;
        }
        ExprKind::Member(b, s) => {
            write_u8(w, 11)?;
            write_expr(w, b)?;
            write_str(w, s)?;
        }
        ExprKind::Arrow(b, s) => {
            write_u8(w, 12)?;
            write_expr(w, b)?;
            write_str(w, s)?;
        }
        ExprKind::Cast(ts, e) => {
            write_u8(w, 13)?;
            write_type_spec(w, ts)?;
            write_expr(w, e)?;
        }
        ExprKind::Sizeof(arg) => {
            write_u8(w, 14)?;
            match arg {
                SizeofArg::Expr(e) => {
                    write_u8(w, 0)?;
                    write_expr(w, e)?;
                }
                SizeofArg::Type(ts) => {
                    write_u8(w, 1)?;
                    write_type_spec(w, ts)?;
                }
            }
        }
        ExprKind::Alignof(arg) => {
            write_u8(w, 17)?;
            match arg {
                SizeofArg::Expr(e) => {
                    write_u8(w, 0)?;
                    write_expr(w, e)?;
                }
                SizeofArg::Type(ts) => {
                    write_u8(w, 1)?;
                    write_type_spec(w, ts)?;
                }
            }
        }
        ExprKind::AddrOf(e) => {
            write_u8(w, 15)?;
            write_expr(w, e)?;
        }
        ExprKind::Deref(e) => {
            write_u8(w, 16)?;
            write_expr(w, e)?;
        }
    }
    Ok(())
}

fn read_expr_kind<R: Read>(r: &mut R) -> std::io::Result<ExprKind> {
    let tag = read_u8(r)?;
    match tag {
        0 => Ok(ExprKind::IntLit(read_u64(r)? as i64)),
        1 => Ok(ExprKind::FloatLit(read_f64(r)?)),
        2 => Ok(ExprKind::StringLit(read_str(r)?)),
        3 => {
            let c = read_u32(r)?;
            Ok(ExprKind::CharLit(std::char::from_u32(c).unwrap_or(' ')))
        }
        4 => Ok(ExprKind::Ident(read_str(r)?)),
        5 => {
            let op = read_unary_op(r)?;
            let e = read_expr(r)?;
            Ok(ExprKind::Unary(op, Box::new(e)))
        }
        6 => {
            let op = read_binary_op(r)?;
            let l = read_expr(r)?;
            let r = read_expr(r)?;
            Ok(ExprKind::Binary(op, Box::new(l), Box::new(r)))
        }
        7 => {
            let op = read_assign_op(r)?;
            let l = read_expr(r)?;
            let r_expr = read_expr(r)?;
            Ok(ExprKind::Assign(op, Box::new(l), Box::new(r_expr)))
        }
        8 => {
            let c = read_expr(r)?;
            let t = read_expr(r)?;
            let e = read_expr(r)?;
            Ok(ExprKind::Ternary(Box::new(c), Box::new(t), Box::new(e)))
        }
        9 => {
            let f = read_expr(r)?;
            let len = read_u32(r)? as usize;
            let mut args = Vec::new();
            for _ in 0..len {
                args.push(read_expr(r)?);
            }
            Ok(ExprKind::Call(Box::new(f), args))
        }
        10 => {
            let b = read_expr(r)?;
            let i = read_expr(r)?;
            Ok(ExprKind::Index(Box::new(b), Box::new(i)))
        }
        11 => {
            let b = read_expr(r)?;
            let s = read_str(r)?;
            Ok(ExprKind::Member(Box::new(b), s))
        }
        12 => {
            let b = read_expr(r)?;
            let s = read_str(r)?;
            Ok(ExprKind::Arrow(Box::new(b), s))
        }
        13 => {
            let ts = read_type_spec(r)?;
            let e = read_expr(r)?;
            Ok(ExprKind::Cast(ts, Box::new(e)))
        }
        14 => {
            let arg = match read_u8(r)? {
                0 => SizeofArg::Expr(Box::new(read_expr(r)?)),
                1 => SizeofArg::Type(read_type_spec(r)?),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid SizeofArg tag",
                    ));
                }
            };
            Ok(ExprKind::Sizeof(arg))
        }
        15 => Ok(ExprKind::AddrOf(Box::new(read_expr(r)?))),
        16 => Ok(ExprKind::Deref(Box::new(read_expr(r)?))),
        17 => {
            let arg = match read_u8(r)? {
                0 => SizeofArg::Expr(Box::new(read_expr(r)?)),
                1 => SizeofArg::Type(read_type_spec(r)?),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid SizeofArg tag",
                    ));
                }
            };
            Ok(ExprKind::Alignof(arg))
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid ExprKind tag",
        )),
    }
}

fn write_unary_op<W: Write>(w: &mut W, op: &UnaryOp) -> std::io::Result<()> {
    let v = match op {
        UnaryOp::Neg => 0,
        UnaryOp::Not => 1,
        UnaryOp::BitNot => 2,
        UnaryOp::PreInc => 3,
        UnaryOp::PreDec => 4,
        UnaryOp::PostInc => 5,
        UnaryOp::PostDec => 6,
    };
    write_u8(w, v)
}
fn read_unary_op<R: Read>(r: &mut R) -> std::io::Result<UnaryOp> {
    let tag = read_u8(r)?;
    match tag {
        0 => Ok(UnaryOp::Neg),
        1 => Ok(UnaryOp::Not),
        2 => Ok(UnaryOp::BitNot),
        3 => Ok(UnaryOp::PreInc),
        4 => Ok(UnaryOp::PreDec),
        5 => Ok(UnaryOp::PostInc),
        6 => Ok(UnaryOp::PostDec),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid UnaryOp",
        )),
    }
}

fn write_binary_op<W: Write>(w: &mut W, op: &BinaryOp) -> std::io::Result<()> {
    let v = match op {
        BinaryOp::Add => 0,
        BinaryOp::Sub => 1,
        BinaryOp::Mul => 2,
        BinaryOp::Div => 3,
        BinaryOp::Mod => 4,
        BinaryOp::BitAnd => 5,
        BinaryOp::BitOr => 6,
        BinaryOp::BitXor => 7,
        BinaryOp::Shl => 8,
        BinaryOp::Shr => 9,
        BinaryOp::And => 10,
        BinaryOp::Or => 11,
        BinaryOp::Eq => 12,
        BinaryOp::Ne => 13,
        BinaryOp::Lt => 14,
        BinaryOp::Gt => 15,
        BinaryOp::Le => 16,
        BinaryOp::Ge => 17,
        BinaryOp::Comma => 18,
    };
    write_u8(w, v)
}
fn read_binary_op<R: Read>(r: &mut R) -> std::io::Result<BinaryOp> {
    let tag = read_u8(r)?;
    match tag {
        0 => Ok(BinaryOp::Add),
        1 => Ok(BinaryOp::Sub),
        2 => Ok(BinaryOp::Mul),
        3 => Ok(BinaryOp::Div),
        4 => Ok(BinaryOp::Mod),
        5 => Ok(BinaryOp::BitAnd),
        6 => Ok(BinaryOp::BitOr),
        7 => Ok(BinaryOp::BitXor),
        8 => Ok(BinaryOp::Shl),
        9 => Ok(BinaryOp::Shr),
        10 => Ok(BinaryOp::And),
        11 => Ok(BinaryOp::Or),
        12 => Ok(BinaryOp::Eq),
        13 => Ok(BinaryOp::Ne),
        14 => Ok(BinaryOp::Lt),
        15 => Ok(BinaryOp::Gt),
        16 => Ok(BinaryOp::Le),
        17 => Ok(BinaryOp::Ge),
        18 => Ok(BinaryOp::Comma),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid BinaryOp",
        )),
    }
}

fn write_assign_op<W: Write>(w: &mut W, op: &AssignOp) -> std::io::Result<()> {
    let v = match op {
        AssignOp::Assign => 0,
        AssignOp::AddAssign => 1,
        AssignOp::SubAssign => 2,
        AssignOp::MulAssign => 3,
        AssignOp::DivAssign => 4,
        AssignOp::ModAssign => 5,
        AssignOp::AndAssign => 6,
        AssignOp::OrAssign => 7,
        AssignOp::XorAssign => 8,
        AssignOp::ShlAssign => 9,
        AssignOp::ShrAssign => 10,
    };
    write_u8(w, v)
}
fn read_assign_op<R: Read>(r: &mut R) -> std::io::Result<AssignOp> {
    let tag = read_u8(r)?;
    match tag {
        0 => Ok(AssignOp::Assign),
        1 => Ok(AssignOp::AddAssign),
        2 => Ok(AssignOp::SubAssign),
        3 => Ok(AssignOp::MulAssign),
        4 => Ok(AssignOp::DivAssign),
        5 => Ok(AssignOp::ModAssign),
        6 => Ok(AssignOp::AndAssign),
        7 => Ok(AssignOp::OrAssign),
        8 => Ok(AssignOp::XorAssign),
        9 => Ok(AssignOp::ShlAssign),
        10 => Ok(AssignOp::ShrAssign),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid AssignOp",
        )),
    }
}

fn write_stmt<W: Write>(w: &mut W, s: &Stmt) -> std::io::Result<()> {
    write_span(w, &s.span)?;
    write_stmt_kind(w, &s.kind)
}

fn read_stmt<R: Read>(r: &mut R) -> std::io::Result<Stmt> {
    let span = read_span(r)?;
    let kind = read_stmt_kind(r)?;
    Ok(Stmt { kind, span })
}

fn write_stmt_kind<W: Write>(w: &mut W, k: &StmtKind) -> std::io::Result<()> {
    match k {
        StmtKind::Decl(d) => {
            write_u8(w, 0)?;
            write_decl(w, d)?;
        }
        StmtKind::Expr(e) => {
            write_u8(w, 1)?;
            write_expr(w, e)?;
        }
        StmtKind::Block(b) => {
            write_u8(w, 2)?;
            write_u32(w, b.len() as u32)?;
            for s in b {
                write_stmt(w, s)?;
            }
        }
        StmtKind::Return(e) => {
            write_u8(w, 3)?;
            match e {
                None => write_u8(w, 0)?,
                Some(expr) => {
                    write_u8(w, 1)?;
                    write_expr(w, expr)?;
                }
            }
        }
        StmtKind::If(c, t, el) => {
            write_u8(w, 4)?;
            write_expr(w, c)?;
            write_stmt(w, t)?;
            match el {
                None => write_u8(w, 0)?,
                Some(s) => {
                    write_u8(w, 1)?;
                    write_stmt(w, s)?;
                }
            }
        }
        StmtKind::While(c, s) => {
            write_u8(w, 5)?;
            write_expr(w, c)?;
            write_stmt(w, s)?;
        }
        StmtKind::DoWhile(s, c) => {
            write_u8(w, 6)?;
            write_stmt(w, s)?;
            write_expr(w, c)?;
        }
        StmtKind::For(init, c, p, s) => {
            write_u8(w, 7)?;
            match init {
                None => write_u8(w, 0)?,
                Some(ForInit::Decl(d)) => {
                    write_u8(w, 1)?;
                    write_decl(w, d)?;
                }
                Some(ForInit::Expr(e)) => {
                    write_u8(w, 2)?;
                    write_expr(w, e)?;
                }
            }
            match c {
                None => write_u8(w, 0)?,
                Some(expr) => {
                    write_u8(w, 1)?;
                    write_expr(w, expr)?;
                }
            }
            match p {
                None => write_u8(w, 0)?,
                Some(expr) => {
                    write_u8(w, 1)?;
                    write_expr(w, expr)?;
                }
            }
            write_stmt(w, s)?;
        }
        StmtKind::Break => {
            write_u8(w, 8)?;
        }
        StmtKind::Continue => {
            write_u8(w, 9)?;
        }
        StmtKind::Goto(s) => {
            write_u8(w, 10)?;
            write_str(w, s)?;
        }
        StmtKind::Label(s, st) => {
            write_u8(w, 11)?;
            write_str(w, s)?;
            write_stmt(w, st)?;
        }
        StmtKind::Switch(e, s) => {
            write_u8(w, 12)?;
            write_expr(w, e)?;
            write_stmt(w, s)?;
        }
        StmtKind::Case(e, s) => {
            write_u8(w, 13)?;
            write_expr(w, e)?;
            write_stmt(w, s)?;
        }
        StmtKind::Default(s) => {
            write_u8(w, 14)?;
            write_stmt(w, s)?;
        }
    }
    Ok(())
}

fn read_stmt_kind<R: Read>(r: &mut R) -> std::io::Result<StmtKind> {
    let tag = read_u8(r)?;
    match tag {
        0 => Ok(StmtKind::Decl(read_decl(r)?)),
        1 => Ok(StmtKind::Expr(read_expr(r)?)),
        2 => {
            let len = read_u32(r)? as usize;
            let mut b = Vec::new();
            for _ in 0..len {
                b.push(read_stmt(r)?);
            }
            Ok(StmtKind::Block(b))
        }
        3 => {
            let e = match read_u8(r)? {
                0 => None,
                1 => Some(read_expr(r)?),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid StmtKind return tag",
                    ));
                }
            };
            Ok(StmtKind::Return(e))
        }
        4 => {
            let c = read_expr(r)?;
            let t = read_stmt(r)?;
            let el = match read_u8(r)? {
                0 => None,
                1 => Some(Box::new(read_stmt(r)?)),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid StmtKind if tag",
                    ));
                }
            };
            Ok(StmtKind::If(c, Box::new(t), el))
        }
        5 => {
            let c = read_expr(r)?;
            let s = read_stmt(r)?;
            Ok(StmtKind::While(c, Box::new(s)))
        }
        6 => {
            let s = read_stmt(r)?;
            let c = read_expr(r)?;
            Ok(StmtKind::DoWhile(Box::new(s), c))
        }
        7 => {
            let init = match read_u8(r)? {
                0 => None,
                1 => Some(ForInit::Decl(read_decl(r)?)),
                2 => Some(ForInit::Expr(read_expr(r)?)),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid StmtKind for init tag",
                    ));
                }
            };
            let c = match read_u8(r)? {
                0 => None,
                1 => Some(read_expr(r)?),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid StmtKind for cond tag",
                    ));
                }
            };
            let p = match read_u8(r)? {
                0 => None,
                1 => Some(read_expr(r)?),
                _ => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "invalid StmtKind for post tag",
                    ));
                }
            };
            let s = read_stmt(r)?;
            Ok(StmtKind::For(init, c, p, Box::new(s)))
        }
        8 => Ok(StmtKind::Break),
        9 => Ok(StmtKind::Continue),
        10 => Ok(StmtKind::Goto(read_str(r)?)),
        11 => {
            let s = read_str(r)?;
            let st = read_stmt(r)?;
            Ok(StmtKind::Label(s, Box::new(st)))
        }
        12 => {
            let e = read_expr(r)?;
            let s = read_stmt(r)?;
            Ok(StmtKind::Switch(e, Box::new(s)))
        }
        13 => {
            let e = read_expr(r)?;
            let s = read_stmt(r)?;
            Ok(StmtKind::Case(e, Box::new(s)))
        }
        14 => {
            let s = read_stmt(r)?;
            Ok(StmtKind::Default(Box::new(s)))
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid StmtKind tag",
        )),
    }
}

fn write_storage_class<W: Write>(w: &mut W, sc: &StorageClass) -> std::io::Result<()> {
    let v = match sc {
        StorageClass::None => 0,
        StorageClass::Extern => 1,
        StorageClass::Static => 2,
        StorageClass::Auto => 3,
        StorageClass::Register => 4,
    };
    write_u8(w, v)
}
fn read_storage_class<R: Read>(r: &mut R) -> std::io::Result<StorageClass> {
    let tag = read_u8(r)?;
    match tag {
        0 => Ok(StorageClass::None),
        1 => Ok(StorageClass::Extern),
        2 => Ok(StorageClass::Static),
        3 => Ok(StorageClass::Auto),
        4 => Ok(StorageClass::Register),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid StorageClass",
        )),
    }
}

fn write_decl<W: Write>(w: &mut W, d: &Decl) -> std::io::Result<()> {
    write_storage_class(w, &d.storage)?;
    write_type_spec(w, &d.ty)?;
    write_str(w, &d.name)?;
    match &d.init {
        None => write_u8(w, 0)?,
        Some(e) => {
            write_u8(w, 1)?;
            write_expr(w, e)?;
        }
    }
    write_span(w, &d.span)
}
fn read_decl<R: Read>(r: &mut R) -> std::io::Result<Decl> {
    let storage = read_storage_class(r)?;
    let ty = read_type_spec(r)?;
    let name = read_str(r)?;
    let init = match read_u8(r)? {
        0 => None,
        1 => Some(read_expr(r)?),
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid Decl init tag",
            ));
        }
    };
    let span = read_span(r)?;
    Ok(Decl {
        storage,
        ty,
        name,
        init,
        span,
    })
}

fn write_param<W: Write>(w: &mut W, p: &Param) -> std::io::Result<()> {
    write_type_spec(w, &p.ty)?;
    match &p.name {
        None => write_u8(w, 0)?,
        Some(s) => {
            write_u8(w, 1)?;
            write_str(w, s)?;
        }
    }
    write_span(w, &p.span)
}
fn read_param<R: Read>(r: &mut R) -> std::io::Result<Param> {
    let ty = read_type_spec(r)?;
    let name = match read_u8(r)? {
        0 => None,
        1 => Some(read_str(r)?),
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid Param name tag",
            ));
        }
    };
    let span = read_span(r)?;
    Ok(Param { ty, name, span })
}

fn write_field_decl<W: Write>(w: &mut W, f: &FieldDecl) -> std::io::Result<()> {
    write_type_spec(w, &f.ty)?;
    write_str(w, &f.name)?;
    write_span(w, &f.span)
}
fn read_field_decl<R: Read>(r: &mut R) -> std::io::Result<FieldDecl> {
    let ty = read_type_spec(r)?;
    let name = read_str(r)?;
    let span = read_span(r)?;
    Ok(FieldDecl { ty, name, span })
}

fn write_struct_def<W: Write>(w: &mut W, s: &StructDef) -> std::io::Result<()> {
    write_str(w, &s.name)?;
    write_u32(w, s.fields.len() as u32)?;
    for f in &s.fields {
        write_field_decl(w, f)?;
    }
    write_u8(w, if s.is_union { 1 } else { 0 })?;
    write_span(w, &s.span)?;
    write_u64(w, s.pack)
}
fn read_struct_def<R: Read>(r: &mut R) -> std::io::Result<StructDef> {
    let name = read_str(r)?;
    let len = read_u32(r)? as usize;
    let mut fields = Vec::new();
    for _ in 0..len {
        fields.push(read_field_decl(r)?);
    }
    let is_union = read_u8(r)? != 0;
    let span = read_span(r)?;
    let pack = read_u64(r)?;
    Ok(StructDef {
        name,
        fields,
        is_union,
        span,
        pack,
    })
}

fn write_func_decl<W: Write>(w: &mut W, fd: &FuncDecl) -> std::io::Result<()> {
    write_str(w, &fd.name)?;
    write_type_spec(w, &fd.ret_ty)?;
    write_u32(w, fd.params.len() as u32)?;
    for p in &fd.params {
        write_param(w, p)?;
    }
    write_u8(w, if fd.variadic { 1 } else { 0 })?;
    write_storage_class(w, &fd.storage)?;
    write_span(w, &fd.span)
}
fn read_func_decl<R: Read>(r: &mut R) -> std::io::Result<FuncDecl> {
    let name = read_str(r)?;
    let ret_ty = read_type_spec(r)?;
    let len = read_u32(r)? as usize;
    let mut params = Vec::new();
    for _ in 0..len {
        params.push(read_param(r)?);
    }
    let variadic = read_u8(r)? != 0;
    let storage = read_storage_class(r)?;
    let span = read_span(r)?;
    Ok(FuncDecl {
        name,
        ret_ty,
        params,
        variadic,
        storage,
        span,
    })
}

fn write_func_def<W: Write>(w: &mut W, fd: &FuncDef) -> std::io::Result<()> {
    write_type_spec(w, &fd.ret_ty)?;
    write_str(w, &fd.name)?;
    write_u32(w, fd.params.len() as u32)?;
    for p in &fd.params {
        write_param(w, p)?;
    }
    write_u8(w, if fd.variadic { 1 } else { 0 })?;
    write_u32(w, fd.body.len() as u32)?;
    for s in &fd.body {
        write_stmt(w, s)?;
    }
    write_span(w, &fd.span)
}
fn read_func_def<R: Read>(r: &mut R) -> std::io::Result<FuncDef> {
    let ret_ty = read_type_spec(r)?;
    let name = read_str(r)?;
    let len = read_u32(r)? as usize;
    let mut params = Vec::new();
    for _ in 0..len {
        params.push(read_param(r)?);
    }
    let variadic = read_u8(r)? != 0;
    let body_len = read_u32(r)? as usize;
    let mut body = Vec::new();
    for _ in 0..body_len {
        body.push(read_stmt(r)?);
    }
    let span = read_span(r)?;
    Ok(FuncDef {
        ret_ty,
        name,
        params,
        variadic,
        body,
        span,
    })
}

fn write_top_level<W: Write>(w: &mut W, tl: &TopLevel) -> std::io::Result<()> {
    match tl {
        TopLevel::Function(fd) => {
            write_u8(w, 0)?;
            write_func_def(w, fd)?;
        }
        TopLevel::FuncDecl(fd) => {
            write_u8(w, 1)?;
            write_func_decl(w, fd)?;
        }
        TopLevel::Declaration(d) => {
            write_u8(w, 2)?;
            write_decl(w, d)?;
        }
        TopLevel::StructDef(s) => {
            write_u8(w, 3)?;
            write_struct_def(w, s)?;
        }
        TopLevel::Typedef(ts, s, span) => {
            write_u8(w, 4)?;
            write_type_spec(w, ts)?;
            write_str(w, s)?;
            write_span(w, span)?;
        }
    }
    Ok(())
}
fn read_top_level<R: Read>(r: &mut R) -> std::io::Result<TopLevel> {
    let tag = read_u8(r)?;
    match tag {
        0 => Ok(TopLevel::Function(read_func_def(r)?)),
        1 => Ok(TopLevel::FuncDecl(read_func_decl(r)?)),
        2 => Ok(TopLevel::Declaration(read_decl(r)?)),
        3 => Ok(TopLevel::StructDef(read_struct_def(r)?)),
        4 => {
            let ts = read_type_spec(r)?;
            let s = read_str(r)?;
            let span = read_span(r)?;
            Ok(TopLevel::Typedef(ts, s, span))
        }
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid TopLevel tag",
        )),
    }
}

pub fn save_pch(
    path: &Path,
    defines: &HashMap<String, MacroDef>,
    typedef_names: &HashSet<String>,
    tu: &TranslationUnit,
) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut writer = std::io::BufWriter::new(file);

    writer.write_all(b"COMPPCH\x01")?;

    write_u32(&mut writer, defines.len() as u32)?;
    for (k, v) in defines {
        write_str(&mut writer, k)?;
        write_macro_def(&mut writer, v)?;
    }

    write_u32(&mut writer, typedef_names.len() as u32)?;
    for k in typedef_names {
        write_str(&mut writer, k)?;
    }

    write_u32(&mut writer, tu.items.len() as u32)?;
    for item in &tu.items {
        write_top_level(&mut writer, item)?;
    }

    write_u32(&mut writer, tu.enum_constants.len() as u32)?;
    for (k, v) in &tu.enum_constants {
        write_str(&mut writer, k)?;
        write_u64(&mut writer, *v as u64)?;
    }

    writer.flush()
}

pub fn load_pch(
    path: &Path,
    defines: &mut HashMap<String, MacroDef>,
    typedef_names: &mut HashSet<String>,
) -> std::io::Result<TranslationUnit> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);

    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    if &magic != b"COMPPCH\x01" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid PCH magic",
        ));
    }

    let defines_len = read_u32(&mut reader)? as usize;
    for _ in 0..defines_len {
        let k = read_str(&mut reader)?;
        let v = read_macro_def(&mut reader)?;
        defines.insert(k, v);
    }

    let typedef_len = read_u32(&mut reader)? as usize;
    for _ in 0..typedef_len {
        typedef_names.insert(read_str(&mut reader)?);
    }

    let items_len = read_u32(&mut reader)? as usize;
    let mut items = Vec::new();
    for _ in 0..items_len {
        items.push(read_top_level(&mut reader)?);
    }

    let enum_len = read_u32(&mut reader)? as usize;
    let mut enum_constants = HashMap::new();
    for _ in 0..enum_len {
        let k = read_str(&mut reader)?;
        let v = read_u64(&mut reader)? as i64;
        enum_constants.insert(k, v);
    }

    Ok(TranslationUnit {
        items,
        enum_constants,
    })
}
