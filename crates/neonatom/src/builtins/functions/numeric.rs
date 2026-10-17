use crate::{
    runtime::{decimal_sqrt, err, eval_expr, State},
    syntax::{EvalContext, Expr, Limits, RuntimeError, RuntimeErrorKind},
    value::{Env, Value},
};
use rust_decimal::{prelude::ToPrimitive, Decimal};
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
        "abs" | "sqrt" | "floor" | "ceil" | "round" => {
            if args.len() != 1 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 1 argument"),
                ));
            }
            let value = eval_arg(0, env)?;
            match (name, value) {
                ("abs", Value::Int(value)) => value
                    .checked_abs()
                    .map(Value::Int)
                    .ok_or_else(|| err(RuntimeErrorKind::Overflow, "absolute value overflow")),
                ("abs", Value::Decimal(value)) => Ok(Value::Decimal(value.abs())),
                ("sqrt", Value::Int(value)) if value >= 0 => {
                    Ok(Value::Int((value as f64).sqrt().floor() as i64))
                }
                ("sqrt", Value::Decimal(value)) if value >= Decimal::ZERO => {
                    decimal_sqrt(value).map(Value::Decimal)
                }
                ("sqrt", _) => Err(err(
                    RuntimeErrorKind::Type,
                    "sqrt expects a nonnegative number",
                )),
                ("floor", Value::Decimal(value)) => {
                    Ok(Value::Int(value.floor().to_i64().ok_or_else(|| {
                        err(RuntimeErrorKind::Overflow, "floor overflow")
                    })?))
                }
                ("ceil", Value::Decimal(value)) => {
                    Ok(Value::Int(value.ceil().to_i64().ok_or_else(|| {
                        err(RuntimeErrorKind::Overflow, "ceil overflow")
                    })?))
                }
                ("round", Value::Decimal(value)) => {
                    Ok(Value::Int(value.round().to_i64().ok_or_else(|| {
                        err(RuntimeErrorKind::Overflow, "round overflow")
                    })?))
                }
                ("floor" | "ceil" | "round", Value::Int(value)) => Ok(Value::Int(value)),
                _ => Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects a number"),
                )),
            }
        }
        "min" | "max" => {
            if args.is_empty() {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects values"),
                ));
            }
            let values = (0..args.len())
                .map(|index| eval_arg(index, env))
                .collect::<Result<Vec<_>, _>>()?;
            let compare = |a: &Value, b: &Value| -> Result<std::cmp::Ordering, RuntimeError> {
                match (a, b) {
                    (Value::Int(a), Value::Int(b)) => Ok(a.cmp(b)),
                    (Value::Decimal(a), Value::Decimal(b)) => Ok(a.cmp(b)),
                    (Value::Int(a), Value::Decimal(b)) => Ok(Decimal::from(*a).cmp(b)),
                    (Value::Decimal(a), Value::Int(b)) => Ok(a.cmp(&Decimal::from(*b))),
                    (Value::Str(a), Value::Str(b)) => Ok(a.cmp(b)),
                    _ => Err(err(
                        RuntimeErrorKind::Type,
                        format!("{name} values must share a comparable type"),
                    )),
                }
            };
            let selected = values.into_iter().try_fold(
                None,
                |selected: Option<Value>, value| match selected {
                    None => Ok(Some(value)),
                    Some(current) => {
                        let ordering = compare(&value, &current)?;
                        let replace = if name == "min" {
                            ordering.is_lt()
                        } else {
                            ordering.is_gt()
                        };
                        Ok(Some(if replace { value } else { current }))
                    }
                },
            )?;
            selected.ok_or_else(|| err(RuntimeErrorKind::Type, "no values to compare"))
        }
        _ => unreachable!("builtin routed to the wrong category"),
    }
}
