use std::{
    cell::RefCell,
    collections::HashMap,
    fmt,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    Lex,
    Parse,
    Limit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
    pub span: Span,
}
impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at {}..{}",
            self.message, self.span.start, self.span.end
        )
    }
}
impl std::error::Error for Diagnostic {}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeErrorKind {
    Type,
    UnknownName,
    Overflow,
    DivisionByZero,
    FuelExhausted,
    NestingLimit,
    Timeout,
    OutputLimit,
    InvalidStream,
}
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeError {
    pub kind: RuntimeErrorKind,
    pub message: String,
}
impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for RuntimeError {}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub fuel: u64,
    pub max_nesting: usize,
    pub max_output_bytes: usize,
    pub timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            fuel: 1_000_000,
            max_nesting: 128,
            max_output_bytes: 16 * 1024 * 1024,
            timeout: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Bool(bool),
    Str(String),
    Var(String),
    Let(String, Box<Expr>),
    Binary(Box<Expr>, Op, Box<Expr>),
    Call(String, Vec<Expr>),
    Lambda(String, Box<Expr>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub expressions: Vec<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Int(i64),
    Ident(String),
    Str(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Arrow,
    LParen,
    RParen,
    Comma,
    Semi,
    Eof,
}
#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    span: Span,
}

fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    if source.len() > 1024 * 1024 {
        return Err(Diagnostic {
            kind: DiagnosticKind::Limit,
            message: "source exceeds 1 MiB limit".into(),
            span: Span {
                start: 0,
                end: source.len(),
            },
        });
    }
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let simple = match c {
            b'+' => Some(TokenKind::Plus),
            b'-' if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Arrow,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'-' => Some(TokenKind::Minus),
            b'*' => Some(TokenKind::Star),
            b'/' => Some(TokenKind::Slash),
            b'%' => Some(TokenKind::Percent),
            b'(' => Some(TokenKind::LParen),
            b')' => Some(TokenKind::RParen),
            b',' => Some(TokenKind::Comma),
            b';' => Some(TokenKind::Semi),
            b'=' if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Arrow,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'=' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::EqEq,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'=' => Some(TokenKind::Eq),
            b'!' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Ne,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'<' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Le,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'<' => Some(TokenKind::Lt),
            b'>' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Ge,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'>' => Some(TokenKind::Gt),
            b'&' if bytes.get(i + 1) == Some(&b'&') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::And,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'|' if bytes.get(i + 1) == Some(&b'|') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Or,
                    span: Span { start, end: i },
                });
                continue;
            }
            _ => None,
        };
        if let Some(kind) = simple {
            i += 1;
            out.push(Token {
                kind,
                span: Span { start, end: i },
            });
            continue;
        }
        if c == b'"' {
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < bytes.len() {
                match bytes[i] {
                    b'"' => {
                        i += 1;
                        closed = true;
                        break;
                    }
                    b'\\' if i + 1 < bytes.len() => {
                        i += 1;
                        let ch = match bytes[i] {
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'"' => '"',
                            b'\\' => '\\',
                            _ => {
                                return Err(Diagnostic {
                                    kind: DiagnosticKind::Lex,
                                    message: "unsupported string escape".into(),
                                    span: Span {
                                        start: i - 1,
                                        end: i + 1,
                                    },
                                })
                            }
                        };
                        s.push(ch);
                        i += 1;
                    }
                    _ => {
                        let rest = std::str::from_utf8(&bytes[i..]).unwrap_or("");
                        let ch = rest.chars().next().ok_or_else(|| Diagnostic {
                            kind: DiagnosticKind::Lex,
                            message: "invalid UTF-8".into(),
                            span: Span {
                                start: i,
                                end: i + 1,
                            },
                        })?;
                        s.push(ch);
                        i += ch.len_utf8();
                    }
                }
            }
            if !closed {
                return Err(Diagnostic {
                    kind: DiagnosticKind::Lex,
                    message: "unterminated string".into(),
                    span: Span {
                        start,
                        end: bytes.len(),
                    },
                });
            }
            out.push(Token {
                kind: TokenKind::Str(s),
                span: Span { start, end: i },
            });
            continue;
        }
        if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let n = source[start..i].parse::<i64>().map_err(|_| Diagnostic {
                kind: DiagnosticKind::Lex,
                message: "integer literal outside signed 64-bit range".into(),
                span: Span { start, end: i },
            })?;
            out.push(Token {
                kind: TokenKind::Int(n),
                span: Span { start, end: i },
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push(Token {
                kind: TokenKind::Ident(source[start..i].to_string()),
                span: Span { start, end: i },
            });
            continue;
        }
        return Err(Diagnostic {
            kind: DiagnosticKind::Lex,
            message: format!("unexpected character {:?}", c as char),
            span: Span {
                start,
                end: start + 1,
            },
        });
    }
    out.push(Token {
        kind: TokenKind::Eof,
        span: Span {
            start: bytes.len(),
            end: bytes.len(),
        },
    });
    Ok(out)
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    max_nesting: usize,
}
impl Parser {
    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        t
    }
    fn error(&self, message: &str) -> Diagnostic {
        Diagnostic {
            kind: DiagnosticKind::Parse,
            message: message.into(),
            span: self.current().span,
        }
    }
    fn eat(&mut self, kind: &TokenKind) -> bool {
        if std::mem::discriminant(&self.current().kind) == std::mem::discriminant(kind) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<(), Diagnostic> {
        if self.eat(kind) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {what}")))
        }
    }
    fn parse_program(&mut self) -> Result<Program, Diagnostic> {
        let mut expressions = Vec::new();
        while !matches!(self.current().kind, TokenKind::Eof) {
            expressions.push(self.expr(0, 0)?);
            if !self.eat(&TokenKind::Semi) && !matches!(self.current().kind, TokenKind::Eof) {
                return Err(self.error("expected ';' between expressions"));
            }
        }
        Ok(Program { expressions })
    }
    fn expr(&mut self, min_bp: u8, depth: usize) -> Result<Expr, Diagnostic> {
        if depth > self.max_nesting {
            return Err(self.error("maximum parser nesting exceeded"));
        }
        let token = self.bump();
        let mut lhs = match token.kind {
            TokenKind::Int(n) => Expr::Int(n),
            TokenKind::Str(s) => Expr::Str(s),
            TokenKind::Ident(ref s) if s == "true" => Expr::Bool(true),
            TokenKind::Ident(ref s) if s == "false" => Expr::Bool(false),
            TokenKind::Ident(ref s) if s == "let" => {
                let name = match self.bump().kind {
                    TokenKind::Ident(n) => n,
                    _ => return Err(self.error("expected identifier after let")),
                };
                self.expect(&TokenKind::Eq, "'='")?;
                Expr::Let(name, Box::new(self.expr(0, depth + 1)?))
            }
            TokenKind::Ident(name) => Expr::Var(name),
            TokenKind::Minus => {
                let rhs = self.expr(11, depth + 1)?;
                Expr::Binary(Box::new(Expr::Int(0)), Op::Sub, Box::new(rhs))
            }
            TokenKind::LParen => {
                let e = self.expr(0, depth + 1)?;
                self.expect(&TokenKind::RParen, "')'")?;
                e
            }
            _ => {
                return Err(Diagnostic {
                    kind: DiagnosticKind::Parse,
                    message: "expected expression".into(),
                    span: token.span,
                })
            }
        };
        loop {
            if matches!(self.current().kind, TokenKind::Arrow) {
                if min_bp > 1 {
                    break;
                }
                let param = match lhs {
                    Expr::Var(name) => name,
                    _ => return Err(self.error("lambda parameter must be an identifier")),
                };
                self.bump();
                let body = self.expr(0, depth + 1)?;
                lhs = Expr::Lambda(param, Box::new(body));
                continue;
            }
            if matches!(self.current().kind, TokenKind::LParen) {
                if min_bp > 13 {
                    break;
                }
                let callee = match lhs {
                    Expr::Var(name) => name,
                    _ => return Err(self.error("only named built-ins can be called")),
                };
                self.bump();
                let mut args = Vec::new();
                if !self.eat(&TokenKind::RParen) {
                    loop {
                        args.push(self.expr(0, depth + 1)?);
                        if self.eat(&TokenKind::RParen) {
                            break;
                        }
                        self.expect(&TokenKind::Comma, "','")?;
                    }
                }
                lhs = Expr::Call(callee, args);
                continue;
            }
            let Some((op, left_bp, right_bp)) = infix(&self.current().kind) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }
            self.bump();
            let rhs = self.expr(right_bp, depth + 1)?;
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        Ok(lhs)
    }
}
fn infix(k: &TokenKind) -> Option<(Op, u8, u8)> {
    Some(match k {
        TokenKind::Or => (Op::Or, 1, 2),
        TokenKind::And => (Op::And, 3, 4),
        TokenKind::EqEq => (Op::Eq, 5, 6),
        TokenKind::Ne => (Op::Ne, 5, 6),
        TokenKind::Lt => (Op::Lt, 7, 8),
        TokenKind::Le => (Op::Le, 7, 8),
        TokenKind::Gt => (Op::Gt, 7, 8),
        TokenKind::Ge => (Op::Ge, 7, 8),
        TokenKind::Plus => (Op::Add, 9, 10),
        TokenKind::Minus => (Op::Sub, 9, 10),
        TokenKind::Star => (Op::Mul, 11, 12),
        TokenKind::Slash => (Op::Div, 11, 12),
        TokenKind::Percent => (Op::Rem, 11, 12),
        _ => return None,
    })
}

