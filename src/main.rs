use anyhow::{bail, Context, Result};
use std::env;
use std::fs;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        bail!("usage: resi <file.resi>");
    }

    let src = fs::read_to_string(&args[1])
        .with_context(|| format!("failed to read {}", args[1]))?;

    let tokens = lexer::lex(&src)?;
    let ast = parser::parse_program(&tokens)?;
    let typed = infer::infer_program(ast)?;
    let rust = codegen::emit_program(&typed);

    println!("{rust}");
    Ok(())
}

mod lexer {
    use anyhow::{bail, Result};

    #[derive(Debug, Clone, PartialEq)]
    pub enum TokenKind {
        Ident(String),
        Number(i64),
        StringLiteral(String),
        RawStringLiteral(String), // full raw lexeme, e.g. r#"hello"#

        KwFn,
        KwLet,
        KwReturn,
        KwMagic,

        Arrow,
        LParen,
        RParen,
        LBrace,
        RBrace,
        Colon,
        Comma,
        Semicolon,
        Plus,
        Minus,
        Star,
        Slash,
        Eq,
        Lt,
        Gt,
    }

    #[derive(Debug, Clone)]
    pub struct Token {
        pub kind: TokenKind,
        pub pos: usize,
    }

    pub fn lex(input: &str) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        let mut i = 0;
        let bytes = input.as_bytes();
        let len = bytes.len();

