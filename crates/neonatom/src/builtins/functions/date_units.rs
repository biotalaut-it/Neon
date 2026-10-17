use crate::{
    runtime::{decimal_value, err, eval_expr, int, State},
    syntax::{EvalContext, Expr, Limits, RuntimeError, RuntimeErrorKind},
    value::{Env, Value},
};
use chrono::NaiveDate;
use rust_decimal::Decimal;
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
                return Err(err(
                    RuntimeErrorKind::Type,
                    "date_add_days expects date and integer",
                ));
            }
            let Value::Date(date) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "date_add_days expects a date"));
            };
            let days = int(eval_arg(1, env)?)?;
            date.checked_add_signed(chrono::TimeDelta::days(days))
                .map(Value::Date)
                .ok_or_else(|| err(RuntimeErrorKind::Overflow, "date overflow"))
        }
        "date_diff_days" => {
            if args.len() != 2 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    "date_diff_days expects 2 dates",
                ));
            }
            let Value::Date(start) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "expected a date"));
            };
            let Value::Date(end) = eval_arg(1, env)? else {
                return Err(err(RuntimeErrorKind::Type, "expected a date"));
            };
            Ok(Value::Int(end.signed_duration_since(start).num_days()))
        }
        "seconds" | "milliseconds" | "hours" | "days" => {
            if args.len() != 1 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 1 argument"),
                ));
            }
            let value = int(eval_arg(0, env)?)?;
            let multiplier = match name {
                "seconds" => 1_000,
                "milliseconds" => 1,
                "hours" => 3_600_000,
                _ => 86_400_000,
            };
            value
                .checked_mul(multiplier)
                .map(Value::Duration)
                .ok_or_else(|| err(RuntimeErrorKind::Overflow, "duration overflow"))
        }
        "bytes" | "kilobytes" | "megabytes" => {
            if args.len() != 1 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    format!("{name} expects 1 argument"),
                ));
            }
            let amount = decimal_value(eval_arg(0, env)?)?;
            let multiplier = match name {
                "bytes" => Decimal::ONE,
                "kilobytes" => Decimal::from(1_000),
                _ => Decimal::from(1_000_000),
            };
            Ok(Value::Quantity(
                amount
                    .checked_mul(multiplier)
                    .ok_or_else(|| err(RuntimeErrorKind::Overflow, "quantity overflow"))?,
                "B".into(),
            ))
        }
        "convert" => {
            if args.len() != 2 {
                return Err(err(
                    RuntimeErrorKind::Type,
                    "convert expects quantity and unit",
                ));
            }
            let Value::Quantity(bytes, _) = eval_arg(0, env)? else {
                return Err(err(RuntimeErrorKind::Type, "convert expects a quantity"));
            };
            let Value::Str(unit) = eval_arg(1, env)? else {
                return Err(err(RuntimeErrorKind::Type, "convert expects a unit name"));
            };
            let divisor = match unit.as_str() {
                "B" | "byte" => Decimal::ONE,
                "KB" => Decimal::from(1_000),
                "MB" => Decimal::from(1_000_000),
                "KiB" => Decimal::from(1_024),
                "MiB" => Decimal::from(1_048_576),
                _ => return Err(err(RuntimeErrorKind::InvalidUnit, "unsupported byte unit")),
            };
            Ok(Value::Quantity(
                bytes
                    .checked_div(divisor)
                    .ok_or_else(|| err(RuntimeErrorKind::Overflow, "unit conversion overflow"))?,
                unit,
            ))
        }
        _ => unreachable!("builtin routed to the wrong category"),
    }
}
