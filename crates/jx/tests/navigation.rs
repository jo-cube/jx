use serde_json::Value;

#[test]
fn navigation_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/navigation.json")).unwrap();
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
fn validation_borrowing_and_cancellation() {
    for source in ["*", "**", "**.id", "a[]", "a^(id)", "a{\"x\":id}", "[1..5]"] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"a":1,"bad":[0,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
    let input = br#"{"a":{"id":123},"b":{"id":456}}"#;
    let expression = jx::compile("**.id").unwrap();
    let mut values = Vec::new();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| values.push(value.as_raw().unwrap()))
        .unwrap();
    assert_eq!(values[0].as_bytes().as_ptr(), input[11..].as_ptr());
    assert_eq!(values[1].as_bytes().as_ptr(), input[26..].as_ptr());
    let mut seen = 0;
    assert_eq!(
        expression.evaluate(input).unwrap().try_for_each(|_| {
            seen += 1;
            Err("done")
        }),
        Err(jx::ConsumeError::Consumer("done"))
    );
    assert_eq!(seen, 1);
    let levels = jx::MAX_DEPTH - 1;
    let input = format!("{}7{}", "[".repeat(levels), "]".repeat(levels));
    jx::compile("**")
        .unwrap()
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|v| assert_eq!(v.as_raw().unwrap().as_str(), "7"))
        .unwrap();
}

#[test]
fn range_limits_and_deferred_tuple_navigation_are_explicit() {
    for expr in ["[0..10000000]", "[9007199254740992..9007199254740992]"] {
        assert_eq!(
            jx::compile(expr)
                .unwrap()
                .evaluate(b"null")
                .unwrap_err()
                .kind,
            jx::ErrorKind::NumericRange
        );
    }
    for expr in [
        "1..3",
        "a[1..3]",
        "a.%",
        "a@$x",
        "a#$i",
        "a{\"x\":1}{\"y\":2}",
    ] {
        assert_eq!(
            jx::compile(expr).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
}

#[test]
fn nested_fallbacks_compile_without_expanding_the_left_tree() {
    for operator in ["??", "?:"] {
        let mut source = "missing".to_owned();
        for _ in 0..24 {
            source = format!("({source} {operator} missing)");
        }
        jx::compile(&source)
            .unwrap()
            .evaluate(b"{}")
            .unwrap()
            .for_each(|_| panic!("all branches are missing"))
            .unwrap();
    }
}
