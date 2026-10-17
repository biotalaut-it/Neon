use super::Value;
use std::fmt;

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "Int({n})"),
            Self::Decimal(n) => write!(f, "Decimal({n})"),
            Self::Bool(b) => write!(f, "Bool({b})"),
            Self::Str(s) => write!(f, "Str({s:?})"),
            Self::List(items) => f.debug_list().entries(items).finish(),
            Self::Tuple(items) => write!(f, "Tuple({items:?})"),
            Self::Map(fields) => write!(f, "Map({fields:?})"),
            Self::Record(fields) => write!(f, "Record({fields:?})"),
            Self::Option(value) => write!(f, "Option({value:?})"),
            Self::Result(value) => write!(f, "Result({value:?})"),
            Self::Date(value) => write!(f, "Date({value})"),
            Self::Duration(value) => write!(f, "Duration({value})"),
            Self::Quantity(value, unit) => write!(f, "Quantity({value}, {unit})"),
            Self::Stream(s) => fmt::Debug::fmt(s, f),
            Self::Function(_) => f.write_str("<function>"),
        }
    }
}
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Decimal(n) => write!(f, "{}", n.normalize()),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Str(s) => f.write_str(s),
            Self::List(items) => write_sequence(f, "[", "]", items),
            Self::Tuple(items) => write_sequence(f, "(", ")", items),
            Self::Map(fields) | Self::Record(fields) => {
                f.write_str("{")?;
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index != 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{key}: {value}")?;
                }
                f.write_str("}")
            }
            Self::Option(Some(value)) => write!(f, "some({value})"),
            Self::Option(None) => f.write_str("none"),
            Self::Result(Ok(value)) => write!(f, "ok({value})"),
            Self::Result(Err(value)) => write!(f, "err({value})"),
            Self::Date(value) => write!(f, "{}", value.format("%Y-%m-%d")),
            Self::Duration(value) => write!(f, "{value}s"),
            Self::Quantity(value, unit) => write!(f, "{value} {unit}"),
            Self::Stream(_) => f.write_str("<lazy-stream>"),
            Self::Function(_) => f.write_str("<function>"),
        }
    }
}
fn write_sequence(
    f: &mut fmt::Formatter<'_>,
    open: &str,
    close: &str,
    items: &[Value],
) -> fmt::Result {
    f.write_str(open)?;
    for (index, item) in items.iter().enumerate() {
        if index != 0 {
            f.write_str(", ")?;
        }
        write!(f, "{item}")?;
    }
    f.write_str(close)
}
