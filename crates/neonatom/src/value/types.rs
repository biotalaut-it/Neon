use super::Stream;
use crate::syntax::Expr;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::{BTreeMap, HashMap};

#[derive(Clone)]
pub enum Value {
    Int(i64),
    Decimal(Decimal),
    Bool(bool),
    Str(String),
    List(Vec<Value>),
    Tuple(Vec<Value>),
    Map(BTreeMap<String, Value>),
    Record(BTreeMap<String, Value>),
    Option(Option<Box<Value>>),
    Result(Result<Box<Value>, Box<Value>>),
    Date(NaiveDate),
    Duration(i64),
    Quantity(Decimal, String),
    Stream(Stream),
    Function(Closure),
}
#[derive(Clone)]
pub struct Closure {
    pub(crate) param: String,
    pub(crate) body: Expr,
    pub(crate) env: Env,
}
pub(crate) type Env = HashMap<String, Value>;
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Decimal(a), Self::Decimal(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::List(a), Self::List(b)) | (Self::Tuple(a), Self::Tuple(b)) => a == b,
            (Self::Map(a), Self::Map(b)) | (Self::Record(a), Self::Record(b)) => a == b,
            (Self::Option(a), Self::Option(b)) => a == b,
            (Self::Result(a), Self::Result(b)) => a == b,
            (Self::Date(a), Self::Date(b)) => a == b,
            (Self::Duration(a), Self::Duration(b)) => a == b,
            (Self::Quantity(a, au), Self::Quantity(b, bu)) => a == b && au == bu,
            _ => false,
        }
    }
}
