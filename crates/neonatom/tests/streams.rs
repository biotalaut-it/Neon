use neonatom::{
    runtime::eval,
    syntax::{parse, Limits, RuntimeErrorKind},
    value::Value,
};
use std::time::Duration;

fn limits() -> Limits {
    Limits {
        fuel: 20_000,
        max_nesting: 128,
        max_output_bytes: 1024,
        timeout: Duration::from_secs(2),
    }
}

#[test]
fn map_and_filter_stay_lazy_and_produce_expected_values() {
    let mapped = eval(&parse("map(range(1, 5), x => x * 2)").unwrap(), limits()).unwrap();
    let Value::Stream(mapped) = mapped else {
        panic!("expected stream");
    };
    assert_eq!(
        mapped.collect_limited(10).unwrap(),
        vec![Value::Int(2), Value::Int(4), Value::Int(6), Value::Int(8)]
    );

    let filtered = eval(
        &parse("filter(range(1, 7), x => x % 2 == 0)").unwrap(),
        limits(),
    )
    .unwrap();
    let Value::Stream(filtered) = filtered else {
        panic!("expected stream");
    };
    assert_eq!(
        filtered.collect_limited(10).unwrap(),
        vec![Value::Int(2), Value::Int(4), Value::Int(6)]
    );
}

#[test]
fn unbounded_work_stops_when_fuel_is_exhausted() {
    let program = parse("sum(range(0, 1000000))").unwrap();
    let limited = Limits {
        fuel: 100,
        ..limits()
    };
    let error = eval(&program, limited).unwrap_err();
    assert_eq!(error.kind, RuntimeErrorKind::FuelExhausted);
}

#[test]
fn collection_obeys_byte_limit() {
    let program = parse("take(range(100, 105), 5)").unwrap();
    let value = eval(
        &program,
        Limits {
            max_output_bytes: 2,
            ..limits()
        },
    )
    .unwrap();
    let Value::Stream(stream) = value else {
        panic!("expected stream");
    };
    assert_eq!(
        stream.collect_limited(5).unwrap_err().kind,
        RuntimeErrorKind::OutputLimit
    );
}