        while i < len {
            let c = bytes[i] as char;
            match c {
                ' ' | '\t' | '\n' | '\r' => i += 1,

                '(' => { tokens.push(tok(TokenKind::LParen, i)); i += 1; }
                ')' => { tokens.push(tok(TokenKind::RParen, i)); i += 1; }
                '{' => { tokens.push(tok(TokenKind::LBrace, i)); i += 1; }
                '}' => { tokens.push(tok(TokenKind::RBrace, i)); i += 1; }
                ':' => { tokens.push(tok(TokenKind::Colon, i)); i += 1; }
                ',' => { tokens.push(tok(TokenKind::Comma, i)); i += 1; }
                ';' => { tokens.push(tok(TokenKind::Semicolon, i)); i += 1; }
                '+' => { tokens.push(tok(TokenKind::Plus, i)); i += 1; }
                '*' => { tokens.push(tok(TokenKind::Star, i)); i += 1; }
                '/' => { tokens.push(tok(TokenKind::Slash, i)); i += 1; }
                '=' => { tokens.push(tok(TokenKind::Eq, i)); i += 1; }
                '<' => { tokens.push(tok(TokenKind::Lt, i)); i += 1; }
                '>' => { tokens.push(tok(TokenKind::Gt, i)); i += 1; }

                '-' => {
                    if i + 1 < len && bytes[i + 1] as char == '>' {
                        tokens.push(tok(TokenKind::Arrow, i));
                        i += 2;
                    } else {
                        tokens.push(tok(TokenKind::Minus, i));
                        i += 1;
                    }
                }

                '"' => {
                    let start_pos = i;
                    i += 1;
                    let mut s = String::new();
                    while i < len {
                        let ch = bytes[i] as char;
                        if ch == '"' {
                            i += 1;
                            break;
                        }
                        if ch == '\\' {
                            if i + 1 >= len {
                                bail!("unterminated escape in string literal at {}", start_pos);
                            }
                            let esc = bytes[i + 1] as char;
                            match esc {
                                'n' => s.push('\n'),
                                'r' => s.push('\r'),
                                't' => s.push('\t'),
                                '\\' => s.push('\\'),
                                '"' => s.push('"'),
                                '0' => s.push('\0'),
                                _ => {
                                    // keep unknown escapes as-is
                                    s.push('\\');
                                    s.push(esc);
                                }
                            }
                            i += 2;
                        } else {
                            if ch == '\n' {
                                bail!("newline in string literal at {}", start_pos);
                            }
                            s.push(ch);
                            i += 1;
                        }
                    }
                    tokens.push(tok(TokenKind::StringLiteral(s), start_pos));
                }

                '0'..='9' => {
                    let start = i;
                    while i < len && (bytes[i] as char).is_ascii_digit() {
                        i += 1;
                    }
                    let s = &input[start..i];
                    let n = s.parse::<i64>().unwrap();
                    tokens.push(tok(TokenKind::Number(n), start));
                }

                'a'..='z' | 'A'..='Z' | '_' => {
                    // special-case raw strings starting with r / r#
                    if c == 'r' && i + 1 < len {
                        let next = bytes[i + 1] as char;
                        if next == '"' || next == '#' {
                            let start = i;
                            i += 1;
                            let mut hashes = 0;
                            while i < len && bytes[i] as char == '#' {
                                hashes += 1;
                                i += 1;
                            }
                            if i >= len || bytes[i] as char != '"' {
                                bail!("invalid raw string literal at {}", start);
                            }
                            i += 1; // skip opening quote

                            loop {
                                if i >= len {
                                    bail!("unterminated raw string literal at {}", start);
                                }
                                let ch = bytes[i] as char;
                                if ch == '"' {
                                    let mut j = 0;
                                    let mut ok = true;
                                    while j < hashes {
                                        if i + 1 + j >= len || bytes[i + 1 + j] as char != '#' {
                                            ok = false;
                                            break;
                                        }
                                        j += 1;
                                    }
                                    if ok {
                                        i += 1 + hashes;
                                        break;
                                    }
                                }
                                i += 1;
                            }

                            let lexeme = &input[start..i];
                            tokens.push(tok(TokenKind::RawStringLiteral(lexeme.to_string()), start));
                            continue;
                        }
                    }

                    let start = i;
                    i += 1;
                    while i < len {
                        let ch = bytes[i] as char;
                        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '!' {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    let s = &input[start..i];
                    let kind = match s {
                        "fn" => TokenKind::KwFn,
                        "let" => TokenKind::KwLet,
                        "return" => TokenKind::KwReturn,
                        "magic" => TokenKind::KwMagic,
                        _ => TokenKind::Ident(s.to_string()),
                    };
                    tokens.push(tok(kind, start));
                }

                _ => bail!("unexpected character {c:?} at {i}"),
            }
        }

        Ok(tokens)
    }

    fn tok(kind: TokenKind, pos: usize) -> Token {
        Token { kind, pos }
    }
}

mod ast {
    #[derive(Debug, Clone)]
    pub struct Program {
        pub functions: Vec<Function>,
    }

    #[derive(Debug, Clone)]
    pub struct Function {
        pub name: String,
        pub type_params: Vec<String>,
        pub params: Vec<Param>,
        pub body: Vec<Stmt>,
        pub ret_magic: bool,
    }

    #[derive(Debug, Clone)]
    pub struct Param {
        pub name: String,
        pub is_magic: bool,
    }

    #[derive(Debug, Clone)]
    pub enum Stmt {
        Let {
            name: String,
            is_magic: bool,
            expr: Expr,
        },
        Expr(Expr),
        Return(Expr),
    }

    #[derive(Debug, Clone)]
    pub enum Expr {
        Number(i64),
        StringLiteral(String),
        RawStringLiteral(String),
        Var(String),
        BinOp {
            op: BinOp,
            left: Box<Expr>,
            right: Box<Expr>,
        },
        Call {
            name: String,
            args: Vec<Expr>,
        },
        MacroCall {
            name: String,
            args: Vec<Expr>,
        },
    }

    #[derive(Debug, Clone, Copy)]
    pub enum BinOp {
        Add,
        Sub,
        Mul,
        Div,
    }
}

mod parser {
    use super::ast::*;
    use super::lexer::{Token, TokenKind};
    use anyhow::{anyhow, bail, Result};

