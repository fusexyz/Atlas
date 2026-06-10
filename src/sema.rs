use crate::lexer::token::Span;
use crate::parser::ast::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct SemanticError {
    span: Span,
    message: String,
}

impl SemanticError {
    fn new(span: &Span, message: impl Into<String>) -> Self {
        Self {
            span: span.clone(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SemanticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "type error at {}:{}: {}",
            self.span.line, self.span.col, self.message
        )
    }
}

type SR<T> = Result<T, SemanticError>;

#[derive(Debug, Clone)]
struct FuncSig {
    ret: TypeSpec,
    params: Vec<TypeSpec>,
    variadic: bool,
}

#[derive(Debug, Clone)]
struct StructInfo {
    fields: HashMap<String, TypeSpec>,
}

#[derive(Debug, Clone)]
struct ExprInfo {
    ty: TypeSpec,
    lvalue: bool,
}

pub fn check_translation_unit(tu: &TranslationUnit) -> SR<()> {
    let mut checker = Checker::new(tu);
    checker.collect()?;
    checker.check()
}

struct Checker<'a> {
    tu: &'a TranslationUnit,
    typedefs: HashMap<String, TypeSpec>,
    structs: HashMap<String, StructInfo>,
    funcs: HashMap<String, FuncSig>,
    globals: HashMap<String, TypeSpec>,
    enum_constants: HashSet<String>,
    scopes: Vec<HashMap<String, TypeSpec>>,
    current_ret: Option<TypeSpec>,
}

impl<'a> Checker<'a> {
    fn new(tu: &'a TranslationUnit) -> Self {
        Self {
            tu,
            typedefs: HashMap::new(),
            structs: HashMap::new(),
            funcs: HashMap::new(),
            globals: HashMap::new(),
            enum_constants: tu.enum_constants.keys().cloned().collect(),
            scopes: Vec::new(),
            current_ret: None,
        }
    }

    fn collect(&mut self) -> SR<()> {
        for item in &self.tu.items {
            if let TopLevel::Typedef(ty, name, _) = item {
                self.typedefs.insert(name.clone(), ty.clone());
            }
        }

        for item in &self.tu.items {
            if let TopLevel::StructDef(def) = item {
                let mut fields = HashMap::new();
                for field in &def.fields {
                    self.resolve_type(&field.ty, &field.span)?;
                    fields.insert(field.name.clone(), field.ty.clone());
                }
                self.structs.insert(def.name.clone(), StructInfo { fields });
            }
        }

        for item in &self.tu.items {
            match item {
                TopLevel::Function(f) => {
                    self.resolve_type(&f.ret_ty, &f.span)?;
                    let params = self.param_types(&f.params)?;
                    self.funcs.insert(
                        f.name.clone(),
                        FuncSig {
                            ret: f.ret_ty.clone(),
                            params,
                            variadic: f.variadic,
                        },
                    );
                }
                TopLevel::FuncDecl(f) => {
                    self.resolve_type(&f.ret_ty, &f.span)?;
                    let params = self.param_types(&f.params)?;
                    self.funcs.insert(
                        f.name.clone(),
                        FuncSig {
                            ret: f.ret_ty.clone(),
                            params,
                            variadic: f.variadic,
                        },
                    );
                }
                TopLevel::Declaration(d) => {
                    self.resolve_type(&d.ty, &d.span)?;
                    self.globals.insert(d.name.clone(), d.ty.clone());
                }
                TopLevel::StructDef(_) | TopLevel::Typedef(_, _, _) => {}
            }
        }
        Ok(())
    }

