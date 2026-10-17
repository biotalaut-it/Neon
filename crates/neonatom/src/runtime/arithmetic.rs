use crate::{
    runtime::err,
    syntax::{Op, RuntimeError, RuntimeErrorKind},
    value::Value,
};
use rust_decimal::Decimal;

pub(crate) fn int(v: Value) -> Result<i64, RuntimeError> {
    if let Value::Int(n) = v {
        Ok(n)
    } else {
        Err(err(RuntimeErrorKind::Type, "expected integer"))
    }
}
pub(crate) fn decimal_value(value: Value) -> Result<Decimal, RuntimeError> {
    match value {
        Value::Int(value) => Ok(Decimal::from(value)),
        Value::Decimal(value) => Ok(value),
        _ => Err(err(RuntimeErrorKind::Type, "expected numeric value")),
    }
}
pub(crate) fn apply_binary(a: Value, op: Op, b: Value) -> Result<Value, RuntimeError> {
    use Op::*;
    if op == Eq || op == Ne {
        let equal = a == b;
        return Ok(Value::Bool(if op == Eq { equal } else { !equal }));
    }
    if op == And || op == Or {
        if let (Value::Bool(x), Value::Bool(y)) = (a, b) {
            return Ok(Value::Bool(if op == And { x && y } else { x || y }));
        }
        return Err(err(
            RuntimeErrorKind::Type,
            "logical operators require booleans",
        ));
    }
    if let (Value::Str(left), Value::Str(right)) = (&a, &b) {
        return match op {
            Add => Ok(Value::Str(format!("{left}{right}"))),
            Lt => Ok(Value::Bool(left < right)),
            Le => Ok(Value::Bool(left <= right)),
            Gt => Ok(Value::Bool(left > right)),
            Ge => Ok(Value::Bool(left >= right)),
            _ => Err(err(
                RuntimeErrorKind::Type,
                "operator is not supported for text",
            )),
        };
    }
    if matches!(a, Value::Decimal(_)) || matches!(b, Value::Decimal(_)) {
        let left = decimal_value(a)?;
        let right = decimal_value(b)?;
        if matches!(op, Div | Rem) && right.is_zero() {
            return Err(err(
                RuntimeErrorKind::DivisionByZero,
                "decimal division by zero",
            ));
        }
        return match op {
            Add => left.checked_add(right).map(Value::Decimal),
            Sub => left.checked_sub(right).map(Value::Decimal),
            Mul => left.checked_mul(right).map(Value::Decimal),
            Div => left.checked_div(right).map(Value::Decimal),
            Rem => left.checked_rem(right).map(Value::Decimal),
            Lt => return Ok(Value::Bool(left < right)),
            Le => return Ok(Value::Bool(left <= right)),
            Gt => return Ok(Value::Bool(left > right)),
            Ge => return Ok(Value::Bool(left >= right)),
            _ => None,
        }
        .ok_or_else(|| err(RuntimeErrorKind::Overflow, "fixed-point decimal overflow"));
    }
    let x = int(a)?;
    let y = int(b)?;
    let n = match op {
        Add => x.checked_add(y),
        Sub => x.checked_sub(y),
        Mul => x.checked_mul(y),
        Div => {
            if y == 0 {
                return Err(err(RuntimeErrorKind::DivisionByZero, "division by zero"));
            }
            x.checked_div(y)
        }
        Rem => {
            if y == 0 {
                return Err(err(RuntimeErrorKind::DivisionByZero, "remainder by zero"));
            }
            x.checked_rem(y)
        }
        Lt => return Ok(Value::Bool(x < y)),
        Le => return Ok(Value::Bool(x <= y)),
        Gt => return Ok(Value::Bool(x > y)),
        Ge => return Ok(Value::Bool(x >= y)),
        _ => None,
    };
    n.map(Value::Int)
        .ok_or_else(|| err(RuntimeErrorKind::Overflow, "integer arithmetic overflow"))
}