pub fn parse(source: &str) -> Result<Program, Diagnostic> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        max_nesting: 128,
    };
    parser.parse_program()
}

type LazyIterator = Box<dyn Iterator<Item = Result<Value, RuntimeError>>>;
type SharedIterator = Rc<RefCell<LazyIterator>>;

#[derive(Clone)]
pub struct Stream {
    inner: SharedIterator,
    pub(crate) state: Rc<RefCell<State>>,
    output_limit: usize,
}
impl Stream {
    fn new<I: Iterator<Item = Result<Value, RuntimeError>> + 'static>(
        iter: I,
        state: Rc<RefCell<State>>,
        output_limit: usize,
    ) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Box::new(iter))),
            state,
            output_limit,
        }
    }
    fn next_item(&mut self) -> Option<Result<Value, RuntimeError>> {
        if let Err(e) = tick(&self.state) {
            return Some(Err(e));
        }
        self.inner.borrow_mut().next()
    }
    pub fn take(self, mut count: u64) -> Self {
        let state = self.state.clone();
        let limit = self.output_limit;
        let mut iter = self;
        Self::new(
            std::iter::from_fn(move || {
                if count == 0 {
                    return None;
                }
                let next = iter.next_item();
                if next.is_some() {
                    count -= 1;
                }
                next
            }),
            state,
            limit,
        )
    }
    pub fn collect_limited(mut self, count: usize) -> Result<Vec<Value>, RuntimeError> {
        let mut out = Vec::new();
        let mut bytes = 0usize;
        while out.len() < count {
            match self.next() {
                None => break,
                Some(Err(e)) => return Err(e),
                Some(Ok(v)) => {
                    bytes = bytes.saturating_add(v.to_string().len());
                    if bytes > self.output_limit {
                        return Err(err(
                            RuntimeErrorKind::OutputLimit,
                            "output byte limit exceeded",
                        ));
                    }
                    out.push(v);
                }
            }
        }
        Ok(out)
    }
}
impl Iterator for Stream {
    type Item = Result<Value, RuntimeError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_item()
    }
}
impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<lazy-stream>")
    }
}

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Str(String),
    Stream(Stream),
    Function(Closure),
}
#[derive(Clone)]
pub struct Closure {
    param: String,
    body: Expr,
    env: Env,
}
type Env = HashMap<String, Value>;
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            _ => false,
        }
    }
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "Int({n})"),
            Self::Bool(b) => write!(f, "Bool({b})"),
            Self::Str(s) => write!(f, "Str({s:?})"),
            Self::Stream(s) => fmt::Debug::fmt(s, f),
            Self::Function(_) => f.write_str("<function>"),
        }
    }
}
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Str(s) => f.write_str(s),
            Self::Stream(_) => f.write_str("<lazy-stream>"),
            Self::Function(_) => f.write_str("<function>"),
        }
    }
}

