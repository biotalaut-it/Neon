mod lexer;
mod parser;
mod type_check;
mod types;

pub use parser::parse;
pub use type_check::check;
pub use types::{
    Diagnostic, DiagnosticKind, EvalContext, Expr, Limits, Op, Program, RuntimeError,
    RuntimeErrorKind, Span, StaticType,
};
