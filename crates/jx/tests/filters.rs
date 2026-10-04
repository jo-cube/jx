use serde_json::Value;

#[test]
fn filter_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/filters.json")).unwrap();
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
fn upstream_stage_errors_take_precedence_over_later_stage_errors() {
    let input = br#"{"a":[{"x":true,"y":null,"b":{"y":null}},{"x":1e999}]}"#;
    for source in ["a[x][y+1]", "a[x].b[y+1]", "a[x][y+1][0]"] {
        let expression = jx::compile(source).unwrap();
        let error = expression
            .evaluate(input)
            .unwrap()
            .for_each(|_| {})
            .unwrap_err();
        assert_eq!(error.kind, jx::ErrorKind::NumericRange, "{source}");
    }
}

#[test]
fn filtered_values_borrow_input_and_cancellation_stops_predicates() {
    let input = br#"{"a":[1,null,3]}"#;
    let expression = jx::compile("a[$+1 > 1]").unwrap();
    let mut values = Vec::new();
    let error = expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| values.push(value))
        .unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::TypeError);
    assert_eq!(values.len(), 1);
    assert_eq!(
        values[0].as_raw().unwrap().as_bytes().as_ptr(),
        input[6..].as_ptr()
    );
    let result = expression
        .evaluate(input)
        .unwrap()
        .try_for_each(|_| Err("stop"));
    assert_eq!(result, Err(jx::ConsumeError::Consumer("stop")));
    let invalid = br#"{"a":[1,null],"bad":[0,]}"#;
    assert_eq!(
        expression.evaluate(invalid).unwrap_err().kind,
        jx::ErrorKind::InvalidJson
    );
}

#[test]
fn undefined_sequence_items_remain_distinct_from_null() {
    let input = br#"{"a":[{},{"b":null}]}"#;
    let expression = jx::compile("a.b[true]").unwrap();
    let mut values = Vec::new();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| values.push(value))
        .unwrap();
    assert!(matches!(values[0], jx::Value::Undefined));
    assert_eq!(values[1].as_raw().unwrap().as_str(), "null");
    jx::compile("a.b[true]")
        .unwrap()
        .evaluate(br#"{"a":[{}]}"#)
        .unwrap()
        .for_each(|_| panic!("a sole undefined normalizes to missing"))
        .unwrap();
}

#[test]
fn filters_have_bounded_depth_and_keep_deferred_syntax_explicit() {
    for source in ["a[", "a[true", "a[0,1]", "a[0..2]"] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression,
            "{source}"
        );
    }
    for source in [
        format!("a{}", "[true]".repeat(200)),
        format!("{}true{}", "a[".repeat(200), "]".repeat(200)),
    ] {
        assert_eq!(
            jx::compile(&source).unwrap_err().kind,
            jx::ErrorKind::DepthLimit
        );
    }
    let expression = jx::compile(&format!("a{}", "[-(1+0)]".repeat(100))).unwrap();
    let mut count = 0;
    expression
        .evaluate(br#"{"a":[1,2,3]}"#)
        .unwrap()
        .for_each(|v| {
            assert_eq!(v.as_raw().unwrap().as_str(), "3");
            count += 1;
        })
        .unwrap();
    assert_eq!(count, 1);
}