    pub fn parse_program(tokens: &[Token]) -> Result<Program> {
        let mut p = Parser { tokens, pos: 0 };
        p.parse_program()
    }

    #[derive(Clone)]
    struct Parser<'a> {
        tokens: &'a [Token],
        pos: usize,
    }

    impl<'a> Parser<'a> {
        fn peek(&self) -> Option<&Token> {
            self.tokens.get(self.pos)
        }

        fn peek_owned(&self) -> Option<Token> {
            self.tokens.get(self.pos).cloned()
        }

        fn bump(&mut self) {
            self.pos += 1;
        }

        fn check(&self, kind: TokenKind) -> bool {
            self.peek().map(|t| t.kind.clone()) == Some(kind)
        }

        fn expect(&mut self, kind: TokenKind) -> Result<()> {
            if self.check(kind.clone()) {
                self.bump();
                Ok(())
            } else {
                bail!("expected {:?}, got {:?}", kind, self.peek().map(|t| &t.kind))
            }
        }

        fn expect_ident(&mut self) -> Result<String> {
            if let Some(tok) = self.peek_owned() {
                match tok.kind {
                    TokenKind::Ident(s) => {
                        self.bump();
                        Ok(s)
                    }
                    _ => bail!("expected identifier, got {:?}", tok.kind),
                }
            } else {
                bail!("expected identifier, got EOF")
            }
        }

        fn expect_magic(&mut self) -> Result<bool> {
            if self.check(TokenKind::KwMagic) {
                self.bump();
                Ok(true)
            } else {
                bail!("expected 'magic'")
            }
        }

        fn eof(&self) -> bool {
            self.pos >= self.tokens.len()
        }

        fn parse_program(&mut self) -> Result<Program> {
            let mut functions = Vec::new();
            while !self.eof() {
                functions.push(self.parse_function()?);
            }
            Ok(Program { functions })
        }

