use serde_json::Value;

#[test]
fn aggregate_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/aggregates.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let source = case["expr"].as_str().unwrap();
        let expression = jx::compile(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        let input = serde_json::to_vec(&case["data"]).unwrap();
        let mut items = Vec::<Value>::new();
        let result = expression.evaluate(&input).and_then(|result| {
            result.for_each(|value| {
                let mut bytes = Vec::new();
                value.write_compact(&mut bytes).unwrap();
                items.push(serde_json::from_slice(&bytes).unwrap());
            })
        });
        if let Some(kind) = case.get("error") {
            assert_eq!(
                format!("{:?}", result.unwrap_err().kind),
                kind.as_str().unwrap(),
                "{case}"
            );
        } else {
            result.unwrap_or_else(|error| panic!("{case}: {error}"));
            assert_eq!(Value::Array(items), case["items"], "{case}");
        }
    }
}

#[test]
fn aggregate_arguments_finish_before_type_or_arity_checks() {
    let input = br#"{"a":[{"x":true,"v":null},{"x":1e999}]}"#;
    for source in [
        "$sum(a[x].v)",
        "$min(a[x].v)",
        "$count(a[x])",
        "$sum(null, a[x])",
    ] {
        let error = jx::compile(source).unwrap().evaluate(input).unwrap_err();
        assert_eq!(error.kind, jx::ErrorKind::NumericRange, "{source}");
    }
    let error = jx::compile("1 + $sum(null)")
        .unwrap()
        .evaluate(b"{}")
        .unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::TypeError);
    assert_eq!(error.offset, 4);
}

#[test]
fn aggregates_preserve_ieee_values_and_signed_zero() {
    for (source, input, expected) in [
        ("$min(a)", &b"{\"a\":[0,-0]}"[..], -0.0_f64),
        ("$max(a)", b"{\"a\":[-0,0]}", 0.0),
        ("$sum(a)", b"{\"a\":[-0]}", 0.0),
        ("$sum(a)", b"{\"a\":[1e999]}", f64::INFINITY),
        ("$count(a)", b"{\"a\":[1e999]}", 1.0),
        ("$min(0/0)", b"{}", f64::NAN),
        ("$max(a.(1/$))", b"{\"a\":[0,1]}", f64::INFINITY),
        ("$min(a.(0/$))", b"{\"a\":[0,1]}", f64::NAN),
        ("$max(a.(0/$))", b"{\"a\":[1,0]}", f64::NAN),
    ] {
        let mut count = 0;
        jx::compile(source)
            .unwrap()
            .evaluate(input)
            .unwrap()
            .for_each(|value| {
                let jx::Value::Number(actual) = value else {
                    panic!("{source}: {value:?}")
                };
                if expected.is_nan() {
                    assert!(actual.is_nan(), "{source}");
                } else {
                    assert_eq!(actual.to_bits(), expected.to_bits(), "{source}");
                }
                count += 1;
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}

#[test]
fn validation_cancellation_and_mapped_aggregate_errors() {
    let source = jx::compile("a.$sum(b)").unwrap();
    let input = br#"{"a":[{"b":[1,2]},{"b":null}]}"#;
    let mut count = 0;
    let result = source.evaluate(input).unwrap().for_each(|value| {
        assert!(matches!(value, jx::Value::Number(3.0)));
        count += 1;
    });
    assert_eq!(count, 1);
    assert_eq!(result.unwrap_err().kind, jx::ErrorKind::TypeError);
    assert_eq!(
        source
            .evaluate(input)
            .unwrap()
            .try_for_each(|_| Err("stop")),
        Err(jx::ConsumeError::Consumer("stop"))
    );
    assert_eq!(
        source
            .evaluate(br#"{"a":[{"b":[1,2]}],"bad":[0,]}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::InvalidJson
    );
    // A fold cannot emit a partial result, even if earlier candidates were valid.
    assert_eq!(
        jx::compile("$sum(a.b)")
            .unwrap()
            .evaluate(input)
            .unwrap_err()
            .kind,
        jx::ErrorKind::TypeError
    );
    let selected = jx::compile("a[$sum(b) > 2].id").unwrap();
    let input = br#"{"a":[{"id":"kept","b":[1,2]}]}"#;
    selected
        .evaluate(input)
        .unwrap()
        .for_each(|value| {
            let raw = value.as_raw().unwrap();
            assert_eq!(raw.as_bytes(), b"\"kept\"");
            assert_eq!(raw.as_bytes().as_ptr(), input[12..].as_ptr());
        })
        .unwrap();
}

#[test]
fn aggregate_calls_keep_syntax_and_depth_limits_explicit() {
    for source in ["$sum(1,)", "$sum(,1)", "$sum(1"] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression,
            "{source}"
        );
    }
    let nested = format!("{}1{}", "$sum(".repeat(200), ")".repeat(200));
    assert_eq!(
        jx::compile(&nested).unwrap_err().kind,
        jx::ErrorKind::DepthLimit
    );
    let nested = format!("{}1{}", "$sum(".repeat(64), ")".repeat(64));
    jx::compile(&nested)
        .unwrap()
        .evaluate(b"null")
        .unwrap()
        .for_each(|value| {
            assert!(matches!(value, jx::Value::Number(1.0)));
        })
        .unwrap();
}