#[derive(Debug)]
pub struct State {
    fuel: u64,
    deadline: Option<Instant>,
}
fn err(kind: RuntimeErrorKind, message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        kind,
        message: message.into(),
    }
}
fn tick(state: &Rc<RefCell<State>>) -> Result<(), RuntimeError> {
    let mut s = state.borrow_mut();
    if s.deadline.is_some_and(|deadline| Instant::now() > deadline) {
        return Err(err(
            RuntimeErrorKind::Timeout,
            "evaluation time limit exceeded",
        ));
    }
    if s.fuel == 0 {
        return Err(err(
            RuntimeErrorKind::FuelExhausted,
            "evaluation fuel exhausted",
        ));
    }
    s.fuel -= 1;
    Ok(())
}

pub fn eval(program: &Program, limits: Limits) -> Result<Value, RuntimeError> {
    let state = Rc::new(RefCell::new(State {
        fuel: limits.fuel,
        deadline: Instant::now().checked_add(limits.timeout),
    }));
    let mut env = Env::new();
    let mut result = Value::Bool(false);
    for expression in &program.expressions {
        tick(&state)?;
        result = eval_expr(expression, &mut env, &state, limits, 0)?;
    }
    Ok(result)
}
fn eval_expr(
    expr: &Expr,
    env: &mut Env,
    state: &Rc<RefCell<State>>,
    limits: Limits,
    depth: usize,
) -> Result<Value, RuntimeError> {
    tick(state)?;
    if depth > limits.max_nesting {
        return Err(err(
            RuntimeErrorKind::NestingLimit,
            "maximum evaluation nesting exceeded",
        ));
    }
    match expr {
        Expr::Int(n) => Ok(Value::Int(*n)),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Str(s) => Ok(Value::Str(s.clone())),
        Expr::Var(n) => env
            .get(n)
            .cloned()
            .ok_or_else(|| err(RuntimeErrorKind::UnknownName, format!("unknown name: {n}"))),
        Expr::Let(n, e) => {
            let v = eval_expr(e, env, state, limits, depth + 1)?;
            env.insert(n.clone(), v.clone());
            Ok(v)
        }
        Expr::Lambda(param, body) => Ok(Value::Function(Closure {
            param: param.clone(),
            body: (**body).clone(),
            env: env.clone(),
        })),
        Expr::Binary(a, op, b) => {
            let left = eval_expr(a, env, state, limits, depth + 1)?;
            if *op == Op::And && left == Value::Bool(false) {
                return Ok(Value::Bool(false));
            }
            if *op == Op::Or && left == Value::Bool(true) {
                return Ok(Value::Bool(true));
            }
            let right = eval_expr(b, env, state, limits, depth + 1)?;
            apply_binary(left, *op, right)
        }
        Expr::Call(name, args) => call(name, args, env, state, limits, depth + 1),
    }
}
fn int(v: Value) -> Result<i64, RuntimeError> {
    if let Value::Int(n) = v {
        Ok(n)
    } else {
        Err(err(RuntimeErrorKind::Type, "expected integer"))
    }
}
fn apply_binary(a: Value, op: Op, b: Value) -> Result<Value, RuntimeError> {
    use Op::*;
    if op == Eq || op == Ne {
        let equal = a == b;
        return Ok(Value::Bool(if op == Eq { equal } else { !equal }));
    }
    if op == And || op == Or {
        if let (Value::Bool(x), Value::Bool(y)) = (a, b) {
            return Ok(Value::Bool(if op == And { x && y } else { x || y }));
        }
        return Err(err(
            RuntimeErrorKind::Type,
            "logical operators require booleans",
        ));
    }
    let x = int(a)?;
    let y = int(b)?;
    let n = match op {
        Add => x.checked_add(y),
        Sub => x.checked_sub(y),
        Mul => x.checked_mul(y),
        Div => {
            if y == 0 {
                return Err(err(RuntimeErrorKind::DivisionByZero, "division by zero"));
            }
            x.checked_div(y)
        }
        Rem => {
            if y == 0 {
                return Err(err(RuntimeErrorKind::DivisionByZero, "remainder by zero"));
            }
            x.checked_rem(y)
        }
        Lt => return Ok(Value::Bool(x < y)),
        Le => return Ok(Value::Bool(x <= y)),
        Gt => return Ok(Value::Bool(x > y)),
        Ge => return Ok(Value::Bool(x >= y)),
        _ => None,
    };
    n.map(Value::Int)
        .ok_or_else(|| err(RuntimeErrorKind::Overflow, "integer arithmetic overflow"))
}
fn call(
    name: &str,
    args: &[Expr],
    env: &mut Env,
    state: &Rc<RefCell<State>>,
    limits: Limits,
    depth: usize,
) -> Result<Value, RuntimeError> {
    let eval_arg = |i: usize, env: &mut Env| -> Result<Value, RuntimeError> {
        args.get(i)
            .ok_or_else(|| {
                err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects more arguments"),
                )
            })
            .and_then(|e| eval_expr(e, env, state, limits, depth + 1))
    };
    match name {
        "range" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "range expects 2 arguments"));
            }
            let start = int(eval_arg(0, env)?)?;
            let end = int(eval_arg(1, env)?)?;
            let state2 = state.clone();
            Ok(Value::Stream(Stream::new(
                (start..end).map(move |n| {
                    tick(&state2)?;
                    Ok(Value::Int(n))
                }),
                state.clone(),
                limits.max_output_bytes,
            )))
        }
        "take" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "take expects 2 arguments"));
            }
            let stream = match eval_arg(0, env)? {
                Value::Stream(s) => s,
                _ => return Err(err(RuntimeErrorKind::Type, "take expects a stream")),
            };
            let count = int(eval_arg(1, env)?)?;
            if count < 0 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    "take count must be nonnegative",
                ));
            }
            Ok(Value::Stream(stream.take(count as u64)))
        }
        "map" | "filter" => {
            if args.len() != 2 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 2 arguments"),
                ));
            }
            let is_map = name == "map";
            let stream = match eval_arg(0, env)? {
                Value::Stream(s) => s,
                _ => {
                    return Err(err(
                        RuntimeErrorKind::Type,
                        format!("{name} expects a stream"),
                    ))
                }
            };
            let function = match eval_arg(1, env)? {
                Value::Function(f) => f,
                _ => {
                    return Err(err(
                        RuntimeErrorKind::Type,
                        format!("{name} expects a lambda"),
                    ))
                }
            };
            let state2 = state.clone();
            let limits2 = limits;
            let mut src = stream;
            let iter = std::iter::from_fn(move || loop {
                let input = match src.next()? {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                let mut local = function.env.clone();
                local.insert(function.param.clone(), input.clone());
                match eval_expr(&function.body, &mut local, &state2, limits2, 0) {
                    Err(e) => return Some(Err(e)),
                    Ok(v) if is_map => return Some(Ok(v)),
                    Ok(Value::Bool(true)) => return Some(Ok(input)),
                    Ok(Value::Bool(false)) => continue,
                    Ok(_) => {
                        return Some(Err(err(
                            RuntimeErrorKind::Type,
                            "filter predicate must return boolean",
                        )))
                    }
                }
            });
            Ok(Value::Stream(Stream::new(
                iter,
                state.clone(),
                limits.max_output_bytes,
            )))
        }
        "sum" | "count" | "checksum" => {
            if args.len() != 1 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 1 argument"),
                ));
            }
            let stream = match eval_arg(0, env)? {
                Value::Stream(s) => s,
                _ => {
                    return Err(err(
                        RuntimeErrorKind::Type,
                        format!("{name} expects a stream"),
                    ))
                }
            };
            let mut total = 0i64;
            let mut count = 0i64;
            let mut hash = 0xcbf29ce484222325u64;
            for item in stream {
                let value = item?;
                let n = int(value)?;
                total = total
                    .checked_add(n)
                    .ok_or_else(|| err(RuntimeErrorKind::Overflow, "sum overflow"))?;
                count = count
                    .checked_add(1)
                    .ok_or_else(|| err(RuntimeErrorKind::Overflow, "count overflow"))?;
                hash ^= n as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            match name {
                "sum" => Ok(Value::Int(total)),
                "count" => Ok(Value::Int(count)),
                _ => Ok(Value::Int(hash as i64)),
            }
        }
        "print" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, "print expects 1 argument"));
            }
            eval_arg(0, env)
        }
        _ => Err(err(
            RuntimeErrorKind::UnknownName,
            format!("unknown function: {name}"),
        )),
    }
}