        fn parse_function(&mut self) -> Result<Function> {
            self.expect(TokenKind::KwFn)?;
            let name = self.expect_ident()?;

            let mut type_params = Vec::new();
            if self.check(TokenKind::Lt) {
                self.bump();
                loop {
                    let tp = self.expect_ident()?;
                    type_params.push(tp);
                    if self.check(TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
                self.expect(TokenKind::Gt)?;
            }

            self.expect(TokenKind::LParen)?;
            let mut params = Vec::new();
            if !self.check(TokenKind::RParen) {
                loop {
                    let pname = self.expect_ident()?;
                    self.expect(TokenKind::Colon)?;
                    let is_magic = self.expect_magic()?;
                    params.push(Param { name: pname, is_magic });
                    if self.check(TokenKind::Comma) {
                        self.bump();
                    } else {
                        break;
                    }
                }
            }
            self.expect(TokenKind::RParen)?;
            self.expect(TokenKind::Arrow)?;
            let ret_magic = self.expect_magic()?;

            self.expect(TokenKind::LBrace)?;
            let mut body = Vec::new();
            while !self.check(TokenKind::RBrace) {
                body.push(self.parse_stmt()?);
            }
            self.expect(TokenKind::RBrace)?;

            Ok(Function {
                name,
                type_params,
                params,
                body,
                ret_magic,
            })
        }

        fn parse_stmt(&mut self) -> Result<Stmt> {
            if self.check(TokenKind::KwLet) {
                self.bump();
                let name = self.expect_ident()?;
                let mut is_magic = false;
                if self.check(TokenKind::Colon) {
                    self.bump();
                    is_magic = self.expect_magic()?;
                }
                self.expect(TokenKind::Eq)?;
                let expr = self.parse_expr()?;
                self.expect(TokenKind::Semicolon)?;
                Ok(Stmt::Let { name, is_magic, expr })
            } else if self.check(TokenKind::KwReturn) {
                self.bump();
                let expr = self.parse_expr()?;
                self.expect(TokenKind::Semicolon)?;
                Ok(Stmt::Return(expr))
            } else {
                let expr = self.parse_expr()?;
                self.expect(TokenKind::Semicolon)?;
                Ok(Stmt::Expr(expr))
            }
        }

        fn parse_expr(&mut self) -> Result<Expr> {
            self.parse_add()
        }

        fn parse_add(&mut self) -> Result<Expr> {
            let mut expr = self.parse_mul()?;
            loop {
                if self.check(TokenKind::Plus) {
                    self.bump();
                    let rhs = self.parse_mul()?;
                    expr = Expr::BinOp {
                        op: BinOp::Add,
                        left: Box::new(expr),
                        right: Box::new(rhs),
                    };
                } else if self.check(TokenKind::Minus) {
                    self.bump();
                    let rhs = self.parse_mul()?;
                    expr = Expr::BinOp {
                        op: BinOp::Sub,
                        left: Box::new(expr),
                        right: Box::new(rhs),
                    };
                } else {
                    break;
                }
            }
            Ok(expr)
        }

        fn parse_mul(&mut self) -> Result<Expr> {
            let mut expr = self.parse_primary()?;
            loop {
                if self.check(TokenKind::Star) {
                    self.bump();
                    let rhs = self.parse_primary()?;
                    expr = Expr::BinOp {
                        op: BinOp::Mul,
                        left: Box::new(expr),
                        right: Box::new(rhs),
                    };
                } else if self.check(TokenKind::Slash) {
                    self.bump();
                    let rhs = self.parse_primary()?;
                    expr = Expr::BinOp {
                        op: BinOp::Div,
                        left: Box::new(expr),
                        right: Box::new(rhs),
                    };
                } else {
                    break;
                }
            }
            Ok(expr)
        }

        fn parse_primary(&mut self) -> Result<Expr> {
            let tok = self.peek_owned().ok_or_else(|| anyhow!("unexpected EOF"))?;

            match tok.kind {
                TokenKind::Number(n) => {
                    self.bump();
                    Ok(Expr::Number(n))
                }

                TokenKind::StringLiteral(s) => {
                    self.bump();
                    Ok(Expr::StringLiteral(s))
                }

                TokenKind::RawStringLiteral(s) => {
                    self.bump();
                    Ok(Expr::RawStringLiteral(s))
                }

                TokenKind::Ident(name) => {
                    self.bump();
                    if name.ends_with('!') {
                        self.expect(TokenKind::LParen)?;
                        let mut args = Vec::new();
                        if !self.check(TokenKind::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if self.check(TokenKind::Comma) {
                                    self.bump();
                                } else {
                                    break;
                                }
                            }
                        }
                        self.expect(TokenKind::RParen)?;
                        Ok(Expr::MacroCall { name, args })
                    } else if self.check(TokenKind::LParen) {
                        self.bump();
                        let mut args = Vec::new();
                        if !self.check(TokenKind::RParen) {
                            loop {
                                args.push(self.parse_expr()?);
                                if self.check(TokenKind::Comma) {
                                    self.bump();
                                } else {
                                    break;
                                }
                            }
                        }
                        self.expect(TokenKind::RParen)?;
                        Ok(Expr::Call { name, args })
                    } else {
                        Ok(Expr::Var(name))
                    }
                }

                TokenKind::LParen => {
                    self.bump();
                    let e = self.parse_expr()?;
                    self.expect(TokenKind::RParen)?;
                    Ok(e)
                }

                _ => bail!("unexpected token {:?} at {}", tok.kind, tok.pos),
            }
        }
    }
}

mod infer {
    use super::ast::*;
    use anyhow::{anyhow, bail, Result};
    use std::collections::HashMap;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub enum Type {
        Int,
        Bool,
        String,
        Unit,
        Var(u32),
        Generic(String),
    }

    #[derive(Debug, Clone)]
    pub struct TypedProgram {
        pub functions: Vec<TypedFunction>,
    }

