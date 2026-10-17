use crate::{
    runtime::err,
    syntax::{RuntimeError, RuntimeErrorKind},
    value::Value,
};
use rust_decimal::Decimal;
use std::{collections::BTreeMap, str::FromStr};

pub(super) fn json_to_value(value: serde_json::Value) -> Result<Value, RuntimeError> {
    Ok(match value {
        serde_json::Value::Null => Value::Option(None),
        serde_json::Value::Bool(value) => Value::Bool(value),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Value::Int(value)
            } else {
                let decimal = Decimal::from_str_exact(&number.to_string())
                    .map_err(|error| err(RuntimeErrorKind::Json, error.to_string()))?;
                Value::Decimal(decimal)
            }
        }
        serde_json::Value::String(value) => Value::Str(value),
        serde_json::Value::Array(values) => Value::List(
            values
                .into_iter()
                .map(json_to_value)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        serde_json::Value::Object(fields) => Value::Record(
            fields
                .into_iter()
                .map(|(key, value)| Ok((key, json_to_value(value)?)))
                .collect::<Result<BTreeMap<_, _>, RuntimeError>>()?,
        ),
    })
}

pub(super) fn value_to_json(value: Value) -> Result<serde_json::Value, RuntimeError> {
    use serde_json::{Map, Number, Value as Json};
    Ok(match value {
        Value::Int(value) => Json::Number(Number::from(value)),
        Value::Decimal(value) => Json::Number(
            Number::from_str(&value.normalize().to_string())
                .map_err(|error| err(RuntimeErrorKind::Json, error.to_string()))?,
        ),
        Value::Bool(value) => Json::Bool(value),
        Value::Str(value) => Json::String(value),
        Value::List(values) | Value::Tuple(values) => Json::Array(
            values
                .into_iter()
                .map(value_to_json)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Map(fields) | Value::Record(fields) => Json::Object(
            fields
                .into_iter()
                .map(|(key, value)| Ok((key, value_to_json(value)?)))
                .collect::<Result<Map<_, _>, RuntimeError>>()?,
        ),
        Value::Option(Some(value)) | Value::Result(Ok(value)) => value_to_json(*value)?,
        Value::Option(None) => Json::Null,
        Value::Result(Err(value)) => {
            let mut object = Map::new();
            object.insert("error".into(), value_to_json(*value)?);
            Json::Object(object)
        }
        Value::Date(value) => Json::String(value.format("%Y-%m-%d").to_string()),
        Value::Duration(value) => Json::Number(Number::from(value)),
        Value::Quantity(value, unit) => {
            let mut object = Map::new();
            object.insert("value".into(), Json::String(value.to_string()));
            object.insert("unit".into(), Json::String(unit));
            Json::Object(object)
        }
        Value::Stream(_) | Value::Function(_) => {
            return Err(err(
                RuntimeErrorKind::Json,
                "streams and functions cannot be serialized as JSON",
            ))
        }
    })
}
