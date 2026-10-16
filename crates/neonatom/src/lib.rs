use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    fmt,
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};
use chrono::NaiveDate;
use rust_decimal::{prelude::ToPrimitive, Decimal};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticKind {
    Lex,
    Parse,
    Type,
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
    InvalidDate,
    InvalidUnit,
    Assertion,
    Io,
    Json,
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

#[derive(Debug, Clone, Default)]
pub struct EvalContext {
    local_root: Option<PathBuf>,
}
impl EvalContext {
    pub fn with_local_root(root: impl Into<PathBuf>) -> Self {
        Self {
            local_root: Some(root.into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Decimal(Decimal),
    Bool(bool),
    Str(String),
    Var(String),
    Let(String, Box<Expr>),
    Binary(Box<Expr>, Op, Box<Expr>),
    Call(String, Vec<Expr>),
    Lambda(String, Box<Expr>),
    List(Vec<Expr>),
    Record(Vec<(String, Expr)>),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaticType {
    Int,
    Decimal,
    Bool,
    Str,
    List,
    Tuple,
    Map,
    Record,
    Option,
    Result,
    Stream,
    Function,
    Date,
    Duration,
    Quantity,
    Any,
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Int(i64),
    Decimal(Decimal),
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
    Colon,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
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
            b':' => Some(TokenKind::Colon),
            b'[' => Some(TokenKind::LBracket),
            b']' => Some(TokenKind::RBracket),
            b'{' => Some(TokenKind::LBrace),
            b'}' => Some(TokenKind::RBrace),
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
            if bytes.get(i) == Some(&b'.')
                && bytes.get(i + 1).is_some_and(u8::is_ascii_digit)
            {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let value = Decimal::from_str_exact(&source[start..i]).map_err(|_| Diagnostic {
                    kind: DiagnosticKind::Lex,
                    message: "invalid fixed-point decimal literal".into(),
                    span: Span { start, end: i },
                })?;
                out.push(Token {
                    kind: TokenKind::Decimal(value),
                    span: Span { start, end: i },
                });
                continue;
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
            TokenKind::Decimal(n) => Expr::Decimal(n),
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
            TokenKind::LBracket => {
                let mut items = Vec::new();
                if !self.eat(&TokenKind::RBracket) {
                    loop {
                        items.push(self.expr(0, depth + 1)?);
                        if self.eat(&TokenKind::RBracket) {
                            break;
                        }
                        self.expect(&TokenKind::Comma, "',' or ']' after list item")?;
                    }
                }
                Expr::List(items)
            }
            TokenKind::LBrace => {
                let mut fields = Vec::new();
                if !self.eat(&TokenKind::RBrace) {
                    loop {
                        let key = match self.bump().kind {
                            TokenKind::Ident(key) | TokenKind::Str(key) => key,
                            _ => return Err(self.error("record key must be text or an identifier")),
                        };
                        self.expect(&TokenKind::Colon, "':' after record key")?;
                        fields.push((key, self.expr(0, depth + 1)?));
                        if self.eat(&TokenKind::RBrace) {
                            break;
                        }
                        self.expect(&TokenKind::Comma, "',' or '}' after record field")?;
                    }
                }
                Expr::Record(fields)
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

pub fn check(program: &Program) -> Result<StaticType, Diagnostic> {
    let mut names = HashMap::new();
    let mut result = StaticType::Any;
    for expression in &program.expressions {
        result = check_expr(expression, &mut names)?;
    }
    Ok(result)
}

fn type_diagnostic(message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        kind: DiagnosticKind::Type,
        message: message.into(),
        span: Span { start: 0, end: 0 },
    }
}

fn compatible(expected: &StaticType, actual: &StaticType) -> bool {
    expected == actual || *expected == StaticType::Any || *actual == StaticType::Any
}

fn check_expr(
    expression: &Expr,
    names: &mut HashMap<String, StaticType>,
) -> Result<StaticType, Diagnostic> {
    match expression {
        Expr::Int(_) => Ok(StaticType::Int),
        Expr::Decimal(_) => Ok(StaticType::Decimal),
        Expr::Bool(_) => Ok(StaticType::Bool),
        Expr::Str(_) => Ok(StaticType::Str),
        Expr::Var(name) => names
            .get(name)
            .cloned()
            .ok_or_else(|| type_diagnostic(format!("unknown name: {name}"))),
        Expr::Let(name, value) => {
            let ty = check_expr(value, names)?;
            names.insert(name.clone(), ty.clone());
            Ok(ty)
        }
        Expr::Lambda(parameter, body) => {
            let previous = names.insert(parameter.clone(), StaticType::Any);
            let result = check_expr(body, names);
            if let Some(previous) = previous {
                names.insert(parameter.clone(), previous);
            } else {
                names.remove(parameter);
            }
            result?;
            Ok(StaticType::Function)
        }
        Expr::List(items) => {
            for item in items {
                check_expr(item, names)?;
            }
            Ok(StaticType::List)
        }
        Expr::Record(fields) => {
            for (_, value) in fields {
                check_expr(value, names)?;
            }
            Ok(StaticType::Record)
        }
        Expr::Binary(left, operator, right) => {
            let left_type = check_expr(left, names)?;
            let right_type = check_expr(right, names)?;
            use Op::*;
            match operator {
                Add | Sub | Mul | Div | Rem => {
                    let numeric = |ty: &StaticType| {
                        matches!(ty, StaticType::Int | StaticType::Decimal | StaticType::Any)
                    };
                    if !numeric(&left_type) || !numeric(&right_type) {
                        return Err(type_diagnostic("arithmetic operators require numeric values"));
                    }
                    if left_type == StaticType::Decimal || right_type == StaticType::Decimal {
                        Ok(StaticType::Decimal)
                    } else {
                        Ok(StaticType::Int)
                    }
                }
                And | Or => {
                    if !compatible(&StaticType::Bool, &left_type)
                        || !compatible(&StaticType::Bool, &right_type)
                    {
                        return Err(type_diagnostic("logical operators require booleans"));
                    }
                    Ok(StaticType::Bool)
                }
                Eq | Ne => Ok(StaticType::Bool),
                Lt | Le | Gt | Ge => {
                    if !compatible(&left_type, &right_type)
                        && !(matches!(left_type, StaticType::Int | StaticType::Decimal)
                            && matches!(right_type, StaticType::Int | StaticType::Decimal))
                    {
                        return Err(type_diagnostic("comparison operands have incompatible types"));
                    }
                    Ok(StaticType::Bool)
                }
            }
        }
        Expr::Call(name, arguments) => {
            let argument_types = arguments
                .iter()
                .map(|argument| check_expr(argument, names))
                .collect::<Result<Vec<_>, _>>()?;
            check_call_type(name, &argument_types, names)
        }
    }
}

fn check_call_type(
    name: &str,
    arguments: &[StaticType],
    names: &HashMap<String, StaticType>,
) -> Result<StaticType, Diagnostic> {
    let count = arguments.len();
    let arity = |expected: usize| {
        if count == expected {
            Ok(())
        } else {
            Err(type_diagnostic(format!("{name} expects {expected} arguments")))
        }
    };
    let expect = |index: usize, ty: StaticType| {
        if compatible(&ty, &arguments[index]) {
            Ok(())
        } else {
            Err(type_diagnostic(format!("argument {} to {name} must be {ty:?}", index + 1)))
        }
    };
    match name {
        "range" => {
            arity(2)?;
            expect(0, StaticType::Int)?;
            expect(1, StaticType::Int)?;
            Ok(StaticType::Stream)
        }
        "take" => {
            arity(2)?;
            expect(0, StaticType::Stream)?;
            expect(1, StaticType::Int)?;
            Ok(StaticType::Stream)
        }
        "filter" | "take_while" => {
            arity(2)?;
            expect(0, StaticType::Stream)?;
            expect(1, StaticType::Function)?;
            Ok(StaticType::Stream)
        }
        "map" if count == 2 && arguments[0] == StaticType::Stream => {
            expect(1, StaticType::Function)?;
            Ok(StaticType::Stream)
        }
        "sum" | "count" | "checksum" | "sort" | "unique" | "chunks" | "enumerate"
        | "first" | "last" | "reverse" | "to_list" => {
            let expected = if matches!(name, "chunks") { 2 } else { 1 };
            arity(expected)?;
            if arguments[0] != StaticType::Stream
                && arguments[0] != StaticType::List
                && arguments[0] != StaticType::Tuple
                && arguments[0] != StaticType::Any
            {
                return Err(type_diagnostic(format!("{name} expects a collection")));
            }
            if name == "chunks" {
                expect(1, StaticType::Int)?;
            }
            Ok(if matches!(name, "sum" | "count" | "checksum") {
                StaticType::Int
            } else {
                StaticType::List
            })
        }
        "print" | "assert" | "json_stringify" | "parse_date" | "seconds" | "bytes"
        | "kilobytes" | "megabytes" | "abs" | "sqrt" | "floor" | "ceil" | "round"
        | "some" | "ok" | "err" | "is_some" | "is_ok" | "unwrap" | "len"
        | "text_len" | "to_string" => {
            arity(1)?;
            Ok(match name {
                "assert" => StaticType::Bool,
                "json_stringify" | "parse_date" | "to_string" => StaticType::Str,
                "some" | "unwrap" => StaticType::Option,
                "ok" | "err" => StaticType::Result,
                "is_some" | "is_ok" => StaticType::Bool,
                "len" | "text_len" => StaticType::Int,
                "seconds" => StaticType::Duration,
                "bytes" | "kilobytes" | "megabytes" => StaticType::Quantity,
                "sqrt" | "abs" | "floor" | "ceil" | "round" => StaticType::Any,
                _ => argument_types_for_result(name, arguments),
            })
        }
        "concat" | "min" | "max" | "list" | "tuple" | "record" | "json_parse"
        | "csv" | "tsv" | "module" | "now" | "date_add_days" | "date_diff_days"
        | "milliseconds" | "hours" | "days" | "convert" | "fold" | "reduce" | "zip"
        | "group_by" | "get" | "text_slice" | "text_find" | "replace" | "contains"
        | "match" | "unwrap_or" | "query_csv" | "zip_with"
        | "assert_eq" => Ok(match name {
            "list" => StaticType::List,
            "tuple" => StaticType::Tuple,
            "map" => StaticType::Map,
            "record" => StaticType::Record,
            "some" | "unwrap_or" => StaticType::Option,
            "ok" | "err" => StaticType::Result,
            "csv" | "tsv" | "zip" | "take_while" => StaticType::Stream,
            "date_diff_days" | "text_find" | "len" | "text_len" => StaticType::Int,
            "now" | "parse_date" => StaticType::Date,
            "seconds" | "milliseconds" | "hours" | "days" => StaticType::Duration,
            "bytes" | "kilobytes" | "megabytes" | "convert" => StaticType::Quantity,
            "contains" | "is_some" | "is_ok" | "assert" | "assert_eq" => StaticType::Bool,
            "text_slice" | "replace" | "concat" | "json_stringify" | "to_string" => {
                StaticType::Str
            }
            "fold" | "reduce" => StaticType::Any,
            _ => StaticType::List,
        }),
        _ if names.get(name) == Some(&StaticType::Function) => Ok(StaticType::Any),
        _ => Err(type_diagnostic(format!("unknown function: {name}"))),
    }
}

fn argument_types_for_result(_name: &str, arguments: &[StaticType]) -> StaticType {
    arguments.first().cloned().unwrap_or(StaticType::Any)
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
    Decimal(Decimal),
    Bool(bool),
    Str(String),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Map(BTreeMap<String, Value>),
    Record(BTreeMap<String, Value>),
    Option(Option<Box<Value>>),
    Result(Result<Box<Value>, Box<Value>>),
    Date(NaiveDate),
    Duration(i64),
    Quantity(Decimal, String),
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
            (Self::Decimal(a), Self::Decimal(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::List(a), Self::List(b)) | (Self::Tuple(a), Self::Tuple(b)) => a == b,
            (Self::Map(a), Self::Map(b)) | (Self::Record(a), Self::Record(b)) => a == b,
            (Self::Option(a), Self::Option(b)) => a == b,
            (Self::Result(a), Self::Result(b)) => a == b,
            (Self::Date(a), Self::Date(b)) => a == b,
            (Self::Duration(a), Self::Duration(b)) => a == b,
            (Self::Quantity(a, au), Self::Quantity(b, bu)) => a == b && au == bu,
            _ => false,
        }
    }
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "Int({n})"),
            Self::Decimal(n) => write!(f, "Decimal({n})"),
            Self::Bool(b) => write!(f, "Bool({b})"),
            Self::Str(s) => write!(f, "Str({s:?})"),
            Self::List(items) => f.debug_list().entries(items).finish(),
            Self::Tuple(items) => write!(f, "Tuple({items:?})"),
            Self::Map(fields) => write!(f, "Map({fields:?})"),
            Self::Record(fields) => write!(f, "Record({fields:?})"),
            Self::Option(value) => write!(f, "Option({value:?})"),
            Self::Result(value) => write!(f, "Result({value:?})"),
            Self::Date(value) => write!(f, "Date({value})"),
            Self::Duration(value) => write!(f, "Duration({value})"),
            Self::Quantity(value, unit) => write!(f, "Quantity({value}, {unit})"),
            Self::Stream(s) => fmt::Debug::fmt(s, f),
            Self::Function(_) => f.write_str("<function>"),
        }
    }
}
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Decimal(n) => write!(f, "{}", n.normalize()),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Str(s) => f.write_str(s),
            Self::List(items) => write_sequence(f, "[", "]", items),
            Self::Tuple(items) => write_sequence(f, "(", ")", items),
            Self::Map(fields) | Self::Record(fields) => {
                f.write_str("{")?;
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index != 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{key}: {value}")?;
                }
                f.write_str("}")
            }
            Self::Option(Some(value)) => write!(f, "some({value})"),
            Self::Option(None) => f.write_str("none"),
            Self::Result(Ok(value)) => write!(f, "ok({value})"),
            Self::Result(Err(value)) => write!(f, "err({value})"),
            Self::Date(value) => write!(f, "{}", value.format("%Y-%m-%d")),
            Self::Duration(value) => write!(f, "{value}s"),
            Self::Quantity(value, unit) => write!(f, "{value} {unit}"),
            Self::Stream(_) => f.write_str("<lazy-stream>"),
            Self::Function(_) => f.write_str("<function>"),
        }
    }
}
fn write_sequence(f: &mut fmt::Formatter<'_>, open: &str, close: &str, items: &[Value]) -> fmt::Result {
    f.write_str(open)?;
    for (index, item) in items.iter().enumerate() {
        if index != 0 {
            f.write_str(", ")?;
        }
        write!(f, "{item}")?;
    }
    f.write_str(close)
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
    eval_with_context(program, limits, &EvalContext::default())
}

