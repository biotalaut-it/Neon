use super::{Diagnostic, DiagnosticKind, Expr, Op, Program, Span, StaticType};
use std::collections::HashMap;

pub fn check(program: &Program) -> Result<StaticType, Diagnostic> {
    let mut names = HashMap::new();
    let mut result = StaticType::Any;
    for expression in &program.expressions {
        result = check_expr(expression, &mut names)?;
    }
    Ok(result)
}

fn type_diagnostic(message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        kind: DiagnosticKind::Type,
        message: message.into(),
        span: Span { start: 0, end: 0 },
    }
}

fn compatible(expected: &StaticType, actual: &StaticType) -> bool {
    expected == actual || *expected == StaticType::Any || *actual == StaticType::Any
}

fn check_expr(
    expression: &Expr,
    names: &mut HashMap<String, StaticType>,
) -> Result<StaticType, Diagnostic> {
    match expression {
        Expr::Int(_) => Ok(StaticType::Int),
        Expr::Decimal(_) => Ok(StaticType::Decimal),
        Expr::Bool(_) => Ok(StaticType::Bool),
        Expr::Str(_) => Ok(StaticType::Str),
        Expr::Var(name) => names
            .get(name)
            .cloned()
            .ok_or_else(|| type_diagnostic(format!("unknown name: {name}"))),
        Expr::Let(name, value) => {
            let ty = check_expr(value, names)?;
            names.insert(name.clone(), ty.clone());
            Ok(ty)
        }
        Expr::Lambda(parameter, body) => {
            let previous = names.insert(parameter.clone(), StaticType::Any);
            let result = check_expr(body, names);
            if let Some(previous) = previous {
                names.insert(parameter.clone(), previous);
            } else {
                names.remove(parameter);
            }
            result?;
            Ok(StaticType::Function)
        }
        Expr::List(items) => {
            for item in items {
                check_expr(item, names)?;
            }
            Ok(StaticType::List)
        }
        Expr::Record(fields) => {
            for (_, value) in fields {
                check_expr(value, names)?;
            }
            Ok(StaticType::Record)
        }
        Expr::Binary(left, operator, right) => {
            let left_type = check_expr(left, names)?;
            let right_type = check_expr(right, names)?;
            use Op::*;
            match operator {
                Add | Sub | Mul | Div | Rem => {
                    let numeric = |ty: &StaticType| {
                        matches!(ty, StaticType::Int | StaticType::Decimal | StaticType::Any)
                    };
                    if !numeric(&left_type) || !numeric(&right_type) {
                        return Err(type_diagnostic(
                            "arithmetic operators require numeric values",
                        ));
                    }
                    if left_type == StaticType::Decimal || right_type == StaticType::Decimal {
                        Ok(StaticType::Decimal)
                    } else {
                        Ok(StaticType::Int)
                    }
                }
                And | Or => {
                    if !compatible(&StaticType::Bool, &left_type)
                        || !compatible(&StaticType::Bool, &right_type)
                    {
                        return Err(type_diagnostic("logical operators require booleans"));
                    }
                    Ok(StaticType::Bool)
                }
                Eq | Ne => Ok(StaticType::Bool),
                Lt | Le | Gt | Ge => {
                    if !compatible(&left_type, &right_type)
                        && !(matches!(left_type, StaticType::Int | StaticType::Decimal)
                            && matches!(right_type, StaticType::Int | StaticType::Decimal))
                    {
                        return Err(type_diagnostic(
                            "comparison operands have incompatible types",
                        ));
                    }
                    Ok(StaticType::Bool)
                }
            }
        }
        Expr::Call(name, arguments) => {
            let argument_types = arguments
                .iter()
                .map(|argument| check_expr(argument, names))
                .collect::<Result<Vec<_>, _>>()?;
            check_call_type(name, &argument_types, names)
        }
    }
}

