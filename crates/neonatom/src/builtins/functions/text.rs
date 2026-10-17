use crate::{
    runtime::{err, eval_expr, int, State},
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
        "concat" => {
            let mut output = String::new();
            for index in 0..args.len() {
                let Value::Str(value) = eval_arg(index, env)? else {
                    return Err(err(RuntimeErrorKind::Type, "concat expects text arguments"));
                };
                output.push_str(&value);
                if output.len() > limits.max_output_bytes {
                    return Err(err(
                        RuntimeErrorKind::OutputLimit,
                        "text output limit exceeded",
                    ));
                }
            }
            Ok(Value::Str(output))
        }
        "text_slice" => {
            if args.len() != 3 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    "text_slice expects text, start, and length",
                ));
            }
            let Value::Str(text) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "text_slice expects text"));
            };
            let start = int(eval_arg(1, env)?)?;
            let length = int(eval_arg(2, env)?)?;
            if start < 0 || length < 0 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    "text slice bounds must be nonnegative",
                ));
            }
            let output = text
                .chars()
                .skip(start as usize)
                .take(length as usize)
                .collect::<String>();
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
                return Err(err(
                    RuntimeErrorKind::Type,
                    "replace expects text, pattern, and replacement",
                ));
            }
            let Value::Str(text) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "replace expects text"));
            };
            let Value::Str(pattern) = eval_arg(1, env)? else {
                return Err(err(RuntimeErrorKind::Type, "replace expects text pattern"));
            };
            let Value::Str(replacement) = eval_arg(2, env)? else {
                return Err(err(
                    RuntimeErrorKind::Type,
                    "replace expects text replacement",
                ));
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
                _ => {
                    return Err(err(
                        RuntimeErrorKind::Type,
                        "contains expects text or a list/tuple",
                    ))
                }
            };
            Ok(Value::Bool(found))
        }
        "len" | "text_len" => {
            if args.len() != 1 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 1 argument"),
                ));
            }
            let length = match eval_arg(0, env)? {
                Value::Str(value) => value.chars().count(),
                Value::List(value) | Value::Tuple(value) => value.len(),
                Value::Map(value) | Value::Record(value) => value.len(),
                _ => {
                    return Err(err(
                        RuntimeErrorKind::Type,
                        "len expects text or a collection",
                    ))
                }
            };
            Ok(Value::Int(i64::try_from(length).map_err(|_| {
                err(RuntimeErrorKind::Overflow, "length overflow")
            })?))
        }
        "to_string" => {
            if args.len() != 1 {
                return Err(err(RuntimeErrorKind::Type, "to_string expects 1 argument"));
            }
            Ok(Value::Str(eval_arg(0, env)?.to_string()))
        }
        _ => unreachable!("builtin routed to the wrong category"),
    }
}
