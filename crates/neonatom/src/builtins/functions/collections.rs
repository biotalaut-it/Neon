use crate::{
    runtime::{collection_values, err, eval_expr, int, invoke, make_stream, tick, State},
    syntax::{EvalContext, Expr, Limits, RuntimeError, RuntimeErrorKind},
    value::{Env, Stream, Value},
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

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
                if !entries.len().is_multiple_of(2) {
                    return Err(err(RuntimeErrorKind::Type, "map expects key/value pairs"));
                }
                let mut map = BTreeMap::new();
                for pair in entries.as_chunks::<2>().0 {
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
            let context2 = context.clone();
            let limits2 = limits;
            let mut src = stream;
            let is_map = name == "map";
            let is_take_while = name == "take_while";
            let iter = std::iter::from_fn(move || loop {
                let input = match src.next()? {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
                match invoke(function.clone(), input.clone(), &state2, limits2, &context2) {
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
                let first = values.next().ok_or_else(|| {
                    err(
                        RuntimeErrorKind::Type,
                        "reduce cannot use an empty collection",
                    )
                })?;
                (first, values.collect(), function)
            } else {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("invalid {name} arguments"),
                ));
            };
            for value in values {
                accumulator = invoke(
                    function.clone(),
                    Value::Tuple(vec![accumulator, value]),
                    state,
                    limits,
                    context,
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
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 1 argument"),
                ));
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
            if !args.len().is_multiple_of(2) {
                return Err(err(
                    RuntimeErrorKind::Type,
                    "record expects text/value pairs",
                ));
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
                | (Value::Record(fields), Value::Str(key)) => {
                    Ok(Value::Option(fields.get(&key).cloned().map(Box::new)))
                }
                (Value::List(items), Value::Int(index))
                | (Value::Tuple(items), Value::Int(index))
                    if index >= 0 =>
                {
                    Ok(Value::Option(
                        items.get(index as usize).cloned().map(Box::new),
                    ))
                }
                _ => Err(err(
                    RuntimeErrorKind::Type,
                    "get expects a map/record and text key, or list and integer index",
                )),
            }
        }
        _ => unreachable!("builtin routed to the wrong category"),
    }
}
