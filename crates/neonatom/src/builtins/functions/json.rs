use super::super::json::{json_to_value, value_to_json};
use crate::{
    runtime::{err, eval_expr, State},
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
                return Err(err(
                    RuntimeErrorKind::Type,
                    "json_stringify expects 1 argument",
                ));
            }
            let value = eval_arg(0, env)?;
            let json = value_to_json(value)?;
            serde_json::to_string(&json)
                .map(Value::Str)
                .map_err(|error| err(RuntimeErrorKind::Json, error.to_string()))
        }
        _ => unreachable!("builtin routed to the wrong category"),
    }
}
