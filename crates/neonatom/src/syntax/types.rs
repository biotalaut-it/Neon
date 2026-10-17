use rust_decimal::Decimal;
use std::{fmt, path::PathBuf, time::Duration};

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
    _local_root: Option<PathBuf>,
}
impl EvalContext {
    pub fn with_local_root(root: impl Into<PathBuf>) -> Self {
        Self {
            _local_root: Some(root.into()),
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
