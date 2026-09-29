use serde_json::Value;

#[test]
fn scoped_path_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/tuples.json")).unwrap();
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
fn scoped_paths_validate_before_output_and_borrow_selected_values() {
    for source in ["a#$i.v", "a@$x.b[$=$x]", "a#$i{\"indices\":$i}"] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"a":[],"bad":[0,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
    let input = br#"{"a":[{"v":123},{"v":456}]}"#;
    let expression = jx::compile("a#$i.v").unwrap();
    let mut values = Vec::new();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|v| values.push(v.as_raw().unwrap()))
        .unwrap();
    assert_eq!(values[0].as_bytes().as_ptr(), input[11..].as_ptr());
    assert_eq!(values[1].as_bytes().as_ptr(), input[21..].as_ptr());
}

#[test]
fn cancellation_stops_scoped_stages_before_later_errors() {
    let expression = jx::compile("a#$i.($i=0 ? $ : 1+null)").unwrap();
    let input = br#"{"a":[10,20]}"#;
    let mut seen = 0;
    assert_eq!(
        expression.evaluate(input).unwrap().try_for_each(|value| {
            seen += 1;
            assert_eq!(value.as_raw().unwrap().as_str(), "10");
            Err("stop")
        }),
        Err(jx::ConsumeError::Consumer("stop"))
    );
    assert_eq!(seen, 1);
    assert_eq!(
        expression
            .evaluate(input)
            .unwrap()
            .for_each(|_| {})
            .unwrap_err()
            .kind,
        jx::ErrorKind::TypeError
    );
}

#[test]
fn invalid_bindings_and_parent_navigation_remain_explicit() {
    for expression in [
        "a@name",
        "a#name",
        "a[0]@$x",
        "a^(id)@$x",
        "a.%",
        "a#$$",
        r#"a#$i^($i){"positions":$i}"#,
        r#"a#$i^($i)[true]{"positions":$i}"#,
        "a#$i^(v)[v>1].$i",
        "a#$i^(v)^(>$i).$i",
    ] {
        assert_eq!(
            jx::compile(expression).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression,
            "{expression}"
        );
    }
}

#[test]
fn an_empty_scoped_group_constructs_an_empty_object() {
    // Upstream 2.2.0 throws an uncoded JavaScript exception here. Use the same
    // empty grouping rule as ordinary sequences, without emulating a host crash.
    let expression = jx::compile("a#$i{\"indices\":$i}").unwrap();
    let mut output = Vec::new();
    expression
        .evaluate(br#"{"a":[]}"#)
        .unwrap()
        .for_each(|value| value.write_compact(&mut output).unwrap())
        .unwrap();
    assert_eq!(output, b"{}");
}