pub fn eval_with_context(
    program: &Program,
    limits: Limits,
    context: &EvalContext,
) -> Result<Value, RuntimeError> {
    check(program).map_err(|diagnostic| err(RuntimeErrorKind::Type, diagnostic.to_string()))?;
    let state = Rc::new(RefCell::new(State {
        fuel: limits.fuel,
        deadline: Instant::now().checked_add(limits.timeout),
    }));
    let mut env = Env::new();
    let mut result = Value::Bool(false);
    for expression in &program.expressions {
        tick(&state)?;
        result = eval_expr(expression, &mut env, &state, limits, context, 0)?;
    }
    Ok(result)
}
fn eval_expr(
    expr: &Expr,
    env: &mut Env,
    state: &Rc<RefCell<State>>,
    limits: Limits,
    context: &EvalContext,
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
        Expr::Decimal(n) => Ok(Value::Decimal(*n)),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Str(s) => Ok(Value::Str(s.clone())),
        Expr::Var(n) => env
            .get(n)
            .cloned()
            .ok_or_else(|| err(RuntimeErrorKind::UnknownName, format!("unknown name: {n}"))),
        Expr::Let(n, e) => {
            let v = eval_expr(e, env, state, limits, context, depth + 1)?;
            env.insert(n.clone(), v.clone());
            Ok(v)
        }
        Expr::Lambda(param, body) => Ok(Value::Function(Closure {
            param: param.clone(),
            body: (**body).clone(),
            env: env.clone(),
        })),
        Expr::List(items) => items
            .iter()
            .map(|item| eval_expr(item, env, state, limits, context, depth + 1))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
        Expr::Record(fields) => {
            let mut values = BTreeMap::new();
            for (key, expression) in fields {
                values.insert(
                    key.clone(),
                    eval_expr(expression, env, state, limits, context, depth + 1)?,
                );
            }
            Ok(Value::Record(values))
        }
        Expr::Binary(a, op, b) => {
            let left = eval_expr(a, env, state, limits, context, depth + 1)?;
            if *op == Op::And && left == Value::Bool(false) {
                return Ok(Value::Bool(false));
            }
            if *op == Op::Or && left == Value::Bool(true) {
                return Ok(Value::Bool(true));
            }
            let right = eval_expr(b, env, state, limits, context, depth + 1)?;
            apply_binary(left, *op, right)
        }
        Expr::Call(name, args) => call(name, args, env, state, limits, context, depth + 1),
    }
}
fn int(v: Value) -> Result<i64, RuntimeError> {
    if let Value::Int(n) = v {
        Ok(n)
    } else {
        Err(err(RuntimeErrorKind::Type, "expected integer"))
    }
}
fn decimal_value(value: Value) -> Result<Decimal, RuntimeError> {
    match value {
        Value::Int(value) => Ok(Decimal::from(value)),
        Value::Decimal(value) => Ok(value),
        _ => Err(err(RuntimeErrorKind::Type, "expected numeric value")),
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
    if let (Value::Str(left), Value::Str(right)) = (&a, &b) {
        return match op {
            Add => Ok(Value::Str(format!("{left}{right}"))),
            Lt => Ok(Value::Bool(left < right)),
            Le => Ok(Value::Bool(left <= right)),
            Gt => Ok(Value::Bool(left > right)),
            Ge => Ok(Value::Bool(left >= right)),
            _ => Err(err(RuntimeErrorKind::Type, "operator is not supported for text")),
        };
    }
    if matches!(a, Value::Decimal(_)) || matches!(b, Value::Decimal(_)) {
        let left = decimal_value(a)?;
        let right = decimal_value(b)?;
        if matches!(op, Div | Rem) && right.is_zero() {
            return Err(err(RuntimeErrorKind::DivisionByZero, "decimal division by zero"));
        }
        return match op {
            Add => left.checked_add(right).map(Value::Decimal),
            Sub => left.checked_sub(right).map(Value::Decimal),
            Mul => left.checked_mul(right).map(Value::Decimal),
            Div => left.checked_div(right).map(Value::Decimal),
            Rem => left.checked_rem(right).map(Value::Decimal),
            Lt => return Ok(Value::Bool(left < right)),
            Le => return Ok(Value::Bool(left <= right)),
            Gt => return Ok(Value::Bool(left > right)),
            Ge => return Ok(Value::Bool(left >= right)),
            _ => None,
        }
        .ok_or_else(|| err(RuntimeErrorKind::Overflow, "fixed-point decimal overflow"));
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
fn invoke(
    function: Closure,
    argument: Value,
    state: &Rc<RefCell<State>>,
    limits: Limits,
    context: &EvalContext,
) -> Result<Value, RuntimeError> {
    let mut local = function.env;
    local.insert(function.param, argument);
    eval_expr(&function.body, &mut local, state, limits, context, 0)
}

fn collection_values(value: Value) -> Result<Vec<Value>, RuntimeError> {
    match value {
        Value::List(values) | Value::Tuple(values) => Ok(values),
        Value::Stream(stream) => stream.collect_limited(usize::MAX),
        _ => Err(err(RuntimeErrorKind::Type, "expected a collection")),
    }
}

fn make_stream<I: Iterator<Item = Result<Value, RuntimeError>> + 'static>(
    iter: I,
    state: &Rc<RefCell<State>>,
    limits: Limits,
) -> Value {
    Value::Stream(Stream::new(iter, state.clone(), limits.max_output_bytes))
}

fn decimal_sqrt(value: Decimal) -> Result<Decimal, RuntimeError> {
    if value.is_zero() {
        return Ok(Decimal::ZERO);
    }
    let mut estimate = if value > Decimal::ONE { value } else { Decimal::ONE };
    for _ in 0..64 {
        let quotient = value
            .checked_div(estimate)
            .ok_or_else(|| err(RuntimeErrorKind::Overflow, "decimal square root overflow"))?;
        let next = estimate
            .checked_add(quotient)
            .and_then(|sum| sum.checked_div(Decimal::from(2)))
            .ok_or_else(|| err(RuntimeErrorKind::Overflow, "decimal square root overflow"))?;
        if next == estimate {
            break;
        }
        estimate = next;
    }
    Ok(estimate)
}

fn json_to_value(value: serde_json::Value) -> Result<Value, RuntimeError> {
    Ok(match value {
        serde_json::Value::Null => Value::Option(None),
        serde_json::Value::Bool(value) => Value::Bool(value),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Value::Int(value)
            } else {
                let decimal = Decimal::from_str_exact(&number.to_string())
                    .map_err(|error| err(RuntimeErrorKind::Json, error.to_string()))?;
                Value::Decimal(decimal)
            }
        }
        serde_json::Value::String(value) => Value::Str(value),
        serde_json::Value::Array(values) => Value::List(
            values
                .into_iter()
                .map(json_to_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        serde_json::Value::Object(fields) => Value::Record(
            fields
                .into_iter()
                .map(|(key, value)| Ok((key, json_to_value(value)?)))
                .collect::<Result<BTreeMap<_, _>, RuntimeError>>()?,
        ),
    })
}

fn value_to_json(value: Value) -> Result<serde_json::Value, RuntimeError> {
    use serde_json::{Map, Number, Value as Json};
    Ok(match value {
        Value::Int(value) => Json::Number(Number::from(value)),
        Value::Decimal(value) => Json::Number(
            Number::from_str(&value.normalize().to_string())
                .map_err(|error| err(RuntimeErrorKind::Json, error.to_string()))?,
        ),
        Value::Bool(value) => Json::Bool(value),
        Value::Str(value) => Json::String(value),
        Value::List(values) | Value::Tuple(values) => Json::Array(
            values
                .into_iter()
                .map(value_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Map(fields) | Value::Record(fields) => Json::Object(
            fields
                .into_iter()
                .map(|(key, value)| Ok((key, value_to_json(value)?)))
                .collect::<Result<Map<_, _>, RuntimeError>>()?,
        ),
        Value::Option(Some(value)) | Value::Result(Ok(value)) => value_to_json(*value)?,
        Value::Option(None) => Json::Null,
        Value::Result(Err(value)) => {
            let mut object = Map::new();
            object.insert("error".into(), value_to_json(*value)?);
            Json::Object(object)
        }
        Value::Date(value) => Json::String(value.format("%Y-%m-%d").to_string()),
        Value::Duration(value) => Json::Number(Number::from(value)),
        Value::Quantity(value, unit) => {
            let mut object = Map::new();
            object.insert("value".into(), Json::String(value.to_string()));
            object.insert("unit".into(), Json::String(unit));
            Json::Object(object)
        }
        Value::Stream(_) | Value::Function(_) => {
            return Err(err(RuntimeErrorKind::Json, "streams and functions cannot be serialized as JSON"))
        }
    })
}
fn call(
    name: &str,
    args: &[Expr],
    env: &mut Env,
    state: &Rc<RefCell<State>>,
    limits: Limits,
    context: &EvalContext,
    depth: usize,
) -> Result<Value, RuntimeError> {
    if let Some(Value::Function(function)) = env.get(name).cloned() {
        if args.len() != 1 {
            return Err(err(RuntimeErrorKind::Type, format!("{name} expects 1 argument")));
        }
        let argument = eval_expr(&args[0], env, state, limits, context, depth + 1)?;
        return invoke(function, argument, state, limits, context);
    }
    let eval_arg = |i: usize, env: &mut Env| -> Result<Value, RuntimeError> {
        args.get(i)
            .ok_or_else(|| {
                err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects more arguments"),
                )
            })
            .and_then(|e| eval_expr(e, env, state, limits, context, depth + 1))
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
        "map" | "filter" | "take_while" => {
            if args.len() != 2 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 2 arguments"),
                ));
            }
            let source = eval_arg(0, env)?;
            if name == "map" && !matches!(source, Value::Stream(_)) {
                let entries = collection_values(source)?;
                if entries.len() % 2 != 0 {
                    return Err(err(RuntimeErrorKind::Type, "map expects key/value pairs"));
                }
                let mut map = BTreeMap::new();
                for pair in entries.chunks_exact(2) {
                    let Value::Str(key) = &pair[0] else {
                        return Err(err(RuntimeErrorKind::Type, "map keys must be text"));
                    };
                    map.insert(key.clone(), pair[1].clone());
                }
                return Ok(Value::Map(map));
            }
            let stream = match source {
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
            let is_map = name == "map";
            let is_take_while = name == "take_while";
            let iter = std::iter::from_fn(move || loop {
                let input = match src.next()? {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                match invoke(function.clone(), input.clone(), &state2, limits2) {
                    Err(e) => return Some(Err(e)),
                    Ok(v) if is_map => return Some(Ok(v)),
                    Ok(Value::Bool(true)) => return Some(Ok(input)),
                    Ok(Value::Bool(false)) if is_take_while => return None,
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
            let values = collection_values(eval_arg(0, env)?)?;
            let mut total = 0i64;
            let mut count = 0i64;
            let mut hash = 0xcbf29ce484222325u64;
            for value in values {
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
        "fold" | "reduce" => {
            let (mut accumulator, values, function) = if name == "fold" && args.len() == 3 {
                let values = collection_values(eval_arg(0, env)?)?;
                let accumulator = eval_arg(1, env)?;
                let Value::Function(function) = eval_arg(2, env)? else {
                    return Err(err(RuntimeErrorKind::Type, "fold expects a function"));
                };
                (accumulator, values, function)
            } else if name == "reduce" && args.len() == 2 {
                let values = collection_values(eval_arg(0, env)?)?;
                let Value::Function(function) = eval_arg(1, env)? else {
                    return Err(err(RuntimeErrorKind::Type, "reduce expects a function"));
                };
                let mut values = values.into_iter();
                let first = values
                    .next()
                    .ok_or_else(|| err(RuntimeErrorKind::Type, "reduce cannot use an empty collection"))?;
                (first, values.collect(), function)
            } else {
                return Err(err(RuntimeErrorKind::Type, format!("invalid {name} arguments")));
            };
            for value in values {
                accumulator = invoke(
                    function.clone(),
                    Value::Tuple(vec![accumulator, value]),
                    state,
                    limits,
                )?;
            }
            Ok(accumulator)
        }
        "enumerate" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, "enumerate expects 1 argument"));
            }
            let Value::Stream(mut stream) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "enumerate expects a stream"));
            };
            let mut index = 0i64;
            let iter = std::iter::from_fn(move || {
                let value = stream.next()?;
                let current = index;
                index = index.saturating_add(1);
                Some(value.map(|value| Value::Tuple(vec![Value::Int(current), value])))
            });
            Ok(make_stream(iter, state, limits))
        }
        "zip" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "zip expects 2 arguments"));
            }
            let Value::Stream(mut left) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "zip expects streams"));
            };
            let Value::Stream(mut right) = eval_arg(1, env)? else {
                return Err(err(RuntimeErrorKind::Type, "zip expects streams"));
            };
            let iter = std::iter::from_fn(move || match (left.next(), right.next()) {
                (Some(Ok(a)), Some(Ok(b))) => Some(Ok(Value::Tuple(vec![a, b]))),
                (Some(Err(error)), _) | (_, Some(Err(error))) => Some(Err(error)),
                _ => None,
            });
            Ok(make_stream(iter, state, limits))
        }
        "chunks" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "chunks expects 2 arguments"));
            }
            let values = collection_values(eval_arg(0, env)?)?;
            let size = int(eval_arg(1, env)?)?;
            if size <= 0 {
                return Err(err(RuntimeErrorKind::Type, "chunk size must be positive"));
            }
            Ok(Value::List(
                values
                    .chunks(size as usize)
                    .map(|chunk| Value::List(chunk.to_vec()))
                    .collect(),
            ))
        }
        "sort" | "unique" | "reverse" | "to_list" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, format!("{name} expects 1 argument")));
            }
            let mut values = collection_values(eval_arg(0, env)?)?;
            match name {
                "sort" => {
                    values.sort_by(|a, b| match (a, b) {
                        (Value::Int(a), Value::Int(b)) => a.cmp(b),
                        (Value::Decimal(a), Value::Decimal(b)) => a.cmp(b),
                        (Value::Str(a), Value::Str(b)) => a.cmp(b),
                        _ => std::cmp::Ordering::Equal,
                    });
                }
                "unique" => {
                    let mut unique = Vec::new();
                    for value in values {
                        if !unique.contains(&value) {
                            unique.push(value);
                        }
                    }
                    values = unique;
                }
                "reverse" => values.reverse(),
                _ => {}
            }
            Ok(Value::List(values))
        }
        "list" | "tuple" => {
            let values = args
                .iter()
                .enumerate()
                .map(|(index, _)| eval_arg(index, env))
                .collect::<Result<Vec<_>, _>>()?;
            if name == "list" {
                Ok(Value::List(values))
            } else {
                Ok(Value::Tuple(values))
            }
        }
        "record" => {
            if args.len() % 2 != 0 {
                return Err(err(RuntimeErrorKind::Type, "record expects text/value pairs"));
            }
            let mut fields = BTreeMap::new();
            for index in (0..args.len()).step_by(2) {
                let Value::Str(key) = eval_arg(index, env)? else {
                    return Err(err(RuntimeErrorKind::Type, "record keys must be text"));
                };
                fields.insert(key, eval_arg(index + 1, env)?);
            }
            Ok(Value::Record(fields))
        }
        "get" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "get expects 2 arguments"));
            }
            let value = eval_arg(0, env)?;
            let key = eval_arg(1, env)?;
            match (value, key) {
                (Value::Map(fields), Value::Str(key))
                | (Value::Record(fields), Value::Str(key)) => Ok(Value::Option(
                    fields.get(&key).cloned().map(Box::new),
                )),
                (Value::List(items), Value::Int(index))
                | (Value::Tuple(items), Value::Int(index)) if index >= 0 => Ok(Value::Option(
                    items.get(index as usize).cloned().map(Box::new),
                )),
                _ => Err(err(RuntimeErrorKind::Type, "get expects a map/record and text key, or list and integer index")),
            }
        }
        "concat" => {
            let mut output = String::new();
            for index in 0..args.len() {
                let Value::Str(value) = eval_arg(index, env)? else {
                    return Err(err(RuntimeErrorKind::Type, "concat expects text arguments"));
                };
                output.push_str(&value);
                if output.len() > limits.max_output_bytes {
                    return Err(err(RuntimeErrorKind::OutputLimit, "text output limit exceeded"));
                }
            }
            Ok(Value::Str(output))
        }
        "text_slice" => {
            if args.len() != 3 {
                return Err(err(RuntimeErrorKind::Type, "text_slice expects text, start, and length"));
            }
            let Value::Str(text) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "text_slice expects text"));
            };
            let start = int(eval_arg(1, env)?)?;
            let length = int(eval_arg(2, env)?)?;
            if start < 0 || length < 0 {
                return Err(err(RuntimeErrorKind::Type, "text slice bounds must be nonnegative"));
            }
            let output = text.chars().skip(start as usize).take(length as usize).collect::<String>();
            Ok(Value::Str(output))
        }
        "text_find" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "text_find expects 2 arguments"));
            }
            let Value::Str(text) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "text_find expects text"));
            };
            let Value::Str(needle) = eval_arg(1, env)? else {
                return Err(err(RuntimeErrorKind::Type, "text_find expects text"));
            };
            let position = text
                .find(&needle)
                .map(|byte_index| text[..byte_index].chars().count() as i64)
                .unwrap_or(-1);
            Ok(Value::Int(position))
        }
        "replace" => {
            if args.len() != 3 {
                return Err(err(RuntimeErrorKind::Type, "replace expects text, pattern, and replacement"));
            }
            let Value::Str(text) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "replace expects text"));
            };
            let Value::Str(pattern) = eval_arg(1, env)? else {
                return Err(err(RuntimeErrorKind::Type, "replace expects text pattern"));
            };
            let Value::Str(replacement) = eval_arg(2, env)? else {
                return Err(err(RuntimeErrorKind::Type, "replace expects text replacement"));
            };
            Ok(Value::Str(text.replace(&pattern, &replacement)))
        }
        "contains" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "contains expects 2 arguments"));
            }
            let container = eval_arg(0, env)?;
            let needle = eval_arg(1, env)?;
            let found = match container {
                Value::Str(text) => matches!(needle, Value::Str(ref value) if text.contains(value)),
                Value::List(values) | Value::Tuple(values) => values.contains(&needle),
                _ => return Err(err(RuntimeErrorKind::Type, "contains expects text or a list/tuple")),
            };
            Ok(Value::Bool(found))
        }
        "len" | "text_len" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, format!("{name} expects 1 argument")));
            }
            let length = match eval_arg(0, env)? {
                Value::Str(value) => value.chars().count(),
                Value::List(value) | Value::Tuple(value) => value.len(),
                Value::Map(value) | Value::Record(value) => value.len(),
                _ => return Err(err(RuntimeErrorKind::Type, "len expects text or a collection")),
            };
            Ok(Value::Int(i64::try_from(length).map_err(|_| err(RuntimeErrorKind::Overflow, "length overflow"))?))
        }
        "to_string" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, "to_string expects 1 argument"));
            }
            Ok(Value::Str(eval_arg(0, env)?.to_string()))
        }
        "none" => {
            if !args.is_empty() {
                return Err(err(RuntimeErrorKind::Type, "none expects no arguments"));
            }
            Ok(Value::Option(None))
        }
        "some" | "ok" | "err" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, format!("{name} expects 1 argument")));
            }
            let value = Box::new(eval_arg(0, env)?);
            Ok(match name {
                "some" => Value::Option(Some(value)),
                "ok" => Value::Result(Ok(value)),
                _ => Value::Result(Err(value)),
            })
        }
        "is_some" | "is_ok" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, format!("{name} expects 1 argument")));
            }
            let value = eval_arg(0, env)?;
            Ok(Value::Bool(match (name, value) {
                ("is_some", Value::Option(Some(_))) | ("is_ok", Value::Result(Ok(_))) => true,
                ("is_some", Value::Option(None)) | ("is_ok", Value::Result(Err(_))) => false,
                _ => return Err(err(RuntimeErrorKind::Type, format!("{name} expects Option or Result"))),
            }))
        }
        "unwrap" | "unwrap_or" => {
            let expected = if name == "unwrap" { 1 } else { 2 };
            if args.len() != expected {
                return Err(err(RuntimeErrorKind::Type, format!("{name} expects {expected} arguments")));
            }
            let value = eval_arg(0, env)?;
            match value {
                Value::Option(Some(value)) | Value::Result(Ok(value)) => Ok(*value),
                Value::Option(None) if name == "unwrap_or" => eval_arg(1, env),
                Value::Option(None) => Err(err(RuntimeErrorKind::Type, "cannot unwrap none")),
                Value::Result(Err(value)) => Err(err(RuntimeErrorKind::Type, format!("cannot unwrap error: {value}"))),
                _ => Err(err(RuntimeErrorKind::Type, "unwrap expects Option or Result")),
            }
        }
        "match" => {
            if args.len() != 3 {
                return Err(err(RuntimeErrorKind::Type, "match expects a value and two handler functions"));
            }
            let value = eval_arg(0, env)?;
            let on_first = eval_arg(1, env)?;
            let on_second = eval_arg(2, env)?;
            let (function, payload) = match (value, on_first, on_second) {
                (Value::Option(Some(value)), Value::Function(first), _) => (first, *value),
                (Value::Option(None), _, Value::Function(second)) => (second, Value::Option(None)),
                (Value::Result(Ok(value)), Value::Function(first), _) => (first, *value),
                (Value::Result(Err(value)), _, Value::Function(second)) => (second, *value),
                (Value::Bool(true), Value::Function(first), _) => (first, Value::Bool(true)),
                (Value::Bool(false), _, Value::Function(second)) => (second, Value::Bool(false)),
                _ => return Err(err(RuntimeErrorKind::Type, "match expects Option, Result, or Bool and two functions")),
            };
            invoke(function, payload, state, limits)
        }
        "assert" | "assert_eq" => {
            let condition = if name == "assert" && args.len() == 1 {
                eval_arg(0, env)? == Value::Bool(true)
            } else if name == "assert_eq" && args.len() == 2 {
                eval_arg(0, env)? == eval_arg(1, env)?
            } else {
                return Err(err(RuntimeErrorKind::Type, format!("invalid {name} arguments")));
            };
            if !condition {
                return Err(err(RuntimeErrorKind::Assertion, format!("{name} failed")));
            }
            Ok(Value::Bool(true))
        }
        "json_parse" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, "json_parse expects 1 argument"));
            }
            let Value::Str(source) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "json_parse expects text"));
            };
            let json: serde_json::Value = serde_json::from_str(&source)
                .map_err(|error| err(RuntimeErrorKind::Json, error.to_string()))?;
            json_to_value(json)
        }
        "json_stringify" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, "json_stringify expects 1 argument"));
            }
            let value = eval_arg(0, env)?;
            let json = value_to_json(value)?;
            serde_json::to_string(&json)
                .map(Value::Str)
                .map_err(|error| err(RuntimeErrorKind::Json, error.to_string()))
        }
        "abs" | "sqrt" | "floor" | "ceil" | "round" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, format!("{name} expects 1 argument")));
            }
            let value = eval_arg(0, env)?;
            match (name, value) {
                ("abs", Value::Int(value)) => value.checked_abs().map(Value::Int).ok_or_else(|| err(RuntimeErrorKind::Overflow, "absolute value overflow")),
                ("abs", Value::Decimal(value)) => Ok(Value::Decimal(value.abs())),
                ("sqrt", Value::Int(value)) if value >= 0 => Ok(Value::Int((value as f64).sqrt().floor() as i64)),
                ("sqrt", Value::Decimal(value)) if value >= Decimal::ZERO => decimal_sqrt(value).map(Value::Decimal),
                ("sqrt", _) => Err(err(RuntimeErrorKind::Type, "sqrt expects a nonnegative number")),
                ("floor", Value::Decimal(value)) => Ok(Value::Int(value.floor().to_i64().ok_or_else(|| err(RuntimeErrorKind::Overflow, "floor overflow"))?)),
                ("ceil", Value::Decimal(value)) => Ok(Value::Int(value.ceil().to_i64().ok_or_else(|| err(RuntimeErrorKind::Overflow, "ceil overflow"))?)),
                ("round", Value::Decimal(value)) => Ok(Value::Int(value.round().to_i64().ok_or_else(|| err(RuntimeErrorKind::Overflow, "round overflow"))?)),
                ("floor" | "ceil" | "round", Value::Int(value)) => Ok(Value::Int(value)),
                _ => Err(err(RuntimeErrorKind::Type, format!("{name} expects a number"))),
            }
        }
        "min" | "max" => {
            if args.is_empty() {
                return Err(err(RuntimeErrorKind::Type, format!("{name} expects values")));
            }
            let values = (0..args.len()).map(|index| eval_arg(index, env)).collect::<Result<Vec<_>, _>>()?;
            let compare = |a: &Value, b: &Value| -> Result<std::cmp::Ordering, RuntimeError> {
                match (a, b) {
                    (Value::Int(a), Value::Int(b)) => Ok(a.cmp(b)),
                    (Value::Decimal(a), Value::Decimal(b)) => Ok(a.cmp(b)),
                    (Value::Int(a), Value::Decimal(b)) => Ok(Decimal::from(*a).cmp(b)),
                    (Value::Decimal(a), Value::Int(b)) => Ok(a.cmp(&Decimal::from(*b))),
                    (Value::Str(a), Value::Str(b)) => Ok(a.cmp(b)),
                    _ => Err(err(RuntimeErrorKind::Type, format!("{name} values must share a comparable type"))),
                }
            };
            let selected = values.into_iter().try_fold(None, |selected: Option<Value>, value| {
                match selected {
                    None => Ok(Some(value)),
                    Some(current) => {
                        let ordering = compare(&value, &current)?;
                        let replace = if name == "min" { ordering.is_lt() } else { ordering.is_gt() };
                        Ok(Some(if replace { value } else { current }))
                    }
                }
            })?;
            selected.ok_or_else(|| err(RuntimeErrorKind::Type, "no values to compare"))
        }
        "parse_date" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, "parse_date expects 1 argument"));
            }
            let Value::Str(value) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "parse_date expects text"));
            };
            NaiveDate::parse_from_str(&value, "%Y-%m-%d")
                .map(Value::Date)
                .map_err(|error| err(RuntimeErrorKind::InvalidDate, error.to_string()))
        }
        "now" => Ok(Value::Date(chrono::Utc::now().date_naive())),
        "date_add_days" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "date_add_days expects date and integer"));
            }
            let Value::Date(date) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "date_add_days expects a date"));
            };
            let days = int(eval_arg(1, env)?)?;
            let days = i64::try_from(days).map_err(|_| err(RuntimeErrorKind::Overflow, "date offset overflow"))?;
            date.checked_add_signed(chrono::TimeDelta::days(days))
                .map(Value::Date)
                .ok_or_else(|| err(RuntimeErrorKind::Overflow, "date overflow"))
        }
        "date_diff_days" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "date_diff_days expects 2 dates"));
            }
            let Value::Date(start) = eval_arg(0, env)? else { return Err(err(RuntimeErrorKind::Type, "expected a date")); };
            let Value::Date(end) = eval_arg(1, env)? else { return Err(err(RuntimeErrorKind::Type, "expected a date")); };
            Ok(Value::Int(end.signed_duration_since(start).num_days()))
        }
        "seconds" | "milliseconds" | "hours" | "days" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, format!("{name} expects 1 argument")));
            }
            let value = int(eval_arg(0, env)?)?;
            let multiplier = match name { "seconds" => 1_000, "milliseconds" => 1, "hours" => 3_600_000, _ => 86_400_000 };
            value.checked_mul(multiplier).map(Value::Duration).ok_or_else(|| err(RuntimeErrorKind::Overflow, "duration overflow"))
        }
        "bytes" | "kilobytes" | "megabytes" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, format!("{name} expects 1 argument")));
            }
            let amount = decimal_value(eval_arg(0, env)?)?;
            let multiplier = match name { "bytes" => Decimal::ONE, "kilobytes" => Decimal::from(1_000), _ => Decimal::from(1_000_000) };
            Ok(Value::Quantity(amount.checked_mul(multiplier).ok_or_else(|| err(RuntimeErrorKind::Overflow, "quantity overflow"))?, "B".into()))
        }
        "convert" => {
            if args.len() != 2 {
                return Err(err(RuntimeErrorKind::Type, "convert expects quantity and unit"));
            }
            let Value::Quantity(bytes, _) = eval_arg(0, env)? else { return Err(err(RuntimeErrorKind::Type, "convert expects a quantity")); };
            let Value::Str(unit) = eval_arg(1, env)? else { return Err(err(RuntimeErrorKind::Type, "convert expects a unit name")); };
            let divisor = match unit.as_str() { "B" | "byte" => Decimal::ONE, "KB" => Decimal::from(1_000), "MB" => Decimal::from(1_000_000), "KiB" => Decimal::from(1_024), "MiB" => Decimal::from(1_048_576), _ => return Err(err(RuntimeErrorKind::InvalidUnit, "unsupported byte unit")) };
            Ok(Value::Quantity(bytes.checked_div(divisor).ok_or_else(|| err(RuntimeErrorKind::Overflow, "unit conversion overflow"))?, unit))
        }
        _ => Err(err(
            RuntimeErrorKind::UnknownName,
            format!("unknown function: {name}"),
        )),
    }
}
