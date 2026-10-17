mod collections;
mod date_units;
mod json;
mod numeric;
mod option_result;
mod text;

use crate::{
    runtime::{err, eval_expr, invoke, State},
    syntax::{EvalContext, Expr, Limits, RuntimeError, RuntimeErrorKind},
    value::{Env, Value},
};
use std::{cell::RefCell, rc::Rc};

pub(crate) fn call(
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
            return Err(err(
                RuntimeErrorKind::Type,
                format!("{name} expects 1 argument"),
            ));
        }
        let argument = eval_expr(&args[0], env, state, limits, context, depth + 1)?;
        return invoke(function, argument, state, limits, context);
    }
    match name {
        "range" | "take" | "map" | "filter" | "take_while" | "sum" | "count" | "checksum"
        | "print" | "fold" | "reduce" | "enumerate" | "zip" | "chunks" | "sort" | "unique"
        | "reverse" | "to_list" | "list" | "tuple" | "record" | "get" => {
            collections::call(name, args, env, state, limits, context, depth)
        }
        "concat" | "text_slice" | "text_find" | "replace" | "contains" | "len" | "text_len"
        | "to_string" => text::call(name, args, env, state, limits, context, depth),
        "none" | "some" | "ok" | "err" | "is_some" | "is_ok" | "unwrap" | "unwrap_or" | "match"
        | "assert" | "assert_eq" => {
            option_result::call(name, args, env, state, limits, context, depth)
        }
        "json_parse" | "json_stringify" => {
            json::call(name, args, env, state, limits, context, depth)
        }
        "abs" | "sqrt" | "floor" | "ceil" | "round" | "min" | "max" => {
            numeric::call(name, args, env, state, limits, context, depth)
        }
        "parse_date" | "now" | "date_add_days" | "date_diff_days" | "seconds" | "milliseconds"
        | "hours" | "days" | "bytes" | "kilobytes" | "megabytes" | "convert" => {
            date_units::call(name, args, env, state, limits, context, depth)
        }
        _ => Err(err(
            RuntimeErrorKind::UnknownName,
            format!("unknown function: {name}"),
        )),
    }
}