    #[derive(Debug, Clone)]
    pub struct TypedFunction {
        pub name: String,
        pub type_params: Vec<String>,
        pub params: Vec<(String, Type)>,
        pub body: Vec<TypedStmt>,
        pub ret_type: Type,
    }

    #[derive(Debug, Clone)]
    pub enum TypedStmt {
        Let {
            name: String,
            ty: Type,
            expr: TypedExpr,
        },
        Expr(TypedExpr),
        Return(TypedExpr),
    }

    #[derive(Debug, Clone)]
    pub enum TypedExpr {
        Number(i64, Type),
        StringLiteral(String, Type),
        RawStringLiteral(String, Type),
        Var(String, Type),
        BinOp {
            op: BinOp,
            left: Box<TypedExpr>,
            right: Box<TypedExpr>,
            ty: Type,
        },
        Call {
            name: String,
            args: Vec<TypedExpr>,
            ty: Type,
        },
        MacroCall {
            name: String,
            args: Vec<TypedExpr>,
            ty: Type,
        },
    }

    #[derive(Debug, Clone)]
    struct Env {
        vars: HashMap<String, Type>,
    }

    impl Env {
        fn new() -> Self {
            Env { vars: HashMap::new() }
        }
    }

    #[derive(Debug, Clone)]
    struct FuncSig {
        params: Vec<Type>,
        ret: Type,
        type_params: Vec<String>,
    }

    #[derive(Debug, Clone)]
    struct Ctx {
        next_var: u32,
        subs: HashMap<u32, Type>,
        funcs: HashMap<String, FuncSig>,
    }

    impl Ctx {
        fn new() -> Self {
            Ctx {
                next_var: 0,
                subs: HashMap::new(),
                funcs: HashMap::new(),
            }
        }

        fn new_var(&mut self) -> Type {
            let v = self.next_var;
            self.next_var += 1;
            Type::Var(v)
        }

        fn prune(&self, t: Type) -> Type {
            match t {
                Type::Var(v) => {
                    if let Some(t2) = self.subs.get(&v) {
                        self.prune(t2.clone())
                    } else {
                        Type::Var(v)
                    }
                }
                _ => t,
            }
        }

        fn occurs(&self, v: u32, t: &Type) -> bool {
            match self.prune(t.clone()) {
                Type::Var(v2) => v == v2,
                _ => false,
            }
        }

        fn unify(&mut self, a: Type, b: Type) -> Result<()> {
            let a = self.prune(a);
            let b = self.prune(b);
            match (a, b) {
                (Type::Int, Type::Int) => Ok(()),
                (Type::Bool, Type::Bool) => Ok(()),
                (Type::String, Type::String) => Ok(()),
                (Type::Unit, Type::Unit) => Ok(()),

                (Type::Generic(_), _) | (_, Type::Generic(_)) => Ok(()),

                (Type::Var(v), t) | (t, Type::Var(v)) => {
                    if t == Type::Var(v) {
                        return Ok(());
                    }
                    if self.occurs(v, &t) {
                        bail!("occurs check failed");
                    }
                    self.subs.insert(v, t);
                    Ok(())
                }

                (x, y) => bail!("cannot unify {:?} with {:?}", x, y),
            }
        }

        fn solve(&mut self) -> Result<()> {
            Ok(())
        }

        fn apply_type(&self, t: Type) -> Type {
            self.prune(t)
        }

