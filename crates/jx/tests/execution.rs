use jx::ErrorKind;
use serde_json::{Value, json};

const ARITHMETIC: &str = "((x+y)*(x-y)+(x*x+y*y))/(x+1)-y*3";
fn evaluate(source: &str, input: &[u8]) -> Result<Vec<Value>, jx::Error> {
    let expression = jx::compile(source).unwrap();
    let mut values = Vec::new();
    expression.evaluate(input)?.for_each(|value| {
        let mut bytes = Vec::new();
        value.write_compact(&mut bytes).unwrap();
        values.push(serde_json::from_slice(&bytes).unwrap());
    })?;
    Ok(values)
}
#[test]
fn numeric_regions_preserve_missing_shape_and_record_isolation() {
    for (input, expected) in [
        (r#"{"x":7,"y":3}"#, vec![json!(3.25)]),
        (r#"[{"x":7,"y":3}]"#, vec![json!(3.25)]),
        (r#"{"y":3}"#, vec![]),
        (r#"{"x":1,"\u0078":7,"y":3}"#, vec![json!(3.25)]),
    ] {
        assert_eq!(evaluate(ARITHMETIC, input.as_bytes()).unwrap(), expected);
    }
    let expression = jx::compile(ARITHMETIC).unwrap();
    for (input, expected) in [
        (br#"{"x":7,"y":3}"#.as_slice(), 1),
        (br#"{"y":3}"#.as_slice(), 0),
        (br#"{"x":7,"y":3}"#.as_slice(), 1),
    ] {
        let mut count = 0;
        expression
            .evaluate(input)
            .unwrap()
            .for_each(|_| count += 1)
            .unwrap();
        assert_eq!(count, expected);
    }
    for input in [
        r#"{"x":[7],"y":3}"#,
        r#"{"x":null,"y":3}"#,
        r#"[{"x":7,"y":3},{"x":2,"y":3}]"#,
    ] {
        assert_eq!(
            evaluate(ARITHMETIC, input.as_bytes()).unwrap_err().kind,
            ErrorKind::TypeError
        );
    }
}
#[test]
fn numeric_regions_compose_with_contexts_and_retention() {
    let input = br#"{"rows":[{"x":7,"y":3},{"x":2,"y":1}]}"#;
    assert_eq!(
        evaluate(&format!("rows.({ARITHMETIC})"), input).unwrap(),
        vec![json!(3.25), json!(8.0 / 3.0 - 3.0)]
    );
    assert_eq!(
        evaluate("$sum(rows[(x*2+x+1)>12].x)", input).unwrap(),
        vec![json!(7)]
    );
    assert_eq!(
        evaluate(&format!("rows.{{\"n\":{ARITHMETIC}}}"), input).unwrap(),
        vec![json!({"n":3.25}), json!({"n":8.0/3.0-3.0})]
    );
    assert_eq!(
        evaluate(
            &format!("($f:=function(){{{ARITHMETIC}}};$f())"),
            br#"{"x":7,"y":3}"#
        )
        .unwrap(),
        vec![json!(3.25)]
    );
}
#[test]
fn numeric_regions_accept_constructed_and_retained_contexts() {
    for source in [
        "[{\"x\":2},{\"x\":3}].(x*x+x+x)",
        "rows.{\"x\":x}.(x*x+x+x)",
        "($rows:=rows.{\"x\":x};$rows.(x*x+x+x))",
        "($f:=function($rows){$rows.(x*x+x+x)};$f(rows.{\"x\":x}))",
    ] {
        assert_eq!(
            evaluate(source, br#"{"rows":[{"x":2},{"x":3}]}"#).unwrap(),
            vec![json!(8), json!(15)]
        );
    }
}

#[test]
fn numeric_regions_keep_validation_and_error_precedence() {
    let input = br#"{"x":7,"y":3}"#;
    for (source, kind) in [
        ("((x+x)+x)/0 + x", ErrorKind::NumericRange),
        ("((x-x)+(y-y))/0 + x", ErrorKind::TypeError),
        ("missing+missing+missing+missing+null", ErrorKind::TypeError),
    ] {
        assert_eq!(evaluate(source, input).unwrap_err().kind, kind);
    }
    assert_eq!(
        evaluate(ARITHMETIC, br#"{"x":7,"y":3,"bad":[1,]}"#)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidJson
    );
    assert_eq!(
        evaluate(&format!("false ? ({ARITHMETIC}) : 7"), br#"{"x":null}"#).unwrap(),
        vec![json!(7)]
    );
}

#[test]
fn numeric_regions_preserve_ordering_and_stream_cancellation() {
    for (source, input, expected) in [
        ("x+x+x+x", r#"{"x":2}"#, json!(8)),
        ("-(x*x+x)-x", r#"{"x":2}"#, json!(-8)),
        ("(x+x+x)/0", r#"{"x":2}"#, json!(null)),
        ("(x-x+x-x)/0", r#"{"x":2}"#, json!(null)),
        ("(x+x+x) < y", r#"{"x":2,"y":7}"#, json!(true)),
        ("(x+x+x) >= y", r#"{"x":2,"y":7}"#, json!(false)),
        ("(x+x+x)/0 < y", r#"{"x":2,"y":7}"#, json!(false)),
        ("(x-x+x-x)/0 < y", r#"{"x":2,"y":7}"#, json!(false)),
    ] {
        assert_eq!(evaluate(source, input.as_bytes()).unwrap(), vec![expected]);
    }
    let expression = jx::compile("rows.(x*x+x+x)").unwrap();
    let input = br#"{"rows":[{"x":2},{"x":null}]}"#;
    assert_eq!(
        expression.evaluate(input).unwrap().try_for_each(|_| Err(7)),
        Err(jx::ConsumeError::Consumer(7))
    );
    assert_eq!(
        expression
            .evaluate(input)
            .unwrap()
            .for_each(|_| {})
            .unwrap_err()
            .kind,
        ErrorKind::TypeError
    );
    let long = std::iter::repeat_n("x", 60).collect::<Vec<_>>().join("+");
    assert_eq!(evaluate(&long, br#"{"x":2}"#).unwrap(), vec![json!(120)]);
}

#[test]
fn planned_regions_do_not_round_borrowed_output() {
    let input = br#"{"x":9007199254740993,"y":1}"#;
    for source in ["x>y and y>0 ? x : y", "{\"raw\":x,\"computed\":x+y}"] {
        let expression = jx::compile(source).unwrap();
        expression
            .evaluate(input)
            .unwrap()
            .for_each(|value| {
                let raw = match &value {
                    jx::Value::Object(object) => object.as_slice()[0].1.as_raw().unwrap(),
                    value => value.as_raw().unwrap(),
                };
                assert_eq!(raw.as_str(), "9007199254740993");
                assert_eq!(raw.as_bytes().as_ptr(), input[5..].as_ptr());
            })
            .unwrap();
    }
}

#[test]
fn branches_fixed_objects_and_streamed_folds_compose() {
    let input=br#"{"x":7,"y":3,"active":true,"payload":{"orders":[{"price":2,"qty":3},{"price":5,"qty":2}]}}"#;
    for (source, expected) in [
        ("x>y ? x*x+y*y : x-y", vec![json!(58)]),
        ("active and x>y and (x+y<20 or y=0)", vec![json!(true)]),
        (
            r#"{"sum":x+y,"product":x*y,"schema":{"v":1}}"#,
            vec![json!({"sum":10,"product":21,"schema":{"v":1}})],
        ),
        (
            "$sum(payload.orders[price>3].(price*qty+1))",
            vec![json!(11)],
        ),
        ("$min(payload.orders.(price*qty+1))", vec![json!(7)]),
        ("$max(payload.orders.(price*qty+1))", vec![json!(11)]),
        (
            "$count(payload.orders[price>3].(price*qty+1))",
            vec![json!(1)],
        ),
        (
            r#"payload.orders.{"gross":price*qty,"large":price>3}"#,
            vec![
                json!({"gross":6,"large":false}),
                json!({"gross":10,"large":true}),
            ],
        ),
    ] {
        assert_eq!(evaluate(source, input).unwrap(), expected, "{source}");
    }
    for (source, expected) in [
        ("$sum(payload.orders[price>3].(price*qty+1))", vec![]),
        (
            "$count(payload.orders[price>3].(price*qty+1))",
            vec![json!(0)],
        ),
    ] {
        assert_eq!(
            evaluate(source, br#"{"payload":{"orders":[]}}"#).unwrap(),
            expected
        );
    }
    assert_eq!(
        evaluate("x>0 or (y*y+y>0)", br#"{"x":2,"y":null}"#).unwrap(),
        vec![json!(true)]
    );
}
