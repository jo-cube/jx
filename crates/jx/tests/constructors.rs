use serde_json::Value;

#[test]
fn constructor_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/constructors.json")).unwrap();
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
fn containers_own_structure_and_retain_borrowed_members() {
    let input = br#"{"a":{"text":"keep\u0020bytes","n":9007199254740993}}"#;
    let expression = jx::compile(r#"{"raw":a,"computed":1+2,"array":[a]}"#).unwrap();
    let mut values = Vec::new();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| values.push(value))
        .unwrap();
    let jx::Value::Object(object) = &values[0] else {
        panic!()
    };
    let members = object.as_slice();
    let raw = members[0].1.as_raw().unwrap();
    assert_eq!(raw.as_bytes().as_ptr(), input[5..].as_ptr());
    assert!(matches!(members[1].1, jx::Value::Number(3.0)));
    let jx::Value::Array(array) = &members[2].1 else {
        panic!()
    };
    assert_eq!(array.as_slice()[0].as_raw(), Some(raw));
    let mut bytes = Vec::new();
    values[0].write_compact(&mut bytes).unwrap();
    assert_eq!(bytes, br#"{"raw":{"text":"keep\u0020bytes","n":9007199254740993},"computed":3,"array":[{"text":"keep\u0020bytes","n":9007199254740993}]}"#);
    // Borrowed input leaves can outlive both the container and expression.
    drop(values);
    drop(expression);
    assert_eq!(raw.as_bytes().as_ptr(), input[5..].as_ptr());
}

#[test]
fn construction_preserves_internal_numbers_and_undefined() {
    for (source, expected) in [
        ("{\"x\":1/0}.x", f64::INFINITY),
        ("[0/0][0]", f64::NAN),
        ("{\"x\":-0}.x", -0.0),
    ] {
        jx::compile(source)
            .unwrap()
            .evaluate(b"{}")
            .unwrap()
            .for_each(|value| {
                let jx::Value::Number(actual) = value else {
                    panic!("{value:?}")
                };
                if expected.is_nan() {
                    assert!(actual.is_nan());
                } else {
                    assert_eq!(actual.to_bits(), expected.to_bits());
                }
            })
            .unwrap();
    }
    let expression = jx::compile("{\"x\":a.b[true]}.x").unwrap();
    let mut values = Vec::new();
    expression
        .evaluate(br#"{"a":[{},{"b":null}]}"#)
        .unwrap()
        .for_each(|value| values.push(value))
        .unwrap();
    assert!(matches!(values[0], jx::Value::Undefined));
    assert_eq!(values[1].as_raw().unwrap().as_str(), "null");
}

#[test]
fn construction_validates_before_output_and_cancels_between_containers() {
    let expression = jx::compile("a.{\"value\":b+1}").unwrap();
    let input = br#"{"a":[{"b":1},{"b":null}]}"#;
    assert_eq!(
        expression
            .evaluate(input)
            .unwrap()
            .try_for_each(|_| Err("stop")),
        Err(jx::ConsumeError::Consumer("stop"))
    );
    let mut count = 0;
    let error = expression
        .evaluate(input)
        .unwrap()
        .for_each(|_| count += 1)
        .unwrap_err();
    assert_eq!(count, 1);
    assert_eq!(error.kind, jx::ErrorKind::TypeError);
    for source in ["[a.{\"value\":b+1}]", "{\"values\":a.{\"value\":b+1}}"] {
        let mut count = 0;
        let error = jx::compile(source)
            .unwrap()
            .evaluate(input)
            .and_then(|result| result.for_each(|_| count += 1))
            .unwrap_err();
        assert_eq!(error.kind, jx::ErrorKind::TypeError);
        assert_eq!(count, 0, "{source}");
    }
    assert_eq!(
        expression
            .evaluate(br#"{"a":[{"b":1}],"bad":[0,]}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::InvalidJson
    );
    let error = jx::compile("1 + {\"x\":1,\"x\":2}")
        .unwrap()
        .evaluate(b"{}")
        .unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::DuplicateKey);
    assert_eq!(error.offset, 4);
}

#[test]
fn serialization_keeps_unicode_units_and_arbitrary_json_keys() {
    // Host-language prototype names are ordinary JSON keys in jx.
    let source = r#"{"\ud800": "\udfff", "a\"\\\n": "é😀", "__proto__": 1, "hasOwnProperty": 2}"#;
    let mut bytes = Vec::new();
    jx::compile(source)
        .unwrap()
        .evaluate(b"{}")
        .unwrap()
        .for_each(|value| value.write_compact(&mut bytes).unwrap())
        .unwrap();
    assert_eq!(
        bytes,
        r#"{"\ud800":"\udfff","a\"\\\n":"é😀","__proto__":1,"hasOwnProperty":2}"#.as_bytes()
    );
}

#[test]
fn constructor_syntax_and_depth_are_bounded() {
    for source in [
        "[1,]",
        "[,1]",
        "[1",
        "{1}",
        "{\"a\":}",
        "{\"a\":1,}",
        "a{\"x\":b}",
        "a[]",
        "[1..3]",
    ] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression,
            "{source}"
        );
    }
    for source in [
        format!("{}0{}", "[".repeat(200), "]".repeat(200)),
        format!("{}0{}", "{\"a\":".repeat(200), "}".repeat(200)),
    ] {
        assert_eq!(
            jx::compile(&source).unwrap_err().kind,
            jx::ErrorKind::DepthLimit
        );
    }
    let source = format!("{}${}", "[".repeat(60), "]".repeat(60));
    let input = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    jx::compile(&source)
        .unwrap()
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|value| value.write_compact(std::io::sink()).unwrap())
        .unwrap();
}

#[test]
fn implicit_string_iteration_after_constructor_collapse_is_explicitly_deferred() {
    let error = jx::compile(r#"["ab"][0].$"#)
        .unwrap()
        .evaluate(b"{}")
        .and_then(|result| result.for_each(|_| {}))
        .unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::UnsupportedExpression);
}

#[test]
fn object_construction_does_not_mutate_empty_input_arrays() {
    let mut values = Vec::new();
    let expression = jx::compile(r#"({"x":missing}.x) or $count($)"#).unwrap();
    expression
        .evaluate(b"[]")
        .unwrap()
        .for_each(|value| values.push(value))
        .unwrap();
    assert!(matches!(values.as_slice(), [jx::Value::Boolean(false)]));
}

#[test]
fn null_collapse_reports_a_type_error_instead_of_a_host_exception() {
    let error = jx::compile("[null][0].$")
        .unwrap()
        .evaluate(b"{}")
        .and_then(|result| result.for_each(|_| {}))
        .unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::TypeError);
}