        fn apply_expr(&self, e: TypedExpr) -> TypedExpr {
            match e {
                TypedExpr::Number(n, t) => TypedExpr::Number(n, self.apply_type(t)),
                TypedExpr::StringLiteral(s, t) => TypedExpr::StringLiteral(s, self.apply_type(t)),
                TypedExpr::RawStringLiteral(s, t) => {
                    TypedExpr::RawStringLiteral(s, self.apply_type(t))
                }
                TypedExpr::Var(n, t) => TypedExpr::Var(n, self.apply_type(t)),
                TypedExpr::BinOp { op, left, right, ty } => TypedExpr::BinOp {
                    op,
                    left: Box::new(self.apply_expr(*left)),
                    right: Box::new(self.apply_expr(*right)),
                    ty: self.apply_type(ty),
                },
                TypedExpr::Call { name, args, ty } => TypedExpr::Call {
                    name,
                    args: args.into_iter().map(|a| self.apply_expr(a)).collect(),
                    ty: self.apply_type(ty),
                },
                TypedExpr::MacroCall { name, args, ty } => TypedExpr::MacroCall {
                    name,
                    args: args.into_iter().map(|a| self.apply_expr(a)).collect(),
                    ty: self.apply_type(ty),
                },
            }
        }

        fn apply_stmt(&self, s: TypedStmt) -> TypedStmt {
            match s {
                TypedStmt::Let { name, ty, expr } => TypedStmt::Let {
                    name,
                    ty: self.apply_type(ty),
                    expr: self.apply_expr(expr),
                },
                TypedStmt::Expr(e) => TypedStmt::Expr(self.apply_expr(e)),
                TypedStmt::Return(e) => TypedStmt::Return(self.apply_expr(e)),
            }
        }

        fn apply_function(&self, f: TypedFunction) -> TypedFunction {
            let params = f
                .params
                .into_iter()
                .map(|(n, t)| (n, self.apply_type(t)))
                .collect();
            let body = f.body.into_iter().map(|s| self.apply_stmt(s)).collect();
            let ret_type = self.apply_type(f.ret_type);
            TypedFunction {
                name: f.name,
                type_params: f.type_params,
                params,
                body,
                ret_type,
            }
        }
    }

    pub fn infer_program(p: Program) -> Result<TypedProgram> {
        let mut ctx = Ctx::new();

        for f in &p.functions {
            let mut param_tys = Vec::new();

            if !f.type_params.is_empty() {
                for (i, _param) in f.params.iter().enumerate() {
                    let tp = f.type_params.get(i).cloned().unwrap_or_else(|| "T".to_string());
                    param_tys.push(Type::Generic(tp));
                }
                let ret_ty = if f.ret_magic {
                    Type::Generic(f.type_params.get(0).cloned().unwrap_or_else(|| "T".to_string()))
                } else {
                    ctx.new_var()
                };
                ctx.funcs.insert(
                    f.name.clone(),
                    FuncSig {
                        params: param_tys,
                        ret: ret_ty,
                        type_params: f.type_params.clone(),
                    },
                );
            } else {
                for _ in &f.params {
                    param_tys.push(ctx.new_var());
                }
                let ret_tv = if f.ret_magic {
                    ctx.new_var()
                } else {
                    Type::Unit
                };
                ctx.funcs.insert(
                    f.name.clone(),
                    FuncSig {
                        params: param_tys,
                        ret: ret_tv,
                        type_params: Vec::new(),
                    },
                );
            }
        }

        let mut typed_funcs = Vec::new();
        for f in p.functions {
            typed_funcs.push(ctx.infer_function(f)?);
        }

        ctx.solve()?;

        let typed_funcs = typed_funcs
            .into_iter()
            .map(|f| ctx.apply_function(f))
            .collect();

        Ok(TypedProgram { functions: typed_funcs })
    }

