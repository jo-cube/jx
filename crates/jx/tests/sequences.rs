use serde_json::Value;

#[test]
fn array_navigation_and_sequence_boundaries() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/sequences.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let expression = jx::compile(case["expr"].as_str().unwrap()).unwrap();
        let input = serde_json::to_vec(&case["data"]).unwrap();
        let mut actual: Vec<Value> = Vec::new();
        expression
            .evaluate(&input)
            .unwrap()
            .for_each(|value| {
                actual.push(serde_json::from_slice(value.as_raw().unwrap().as_bytes()).unwrap());
            })
            .unwrap();
        assert_eq!(Value::Array(actual), case["items"], "{case}");
    }
}

fn items(source: &str, input: &str) -> Vec<String> {
    let mut values = Vec::new();
    jx::compile(source)
        .unwrap()
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|value| {
            values.push(value.as_raw().unwrap().as_str().to_owned());
        })
        .unwrap();
    values
}

#[test]
fn duplicate_keys_and_escaped_names_inside_arrays() {
    for (source, input, expected) in [
        ("a.b", r#"{"a":[{"b":1,"b":2},{"b":3}]}"#, vec!["2", "3"]),
        (
            "a.b.c",
            r#"{"a":[{"b":{"c":1},"b":{}},{"b":{"c":2}}]}"#,
            vec!["2"],
        ),
        ("a.b", r#"{"a":[{"b":[1,2]}],"a":[{"b":[3]}]}"#, vec!["[3]"]),
        ("a.b", r#"{"a":[{"b":1}],"a":null}"#, vec![]),
        ("a.b", r#"{"a":[{"b":1,"\u0062":[]}]}"#, vec!["[]"]),
        (
            "a.`😀`",
            r#"{"a":[{"\ud83d\ude00":1},{"😀":2}]}"#,
            vec!["1", "2"],
        ),
    ] {
        assert_eq!(items(source, input), expected, "{source} / {input}");
    }
}

#[test]
fn fully_validate_before_emitting_any_sequence_item() {
    for input in [
        r#"{"a":[{"b":1},{"b":2}],"bad":[0,]}"#,
        r#"{"a":[{"b":1},{"b":2},]}"#,
        r#"[{"a":1},{"a":2}] trailing"#,
    ] {
        assert_eq!(
            jx::compile("a.b")
                .unwrap()
                .evaluate(input.as_bytes())
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
}

#[test]
fn cancellation_preserves_the_consumer_error_and_stops_emission() {
    let expression = jx::compile("a.b").unwrap();
    let input = br#"{"a":[{"b":1},{"b":2},{"b":3}]}"#;
    let mut seen = 0;
    let error = expression.evaluate(input).unwrap().try_for_each(|_| {
        seen += 1;
        Err("stop")
    });
    assert_eq!(error, Err(jx::ConsumeError::Consumer("stop")));
    assert_eq!(seen, 1);
    assert_eq!(
        expression
            .evaluate(input)
            .unwrap()
            .try_for_each(|_| Ok::<_, &str>(())),
        Ok(())
    );
    assert_eq!(
        items("a.b", std::str::from_utf8(input).unwrap()),
        ["1", "2", "3"]
    );
}

#[test]
fn streamed_values_borrow_their_original_ranges() {
    let input = br#"[{"a":123},{"a":456}]"#;
    let mut values = Vec::new();
    jx::compile("a")
        .unwrap()
        .evaluate(input)
        .unwrap()
        .for_each(|value| values.push(value.as_raw().unwrap()))
        .unwrap();
    assert_eq!(values[0].as_bytes().as_ptr(), input[6..].as_ptr());
    assert_eq!(values[1].as_bytes().as_ptr(), input[16..].as_ptr());
}

#[test]
fn traversal_is_bounded_by_validated_input_depth_not_array_width() {
    let levels = jx::MAX_DEPTH - 1;
    let input = format!("{}{{\"a\":1}}{}", "[".repeat(levels), "]".repeat(levels));
    assert_eq!(items("a", &input), ["1"]);
    let input = format!("[{}1{}]", "{\"a\":".repeat(levels), "}".repeat(levels));
    let source = vec!["a"; levels].join(".");
    assert_eq!(items(&source, &input), ["1"]);
    let input = format!("[{}]", vec![r#"{"a":1}"#; 10_000].join(","));
    let mut count = 0;
    jx::compile("a")
        .unwrap()
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|_| count += 1)
        .unwrap();
    assert_eq!(count, 10_000);
}