    fn param_types(&self, params: &[Param]) -> SR<Vec<TypeSpec>> {
        if params.len() == 1 && self.is_void(&params[0].ty)? {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for p in params {
            self.resolve_type(&p.ty, &p.span)?;
            out.push(p.ty.clone());
        }
        Ok(out)
    }

    fn check(&mut self) -> SR<()> {
        for item in &self.tu.items {
            match item {
                TopLevel::Function(f) => self.check_function(f)?,
                TopLevel::Declaration(d) => {
                    if let Some(init) = &d.init {
                        let rhs = self.check_expr(init)?;
                        self.require_assignable(&d.ty, &rhs.ty, &d.span)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn check_function(&mut self, f: &FuncDef) -> SR<()> {
        self.scopes.clear();
        self.push_scope();
        self.current_ret = Some(f.ret_ty.clone());
        let mut seen = HashSet::new();
        for p in &f.params {
            if let Some(name) = &p.name {
                if !seen.insert(name.clone()) {
                    return Err(SemanticError::new(
                        &p.span,
                        format!("duplicate parameter '{}'", name),
                    ));
                }
                self.declare_local(name, p.ty.clone(), &p.span)?;
            }
        }
        self.check_block(&f.body)?;
        self.current_ret = None;
        self.scopes.clear();
        Ok(())
    }

    fn check_block(&mut self, block: &Block) -> SR<()> {
        self.push_scope();
        for stmt in block {
            self.check_stmt(stmt)?;
        }
        self.pop_scope();
        Ok(())
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> SR<()> {
        match &stmt.kind {
            StmtKind::Decl(d) => {
                self.resolve_type(&d.ty, &d.span)?;
                if let Some(init) = &d.init {
                    let rhs = self.check_expr(init)?;
                    self.require_assignable(&d.ty, &rhs.ty, &d.span)?;
                }
                self.declare_local(&d.name, d.ty.clone(), &d.span)
            }
            StmtKind::Expr(e) => {
                self.check_expr(e)?;
                Ok(())
            }
            StmtKind::Block(b) => self.check_block(b),
            StmtKind::Return(None) => {
                let ret = self.current_ret.clone().unwrap_or(TypeSpec::Void);
                if self.is_void(&ret)? {
                    Ok(())
                } else {
                    Err(SemanticError::new(
                        &stmt.span,
                        format!(
                            "return without value in function returning {}",
                            self.type_name(&ret)
                        ),
                    ))
                }
            }
            StmtKind::Return(Some(e)) => {
                let ret = self.current_ret.clone().unwrap_or(TypeSpec::Void);
                if self.is_void(&ret)? {
                    return Err(SemanticError::new(
                        &stmt.span,
                        "return with value in void function",
                    ));
                }
                let val = self.check_expr(e)?;
                self.require_assignable(&ret, &val.ty, &e.span)
            }
            StmtKind::If(cond, then_s, else_s) => {
                self.require_scalar_expr(cond)?;
                self.check_stmt(then_s)?;
                if let Some(else_s) = else_s {
                    self.check_stmt(else_s)?;
                }
                Ok(())
            }
            StmtKind::While(cond, body) => {
                self.require_scalar_expr(cond)?;
                self.check_stmt(body)
            }
            StmtKind::DoWhile(body, cond) => {
                self.check_stmt(body)?;
                self.require_scalar_expr(cond)
            }
            StmtKind::For(init, cond, post, body) => {
                self.push_scope();
                if let Some(init) = init {
                    match init {
                        ForInit::Decl(d) => self.check_stmt(&Stmt {
                            kind: StmtKind::Decl(d.clone()),
                            span: d.span.clone(),
                        })?,
                        ForInit::Expr(e) => {
                            self.check_expr(e)?;
                        }
                    }
                }
                if let Some(cond) = cond {
                    self.require_scalar_expr(cond)?;
                }
                if let Some(post) = post {
                    self.check_expr(post)?;
                }
                self.check_stmt(body)?;
                self.pop_scope();
                Ok(())
            }
            StmtKind::Break | StmtKind::Continue | StmtKind::Goto(_) => Ok(()),
            StmtKind::Label(_, s) | StmtKind::Case(_, s) | StmtKind::Default(s) => {
                self.check_stmt(s)
            }
            StmtKind::Switch(e, body) => {
                let ty = self.check_expr(e)?.ty;
                if !self.is_integer(&ty)? {
                    return Err(SemanticError::new(
                        &e.span,
                        "switch expression must be integer",
                    ));
                }
                self.check_stmt(body)
            }
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> SR<ExprInfo> {
        match &expr.kind {
            ExprKind::IntLit(_) | ExprKind::CharLit(_) => Ok(ExprInfo {
                ty: TypeSpec::Int,
                lvalue: false,
            }),
            ExprKind::FloatLit(_) => Ok(ExprInfo {
                ty: TypeSpec::Double,
                lvalue: false,
            }),
            ExprKind::StringLit(_) => Ok(ExprInfo {
                ty: TypeSpec::Pointer(Box::new(TypeSpec::Char)),
                lvalue: false,
            }),
            ExprKind::Ident(name) => {
                if let Some(ty) = self.lookup_local(name) {
                    Ok(ExprInfo { ty, lvalue: true })
                } else if let Some(ty) = self.globals.get(name) {
                    Ok(ExprInfo {
                        ty: ty.clone(),
                        lvalue: true,
                    })
                } else if self.enum_constants.contains(name) {
                    Ok(ExprInfo {
                        ty: TypeSpec::Int,
                        lvalue: false,
                    })
                } else if let Some(sig) = self.funcs.get(name) {
                    Ok(ExprInfo {
                        ty: TypeSpec::Pointer(Box::new(TypeSpec::Void)),
                        lvalue: false,
                    })
                    .map(|mut info| {
                        info.ty = TypeSpec::Pointer(Box::new(sig.ret.clone()));
                        info
                    })
                } else {
                    Ok(ExprInfo {
                        ty: TypeSpec::Int,
                        lvalue: false,
                    })
                }
            }
            ExprKind::Unary(op, inner) => self.check_unary(expr, op, inner),
            ExprKind::Binary(op, lhs, rhs) => self.check_binary(expr, op, lhs, rhs),
            ExprKind::Assign(op, lhs, rhs) => self.check_assign(expr, op, lhs, rhs),
            ExprKind::Ternary(cond, then_e, else_e) => {
                self.require_scalar_expr(cond)?;
                let then_t = self.check_expr(then_e)?.ty;
                let else_t = self.check_expr(else_e)?.ty;
                if self.can_assign(&then_t, &else_t)? {
                    Ok(ExprInfo {
                        ty: then_t,
                        lvalue: false,
                    })
                } else if self.can_assign(&else_t, &then_t)? {
                    Ok(ExprInfo {
                        ty: else_t,
                        lvalue: false,
                    })
                } else {
                    Err(SemanticError::new(
                        &expr.span,
                        "ternary branches have incompatible types",
                    ))
                }
            }
            ExprKind::Call(func, args) => self.check_call(expr, func, args),
            ExprKind::Index(base, idx) => {
                let base_i = self.check_expr(base)?;
                let idx_i = self.check_expr(idx)?;
                if !self.is_integer(&idx_i.ty)? {
                    return Err(SemanticError::new(&idx.span, "array index must be integer"));
                }
                let base_t = self.resolve_type(&base_i.ty, &base.span)?;
                match base_t {
                    TypeSpec::Pointer(inner) | TypeSpec::Array(inner, _) => Ok(ExprInfo {
                        ty: *inner,
                        lvalue: true,
                    }),
                    _ => Err(SemanticError::new(
                        &base.span,
                        "indexing requires pointer or array",
                    )),
                }
            }
            ExprKind::Member(base, field) => self.check_member(expr, base, field, false),
            ExprKind::Arrow(base, field) => self.check_member(expr, base, field, true),
            ExprKind::Cast(ty, inner) => {
                self.resolve_type(ty, &expr.span)?;
                self.check_expr(inner)?;
                Ok(ExprInfo {
                    ty: ty.clone(),
                    lvalue: false,
                })
            }
            ExprKind::Sizeof(_) | ExprKind::Alignof(_) => Ok(ExprInfo {
                ty: TypeSpec::Unsigned(Box::new(TypeSpec::LongLong)),
                lvalue: false,
            }),
            ExprKind::AddrOf(inner) => {
                let inner_i = self.check_expr(inner)?;
                if !inner_i.lvalue {
                    return Err(SemanticError::new(
                        &inner.span,
                        "address-of requires an lvalue",
                    ));
                }
                Ok(ExprInfo {
                    ty: TypeSpec::Pointer(Box::new(inner_i.ty)),
                    lvalue: false,
                })
            }
            ExprKind::Deref(inner) => {
                let inner_i = self.check_expr(inner)?;
                let inner_t = self.resolve_type(&inner_i.ty, &inner.span)?;
                match inner_t {
                    TypeSpec::Pointer(pointee) => Ok(ExprInfo {
                        ty: *pointee,
                        lvalue: true,
                    }),
                    _ => Err(SemanticError::new(
                        &expr.span,
                        "dereference requires a pointer",
                    )),
                }
            }
        }
    }

    fn check_unary(&mut self, expr: &Expr, op: &UnaryOp, inner: &Expr) -> SR<ExprInfo> {
        let val = self.check_expr(inner)?;
        match op {
            UnaryOp::Neg | UnaryOp::BitNot => {
                if !self.is_arithmetic(&val.ty)? {
                    return Err(SemanticError::new(
                        &expr.span,
                        "unary operator requires arithmetic type",
                    ));
                }
                Ok(ExprInfo {
                    ty: val.ty,
                    lvalue: false,
                })
            }
            UnaryOp::Not => {
                if !self.is_scalar(&val.ty)? {
                    return Err(SemanticError::new(
                        &expr.span,
                        "logical not requires scalar type",
                    ));
                }
                Ok(ExprInfo {
                    ty: TypeSpec::Int,
                    lvalue: false,
                })
            }
            UnaryOp::PreInc | UnaryOp::PreDec | UnaryOp::PostInc | UnaryOp::PostDec => {
                if !val.lvalue {
                    return Err(SemanticError::new(
                        &inner.span,
                        "increment/decrement requires an lvalue",
                    ));
                }
                if !self.is_scalar(&val.ty)? {
                    return Err(SemanticError::new(
                        &expr.span,
                        "increment/decrement requires scalar type",
                    ));
                }
                Ok(ExprInfo {
                    ty: val.ty,
                    lvalue: false,
                })
            }
        }
    }

    fn check_binary(&mut self, expr: &Expr, op: &BinaryOp, lhs: &Expr, rhs: &Expr) -> SR<ExprInfo> {
        let l = self.check_expr(lhs)?;
        let r = self.check_expr(rhs)?;
        match op {
            BinaryOp::Add => {
                if self.is_arithmetic(&l.ty)? && self.is_arithmetic(&r.ty)? {
                    Ok(self.usual_arith(l.ty, r.ty))
                } else if self.is_pointer(&l.ty)? && self.is_integer(&r.ty)? {
                    Ok(ExprInfo {
                        ty: l.ty,
                        lvalue: false,
                    })
                } else if self.is_integer(&l.ty)? && self.is_pointer(&r.ty)? {
                    Ok(ExprInfo {
                        ty: r.ty,
                        lvalue: false,
                    })
                } else {
                    Err(SemanticError::new(
                        &expr.span,
                        "operator + requires arithmetic types or pointer plus integer",
                    ))
                }
            }
            BinaryOp::Sub => {
                if self.is_arithmetic(&l.ty)? && self.is_arithmetic(&r.ty)? {
                    Ok(self.usual_arith(l.ty, r.ty))
                } else if self.is_pointer(&l.ty)? && self.is_integer(&r.ty)? {
                    Ok(ExprInfo {
                        ty: l.ty,
                        lvalue: false,
                    })
                } else if self.is_pointer(&l.ty)? && self.is_pointer(&r.ty)? {
                    Ok(ExprInfo {
                        ty: TypeSpec::LongLong,
                        lvalue: false,
                    })
                } else {
                    Err(SemanticError::new(
                        &expr.span,
                        "operator - requires arithmetic types, pointer minus integer, or pointer difference",
                    ))
                }
            }
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                if self.is_arithmetic(&l.ty)? && self.is_arithmetic(&r.ty)? {
                    Ok(self.usual_arith(l.ty, r.ty))
                } else {
                    Err(SemanticError::new(
                        &expr.span,
                        "arithmetic operator requires arithmetic operands",
                    ))
                }
            }
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                if self.is_integer(&l.ty)? && self.is_integer(&r.ty)? {
                    Ok(ExprInfo {
                        ty: TypeSpec::LongLong,
                        lvalue: false,
                    })
                } else {
                    Err(SemanticError::new(
                        &expr.span,
                        "bitwise operator requires integer operands",
                    ))
                }
            }
            BinaryOp::And | BinaryOp::Or => {
                if self.is_scalar(&l.ty)? && self.is_scalar(&r.ty)? {
                    Ok(ExprInfo {
                        ty: TypeSpec::Int,
                        lvalue: false,
                    })
                } else {
                    Err(SemanticError::new(
                        &expr.span,
                        "logical operator requires scalar operands",
                    ))
                }
            }
            BinaryOp::Eq
            | BinaryOp::Ne
            | BinaryOp::Lt
            | BinaryOp::Gt
            | BinaryOp::Le
            | BinaryOp::Ge => {
                if (self.is_scalar(&l.ty)? && self.is_scalar(&r.ty)?)
                    || self.can_assign(&l.ty, &r.ty)?
                    || self.can_assign(&r.ty, &l.ty)?
                {
                    Ok(ExprInfo {
                        ty: TypeSpec::Int,
                        lvalue: false,
                    })
                } else {
                    Err(SemanticError::new(
                        &expr.span,
                        "comparison operands are incompatible",
                    ))
                }
            }
            BinaryOp::Comma => Ok(ExprInfo {
                ty: r.ty,
                lvalue: false,
            }),
        }
    }

    fn check_assign(&mut self, expr: &Expr, op: &AssignOp, lhs: &Expr, rhs: &Expr) -> SR<ExprInfo> {
        let l = self.check_expr(lhs)?;
        if !l.lvalue {
            return Err(SemanticError::new(
                &lhs.span,
                "assignment target must be an lvalue",
            ));
        }
        let r = self.check_expr(rhs)?;
        if *op == AssignOp::Assign {
            self.require_assignable(&l.ty, &r.ty, &expr.span)?;
        } else if !self.is_scalar(&l.ty)? || !self.is_scalar(&r.ty)? {
            return Err(SemanticError::new(
                &expr.span,
                "compound assignment requires scalar operands",
            ));
        }
        Ok(ExprInfo {
            ty: l.ty,
            lvalue: false,
        })
    }

    fn check_call(&mut self, expr: &Expr, func: &Expr, args: &[Expr]) -> SR<ExprInfo> {
        if let ExprKind::Ident(name) = &func.kind {
            if let Some(sig) = self.funcs.get(name).cloned() {
                if (!sig.variadic && args.len() != sig.params.len())
                    || (sig.variadic && args.len() < sig.params.len())
                {
                    return Err(SemanticError::new(
                        &expr.span,
                        format!(
                            "function '{}' expects {} argument{}{} but got {}",
                            name,
                            sig.params.len(),
                            if sig.params.len() == 1 { "" } else { "s" },
                            if sig.variadic { " or more" } else { "" },
                            args.len()
                        ),
                    ));
                }
                for (arg, param_ty) in args.iter().zip(sig.params.iter()) {
                    let arg_i = self.check_expr(arg)?;
                    self.require_assignable(param_ty, &arg_i.ty, &arg.span)?;
                }
                for arg in args.iter().skip(sig.params.len()) {
                    self.check_expr(arg)?;
                }
                return Ok(ExprInfo {
                    ty: sig.ret,
                    lvalue: false,
                });
            }
        }

        let callee = self.check_expr(func)?;
        for arg in args {
            self.check_expr(arg)?;
        }
        if self.is_pointer(&callee.ty)? || matches!(func.kind, ExprKind::Ident(_)) {
            Ok(ExprInfo {
                ty: TypeSpec::Int,
                lvalue: false,
            })
        } else {
            Err(SemanticError::new(
                &func.span,
                "called expression is not a function",
            ))
        }
    }

    fn check_member(&mut self, expr: &Expr, base: &Expr, field: &str, arrow: bool) -> SR<ExprInfo> {
        let base_i = self.check_expr(base)?;
        let mut base_t = self.resolve_type(&base_i.ty, &base.span)?;
        if arrow {
            base_t = match base_t {
                TypeSpec::Pointer(inner) => self.resolve_type(&inner, &base.span)?,
                _ => {
                    return Err(SemanticError::new(
                        &base.span,
                        "operator -> requires pointer to struct or union",
                    ));
                }
            };
        }
        let name = match base_t {
            TypeSpec::Struct(name) | TypeSpec::Union(name) => name,
            _ => {
                return Err(SemanticError::new(
                    &expr.span,
                    "member access requires struct or union",
                ));
            }
        };
        let info = self.structs.get(&name).ok_or_else(|| {
            SemanticError::new(&expr.span, format!("undefined struct or union '{}'", name))
        })?;
        let ty = info.fields.get(field).ok_or_else(|| {
            SemanticError::new(
                &expr.span,
                format!("no field '{}' in struct or union '{}'", field, name),
            )
        })?;
        Ok(ExprInfo {
            ty: ty.clone(),
            lvalue: true,
        })
    }

    fn require_scalar_expr(&mut self, expr: &Expr) -> SR<()> {
        let ty = self.check_expr(expr)?.ty;
        if self.is_scalar(&ty)? {
            Ok(())
        } else {
            Err(SemanticError::new(&expr.span, "expression must be scalar"))
        }
    }

    fn require_assignable(&self, dst: &TypeSpec, src: &TypeSpec, span: &Span) -> SR<()> {
        if self.can_assign(dst, src)? {
            Ok(())
        } else {
            Err(SemanticError::new(
                span,
                format!(
                    "cannot assign {} to {}",
                    self.type_name(src),
                    self.type_name(dst)
                ),
            ))
        }
    }

    fn can_assign(&self, dst: &TypeSpec, src: &TypeSpec) -> SR<bool> {
        let dst = self.resolve_type(dst, &Span { line: 0, col: 0 })?;
        let src = self.resolve_type(src, &Span { line: 0, col: 0 })?;
        if self.same_type(&dst, &src)? {
            return Ok(true);
        }
        if self.is_arithmetic(&dst)? && self.is_arithmetic(&src)? {
            return Ok(true);
        }
        if self.is_pointer(&dst)? && self.is_pointer(&src)? {
            return Ok(true);
        }
        if self.is_pointer(&dst)? && self.is_integer(&src)? {
            return Ok(true);
        }
        Ok(false)
    }

    fn same_type(&self, a: &TypeSpec, b: &TypeSpec) -> SR<bool> {
        let a = self.resolve_type(a, &Span { line: 0, col: 0 })?;
        let b = self.resolve_type(b, &Span { line: 0, col: 0 })?;
        Ok(match (a, b) {
            (TypeSpec::Void, TypeSpec::Void)
            | (TypeSpec::Char, TypeSpec::Char)
            | (TypeSpec::Short, TypeSpec::Short)
            | (TypeSpec::Int, TypeSpec::Int)
            | (TypeSpec::Long, TypeSpec::Long)
            | (TypeSpec::LongLong, TypeSpec::LongLong)
            | (TypeSpec::Float, TypeSpec::Float)
            | (TypeSpec::Double, TypeSpec::Double) => true,
            (TypeSpec::Unsigned(a), TypeSpec::Unsigned(b))
            | (TypeSpec::Signed(a), TypeSpec::Signed(b)) => self.same_type(&a, &b)?,
            (TypeSpec::Pointer(a), TypeSpec::Pointer(b)) => self.same_type(&a, &b)?,
            (TypeSpec::Array(a, _), TypeSpec::Array(b, _)) => self.same_type(&a, &b)?,
            (TypeSpec::Struct(a), TypeSpec::Struct(b))
            | (TypeSpec::Union(a), TypeSpec::Union(b))
            | (TypeSpec::Enum(a), TypeSpec::Enum(b)) => a == b,
            _ => false,
        })
    }

    fn usual_arith(&self, lhs: TypeSpec, rhs: TypeSpec) -> ExprInfo {
        let ty = if matches!(lhs, TypeSpec::Double) || matches!(rhs, TypeSpec::Double) {
            TypeSpec::Double
        } else if matches!(lhs, TypeSpec::Float) || matches!(rhs, TypeSpec::Float) {
            TypeSpec::Float
        } else {
            TypeSpec::LongLong
        };
        ExprInfo { ty, lvalue: false }
    }

    fn resolve_type(&self, ty: &TypeSpec, span: &Span) -> SR<TypeSpec> {
        let mut cur = ty.clone();
        let mut seen = HashSet::new();
        loop {
            match cur {
                TypeSpec::Named(name) => {
                    if !seen.insert(name.clone()) {
                        return Err(SemanticError::new(
                            span,
                            format!("typedef cycle at '{}'", name),
                        ));
                    }
                    cur = self.typedefs.get(&name).cloned().ok_or_else(|| {
                        SemanticError::new(span, format!("unknown type '{}'", name))
                    })?;
                }
                TypeSpec::Pointer(inner) => {
                    let inner = self.resolve_type(&inner, span)?;
                    return Ok(TypeSpec::Pointer(Box::new(inner)));
                }
                TypeSpec::Array(inner, len) => {
                    let inner = self.resolve_type(&inner, span)?;
                    return Ok(TypeSpec::Array(Box::new(inner), len));
                }
                TypeSpec::Unsigned(inner) => {
                    let inner = self.resolve_type(&inner, span)?;
                    return Ok(TypeSpec::Unsigned(Box::new(inner)));
                }
                TypeSpec::Signed(inner) => {
                    let inner = self.resolve_type(&inner, span)?;
                    return Ok(TypeSpec::Signed(Box::new(inner)));
                }
                TypeSpec::Struct(ref name) | TypeSpec::Union(ref name) => {
                    if self.structs.contains_key(name) {
                        return Ok(cur);
                    }
                    return Ok(cur);
                }
                _ => return Ok(cur),
            }
        }
    }

    fn is_void(&self, ty: &TypeSpec) -> SR<bool> {
        Ok(matches!(
            self.resolve_type(ty, &Span { line: 0, col: 0 })?,
            TypeSpec::Void
        ))
    }

    fn is_integer(&self, ty: &TypeSpec) -> SR<bool> {
        Ok(matches!(
            self.resolve_type(ty, &Span { line: 0, col: 0 })?,
            TypeSpec::Char
                | TypeSpec::Short
                | TypeSpec::Int
                | TypeSpec::Long
                | TypeSpec::LongLong
                | TypeSpec::Enum(_)
                | TypeSpec::Unsigned(_)
                | TypeSpec::Signed(_)
        ))
    }

    fn is_arithmetic(&self, ty: &TypeSpec) -> SR<bool> {
        Ok(self.is_integer(ty)?
            || matches!(
                self.resolve_type(ty, &Span { line: 0, col: 0 })?,
                TypeSpec::Float | TypeSpec::Double
            ))
    }

    fn is_pointer(&self, ty: &TypeSpec) -> SR<bool> {
        Ok(matches!(
            self.resolve_type(ty, &Span { line: 0, col: 0 })?,
            TypeSpec::Pointer(_) | TypeSpec::Array(_, _)
        ))
    }

    fn is_scalar(&self, ty: &TypeSpec) -> SR<bool> {
        Ok(self.is_arithmetic(ty)? || self.is_pointer(ty)?)
    }

    fn type_name(&self, ty: &TypeSpec) -> String {
        match ty {
            TypeSpec::Void => "void".into(),
            TypeSpec::Char => "char".into(),
            TypeSpec::Short => "short".into(),
            TypeSpec::Int => "int".into(),
            TypeSpec::Long => "long".into(),
            TypeSpec::LongLong => "long long".into(),
            TypeSpec::Float => "float".into(),
            TypeSpec::Double => "double".into(),
            TypeSpec::Unsigned(inner) => format!("unsigned {}", self.type_name(inner)),
            TypeSpec::Signed(inner) => format!("signed {}", self.type_name(inner)),
            TypeSpec::Pointer(inner) => format!("{}*", self.type_name(inner)),
            TypeSpec::Array(inner, _) => format!("{}[]", self.type_name(inner)),
            TypeSpec::Struct(name) => format!("struct {}", name),
            TypeSpec::Union(name) => format!("union {}", name),
            TypeSpec::Enum(name) => format!("enum {}", name),
            TypeSpec::Named(name) => name.clone(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare_local(&mut self, name: &str, ty: TypeSpec, span: &Span) -> SR<()> {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(name) {
            return Err(SemanticError::new(
                span,
                format!("redefinition of local '{}'", name),
            ));
        }
        scope.insert(name.to_string(), ty);
        Ok(())
    }

    fn lookup_local(&self, name: &str) -> Option<TypeSpec> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
    }
}