    impl Ctx {
        fn infer_function(&mut self, f: Function) -> Result<TypedFunction> {
            let sig = self
                .funcs
                .get(&f.name)
                .ok_or_else(|| anyhow!("missing function sig for {}", f.name))?
                .clone();

            let mut env = Env::new();
            let mut params = Vec::new();
            for (param, ty) in f.params.iter().zip(sig.params.iter()) {
                env.vars.insert(param.name.clone(), ty.clone());
                params.push((param.name.clone(), ty.clone()));
            }

            let mut typed_body = Vec::new();
            let mut ret_ty = sig.ret.clone();

            for stmt in f.body {
                match stmt {
                    Stmt::Let { name, expr, .. } => {
                        let (texpr, ty) = self.infer_expr(&mut env, expr)?;
                        env.vars.insert(name.clone(), ty.clone());
                        typed_body.push(TypedStmt::Let { name, ty, expr: texpr });
                    }
                    Stmt::Expr(e) => {
                        let (texpr, _) = self.infer_expr(&mut env, e)?;
                        typed_body.push(TypedStmt::Expr(texpr));
                    }
                    Stmt::Return(e) => {
                        let (texpr, ty) = self.infer_expr(&mut env, e)?;
                        self.unify(ret_ty.clone(), ty.clone())?;
                        ret_ty = self.prune(ret_ty.clone());
                        typed_body.push(TypedStmt::Return(texpr));
                    }
                }
            }

            Ok(TypedFunction {
                name: f.name,
                type_params: sig.type_params,
                params,
                body: typed_body,
                ret_type: ret_ty,
            })
        }

        fn infer_expr(&mut self, env: &mut Env, e: Expr) -> Result<(TypedExpr, Type)> {
            match e {
                Expr::Number(n) => Ok((TypedExpr::Number(n, Type::Int), Type::Int)),

                Expr::StringLiteral(s) => {
                    Ok((TypedExpr::StringLiteral(s, Type::String), Type::String))
                }

                Expr::RawStringLiteral(s) => {
                    Ok((TypedExpr::RawStringLiteral(s, Type::String), Type::String))
                }

                Expr::Var(name) => {
                    if let Some(ty) = env.vars.get(&name) {
                        Ok((TypedExpr::Var(name, ty.clone()), ty.clone()))
                    } else {
                        bail!("unknown variable {name}")
                    }
                }

                Expr::BinOp { op, left, right } => {
                    let (l, lt) = self.infer_expr(env, *left)?;
                    let (r, rt) = self.infer_expr(env, *right)?;
                    self.unify(lt.clone(), Type::Int)?;
                    self.unify(rt.clone(), Type::Int)?;
                    let ty = Type::Int;
                    Ok((
                        TypedExpr::BinOp {
                            op,
                            left: Box::new(l),
                            right: Box::new(r),
                            ty: ty.clone(),
                        },
                        ty,
                    ))
                }

                Expr::Call { name, args } => {
                    let sig = self
                        .funcs
                        .get(&name)
                        .ok_or_else(|| anyhow!("unknown function {name}"))?
                        .clone();
                    if sig.params.len() != args.len() {
                        bail!("arity mismatch in call to {name}");
                    }
                    let mut targs = Vec::new();
                    for (arg, pty) in args.into_iter().zip(sig.params.iter()) {
                        let (ta, aty) = self.infer_expr(env, arg)?;
                        match pty {
                            Type::Generic(_) => {}
                            _ => self.unify(pty.clone(), aty)?,
                        }
                        targs.push(ta);
                    }
                    let ret_ty = match sig.ret.clone() {
                        Type::Generic(_) => self.new_var(),
                        other => other,
                    };
                    Ok((
                        TypedExpr::Call {
                            name,
                            args: targs,
                            ty: ret_ty.clone(),
                        },
                        ret_ty,
                    ))
                }

                Expr::MacroCall { name, args } => {
                    let mut targs = Vec::new();
                    for arg in args {
                        let (ta, _) = self.infer_expr(env, arg)?;
                        targs.push(ta);
                    }
                    Ok((
                        TypedExpr::MacroCall {
                            name,
                            args: targs,
                            ty: Type::Unit,
                        },
                        Type::Unit,
                    ))
                }
            }
        }
    }
}

mod codegen {
    use super::ast::BinOp;
    use super::infer::{Type, TypedExpr, TypedFunction, TypedProgram, TypedStmt};
    use std::fmt::Write;

