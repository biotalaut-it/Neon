use super::{
    arithmetic::apply_binary,
    state::{err, tick, State},
};
use crate::{
    builtins::call,
    syntax::{check, EvalContext, Expr, Limits, Op, Program, RuntimeError, RuntimeErrorKind},
    value::{Closure, Env, Stream, Value},
};
use rust_decimal::Decimal;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, time::Instant};

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
pub(crate) fn eval_expr(
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
pub(crate) fn invoke(
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

pub(crate) fn collection_values(value: Value) -> Result<Vec<Value>, RuntimeError> {
    match value {
        Value::List(values) | Value::Tuple(values) => Ok(values),
        Value::Stream(stream) => stream.collect_limited(usize::MAX),
        _ => Err(err(RuntimeErrorKind::Type, "expected a collection")),
    }
}

pub(crate) fn make_stream<I: Iterator<Item = Result<Value, RuntimeError>> + 'static>(
    iter: I,
    state: &Rc<RefCell<State>>,
    limits: Limits,
) -> Value {
    Value::Stream(Stream::new(iter, state.clone(), limits.max_output_bytes))
}

pub(crate) fn decimal_sqrt(value: Decimal) -> Result<Decimal, RuntimeError> {
    if value.is_zero() {
        return Ok(Decimal::ZERO);
    }
    let mut estimate = if value > Decimal::ONE {
        value
    } else {
        Decimal::ONE
    };
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
