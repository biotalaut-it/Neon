use neonatom::{
    runtime::eval,
    syntax::{parse, DiagnosticKind, Limits, RuntimeErrorKind},
    value::Value,
};
use std::time::Duration;

fn limits() -> Limits {
    Limits {
        fuel: 100_000,
        max_nesting: 128,
        max_output_bytes: 16 * 1024 * 1024,
        timeout: Duration::from_secs(2),
    }
}

#[test]
fn parses_and_evaluates_precedence_and_bindings() {
    let program = parse("let x = 2 + 3 * 4; x - 1").unwrap();
    assert_eq!(eval(&program, limits()).unwrap(), Value::Int(13));
}

#[test]
fn checked_arithmetic_reports_overflow() {
    let program = parse("9223372036854775807 + 1").unwrap();
    assert!(eval(&program, limits())
        .unwrap_err()
        .to_string()
        .contains("overflow"));
}

#[test]
fn lazy_range_take_does_not_materialize_the_full_range() {
    let program = parse("take(range(0, 1000000000000), 4)").unwrap();
    let value = eval(&program, limits()).unwrap();
    let Value::Stream(mut stream) = value else {
        panic!("expected stream");
    };
    let items = stream
        .by_ref()
        .take(5)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        items,
        vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
    );
}

#[test]
fn division_by_zero_is_an_error() {
    let program = parse("1 / 0").unwrap();
    assert!(eval(&program, limits())
        .unwrap_err()
        .to_string()
        .contains("zero"));
}

#[test]
fn malformed_source_reports_a_span() {
    let error = parse("let =").unwrap_err();
    assert!(error.span.end >= error.span.start);
}

#[test]
fn parser_and_evaluator_enforce_nesting_limits() {
    let deeply_nested = format!("{}1{}", "(".repeat(140), ")".repeat(140));
    assert!(parse(&deeply_nested)
        .unwrap_err()
        .message
        .contains("nesting"));

    let program = parse("1 + (2 + (3 + (4 + 5)))").unwrap();
    let error = eval(
        &program,
        Limits {
            max_nesting: 2,
            ..limits()
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, RuntimeErrorKind::NestingLimit);
}

#[test]
fn source_size_and_evaluation_timeout_are_bounded() {
    let too_large = " ".repeat(1024 * 1024 + 1);
    assert_eq!(parse(&too_large).unwrap_err().kind, DiagnosticKind::Limit);

    let program = parse("1 + 2").unwrap();
    let error = eval(
        &program,
        Limits {
            timeout: Duration::ZERO,
            ..limits()
        },
    )
    .unwrap_err();
    assert_eq!(error.kind, RuntimeErrorKind::Timeout);

    let no_overflow = eval(
        &program,
        Limits {
            timeout: Duration::MAX,
            ..limits()
        },
    )
    .unwrap();
    assert_eq!(no_overflow, Value::Int(3));
}