    pub fn emit_program(p: &TypedProgram) -> String {
        let mut out = String::new();
        writeln!(&mut out, "// Generated by Resi").unwrap();
        writeln!(&mut out).unwrap();
        for f in &p.functions {
            emit_function(&mut out, f);
            writeln!(&mut out).unwrap();
        }
        out
    }

    fn emit_function(out: &mut String, f: &TypedFunction) {
        let is_main = f.name == "main";

        if !f.type_params.is_empty() && !is_main {
            write!(out, "fn {}<", f.name).unwrap();
            for (i, tp) in f.type_params.iter().enumerate() {
                if i > 0 {
                    write!(out, ", ").unwrap();
                }
                write!(out, "{tp}").unwrap();
            }
            write!(out, ">(").unwrap();
        } else {
            write!(out, "fn {}(", f.name).unwrap();
        }

        for (i, (name, ty)) in f.params.iter().enumerate() {
            if i > 0 {
                write!(out, ", ").unwrap();
            }
            write!(out, "{}: {}", name, ty_to_rust(ty)).unwrap();
        }

        if is_main {
            writeln!(out, ") {{").unwrap();
        } else {
            writeln!(out, ") -> {} {{", ty_to_rust(&f.ret_type)).unwrap();
        }

        for stmt in &f.body {
            emit_stmt(out, stmt, is_main);
        }

        writeln!(out, "}}").unwrap();
    }

    fn emit_stmt(out: &mut String, s: &TypedStmt, is_main: bool) {
        match s {
            TypedStmt::Let { name, ty, expr } => {
                write!(out, "    let {}: {} = ", name, ty_to_rust(ty)).unwrap();
                emit_expr(out, expr);
                writeln!(out, ";").unwrap();
            }
            TypedStmt::Expr(e) => {
                write!(out, "    ").unwrap();
                emit_expr(out, e);
                writeln!(out, ";").unwrap();
            }
            TypedStmt::Return(e) => {
                if is_main {
                    write!(out, "    std::process::exit(").unwrap();
                    emit_expr(out, e);
                    writeln!(out, " as i32);").unwrap();
                } else {
                    write!(out, "    return ").unwrap();
                    emit_expr(out, e);
                    writeln!(out, ";").unwrap();
                }
            }
        }
    }

    fn emit_expr(out: &mut String, e: &TypedExpr) {
        match e {
            TypedExpr::Number(n, _) => write!(out, "{n}").unwrap(),

            TypedExpr::StringLiteral(s, _) => {
                write!(out, "{:?}", s).unwrap();
            }

            TypedExpr::RawStringLiteral(lexeme, _) => {
                write!(out, "{}", lexeme).unwrap();
            }

            TypedExpr::Var(name, _) => write!(out, "{name}").unwrap(),

            TypedExpr::BinOp { op, left, right, .. } => {
                write!(out, "(").unwrap();
                emit_expr(out, left);
                let op_str = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                };
                write!(out, " {} ", op_str).unwrap();
                emit_expr(out, right);
                write!(out, ")").unwrap();
            }

            TypedExpr::Call { name, args, .. } => {
                write!(out, "{}(", name).unwrap();
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(out, ", ").unwrap();
                    }
                    emit_expr(out, a);
                }
                write!(out, ")").unwrap();
            }

            TypedExpr::MacroCall { name, args, .. } => {
                write!(out, "{}(", name).unwrap();
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        write!(out, ", ").unwrap();
                    }
                    emit_expr(out, a);
                }
                write!(out, ")").unwrap();
            }
        }
    }

    fn ty_to_rust(t: &Type) -> String {
        match t {
            Type::Int => "i64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::String => "String".to_string(),
            Type::Unit => "()".to_string(),
            Type::Var(_) => "i64".to_string(),
            Type::Generic(name) => name.clone(),
        }
    }
}
