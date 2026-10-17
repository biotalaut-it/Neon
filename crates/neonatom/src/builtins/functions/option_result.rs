use crate::{
    runtime::{err, eval_expr, invoke, State},
    syntax::{EvalContext, Expr, Limits, RuntimeError, RuntimeErrorKind},
    value::{Env, Value},
};
use std::{cell::RefCell, rc::Rc};

pub(super) fn call(
    name: &str,
    args: &[Expr],
    env: &mut Env,
    state: &Rc<RefCell<State>>,
    limits: Limits,
    context: &EvalContext,
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
            .and_then(|expression| eval_expr(expression, env, state, limits, context, depth + 1))
    };
    match name {
        "none" => {
            if !args.is_empty() {
                return Err(err(RuntimeErrorKind::Type, "none expects no arguments"));
            }
            Ok(Value::Option(None))
        }
        "some" | "ok" | "err" => {
            if args.len() != 1 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 1 argument"),
                ));
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
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 1 argument"),
                ));
            }
            let value = eval_arg(0, env)?;
            Ok(Value::Bool(match (name, value) {
                ("is_some", Value::Option(Some(_))) | ("is_ok", Value::Result(Ok(_))) => true,
                ("is_some", Value::Option(None)) | ("is_ok", Value::Result(Err(_))) => false,
                _ => {
                    return Err(err(
                        RuntimeErrorKind::Type,
                        format!("{name} expects Option or Result"),
                    ))
                }
            }))
        }
        "unwrap" | "unwrap_or" => {
            let expected = if name == "unwrap" { 1 } else { 2 };
            if args.len() != expected {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects {expected} arguments"),
                ));
            }
            let value = eval_arg(0, env)?;
            match value {
                Value::Option(Some(value)) | Value::Result(Ok(value)) => Ok(*value),
                Value::Option(None) if name == "unwrap_or" => eval_arg(1, env),
                Value::Option(None) => Err(err(RuntimeErrorKind::Type, "cannot unwrap none")),
                Value::Result(Err(value)) => Err(err(
                    RuntimeErrorKind::Type,
                    format!("cannot unwrap error: {value}"),
                )),
                _ => Err(err(
                    RuntimeErrorKind::Type,
                    "unwrap expects Option or Result",
                )),
            }
        }
        "match" => {
            if args.len() != 3 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    "match expects a value and two handler functions",
                ));
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
                _ => {
                    return Err(err(
                        RuntimeErrorKind::Type,
                        "match expects Option, Result, or Bool and two functions",
                    ))
                }
            };
            invoke(function, payload, state, limits, context)
        }
        "assert" | "assert_eq" => {
            let condition = if name == "assert" && args.len() == 1 {
                eval_arg(0, env)? == Value::Bool(true)
            } else if name == "assert_eq" && args.len() == 2 {
                eval_arg(0, env)? == eval_arg(1, env)?
            } else {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("invalid {name} arguments"),
                ));
            };
            if !condition {
                return Err(err(RuntimeErrorKind::Assertion, format!("{name} failed")));
            }
            Ok(Value::Bool(true))
        }
        _ => unreachable!("builtin routed to the wrong category"),
    }
}