fn check_call_type(
    name: &str,
    arguments: &[StaticType],
    names: &HashMap<String, StaticType>,
) -> Result<StaticType, Diagnostic> {
    let count = arguments.len();
    let arity = |expected: usize| {
        if count == expected {
            Ok(())
        } else {
            Err(type_diagnostic(format!(
                "{name} expects {expected} arguments"
            )))
        }
    };
    let expect = |index: usize, ty: StaticType| {
        if compatible(&ty, &arguments[index]) {
            Ok(())
        } else {
            Err(type_diagnostic(format!(
                "argument {} to {name} must be {ty:?}",
                index + 1
            )))
        }
    };
    match name {
        "range" => {
            arity(2)?;
            expect(0, StaticType::Int)?;
            expect(1, StaticType::Int)?;
            Ok(StaticType::Stream)
        }
        "take" => {
            arity(2)?;
            expect(0, StaticType::Stream)?;
            expect(1, StaticType::Int)?;
            Ok(StaticType::Stream)
        }
        "filter" | "take_while" => {
            arity(2)?;
            expect(0, StaticType::Stream)?;
            expect(1, StaticType::Function)?;
            Ok(StaticType::Stream)
        }
        "map" if count == 2 && arguments[0] == StaticType::Stream => {
            expect(1, StaticType::Function)?;
            Ok(StaticType::Stream)
        }
        "sum" | "count" | "checksum" | "sort" | "unique" | "chunks" | "enumerate" | "first"
        | "last" | "reverse" | "to_list" => {
            let expected = if matches!(name, "chunks") { 2 } else { 1 };
            arity(expected)?;
            if arguments[0] != StaticType::Stream
                && arguments[0] != StaticType::List
                && arguments[0] != StaticType::Tuple
                && arguments[0] != StaticType::Any
            {
                return Err(type_diagnostic(format!("{name} expects a collection")));
            }
            if name == "chunks" {
                expect(1, StaticType::Int)?;
            }
            Ok(if matches!(name, "sum" | "count" | "checksum") {
                StaticType::Int
            } else {
                StaticType::List
            })
        }
        "print" | "assert" | "json_stringify" | "parse_date" | "seconds" | "bytes"
        | "kilobytes" | "megabytes" | "abs" | "sqrt" | "floor" | "ceil" | "round" | "some"
        | "ok" | "err" | "is_some" | "is_ok" | "unwrap" | "len" | "text_len" | "to_string" => {
            arity(1)?;
            Ok(match name {
                "assert" => StaticType::Bool,
                "json_stringify" | "parse_date" | "to_string" => StaticType::Str,
                "some" | "unwrap" => StaticType::Option,
                "ok" | "err" => StaticType::Result,
                "is_some" | "is_ok" => StaticType::Bool,
                "len" | "text_len" => StaticType::Int,
                "seconds" => StaticType::Duration,
                "bytes" | "kilobytes" | "megabytes" => StaticType::Quantity,
                "sqrt" | "abs" | "floor" | "ceil" | "round" => StaticType::Any,
                _ => argument_types_for_result(name, arguments),
            })
        }
        "concat" | "min" | "max" | "list" | "tuple" | "record" | "json_parse" | "csv" | "tsv"
        | "module" | "now" | "date_add_days" | "date_diff_days" | "milliseconds" | "hours"
        | "days" | "convert" | "fold" | "reduce" | "zip" | "group_by" | "get" | "text_slice"
        | "text_find" | "replace" | "contains" | "match" | "unwrap_or" | "query_csv"
        | "zip_with" | "assert_eq" => Ok(match name {
            "list" => StaticType::List,
            "tuple" => StaticType::Tuple,
            "map" => StaticType::Map,
            "record" => StaticType::Record,
            "some" | "unwrap_or" => StaticType::Option,
            "ok" | "err" => StaticType::Result,
            "csv" | "tsv" | "zip" | "take_while" => StaticType::Stream,
            "date_diff_days" | "text_find" | "len" | "text_len" => StaticType::Int,
            "now" | "parse_date" => StaticType::Date,
            "seconds" | "milliseconds" | "hours" | "days" => StaticType::Duration,
            "bytes" | "kilobytes" | "megabytes" | "convert" => StaticType::Quantity,
            "contains" | "is_some" | "is_ok" | "assert" | "assert_eq" => StaticType::Bool,
            "text_slice" | "replace" | "concat" | "json_stringify" | "to_string" => StaticType::Str,
            "fold" | "reduce" => StaticType::Any,
            _ => StaticType::List,
        }),
        _ if names.get(name) == Some(&StaticType::Function) => Ok(StaticType::Any),
        _ => Err(type_diagnostic(format!("unknown function: {name}"))),
    }
}

fn argument_types_for_result(_name: &str, arguments: &[StaticType]) -> StaticType {
    arguments.first().cloned().unwrap_or(StaticType::Any)
}
