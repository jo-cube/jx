use serde_json::Value;

#[test]
fn scalar_operators_and_sequence_operands() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/scalars.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let source = case["expr"].as_str().unwrap();
        let expression = jx::compile(source).unwrap_or_else(|error| panic!("{source}: {error}"));
        let input = serde_json::to_vec(&case["data"]).unwrap();
        let result = expression.evaluate(&input);
        if let Some(kind) = case.get("error") {
            assert_eq!(
                format!("{:?}", result.unwrap_err().kind),
                kind.as_str().unwrap(),
                "{case}"
            );
        } else {
            let mut items = Vec::<Value>::new();
            result
                .unwrap_or_else(|error| panic!("{case}: {error}"))
                .for_each(|value| {
                    let mut bytes = Vec::new();
                    value.write_compact(&mut bytes).unwrap();
                    items.push(serde_json::from_slice(&bytes).unwrap());
                })
                .unwrap();
            assert_eq!(Value::Array(items), case["items"], "{case}");
        }
    }
}

#[test]
fn operator_errors_have_expression_offsets_after_complete_validation() {
    let expression = jx::compile("a + null").unwrap();
    let error = expression.evaluate(br#"{"a":1}"#).unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::TypeError);
    assert_eq!(error.offset, 2);
    for source in ["a + null", "false and (a + null)", "true", "'text'"] {
        let error = jx::compile(source)
            .unwrap()
            .evaluate(br#"{"bad":[0,]}"#)
            .unwrap_err();
        assert_eq!(error.kind, jx::ErrorKind::InvalidJson, "{source}");
    }
}

#[test]
fn compile_errors_and_expression_depth_are_bounded() {
    for source in [
        "1 +",
        "(1",
        "1)",
        "+1",
        ".5",
        "01",
        "1.",
        "1e",
        "!true",
        "a in b",
        "a & b",
        "'\\y'",
        "\"\\u123\"",
    ] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression,
            "{source}"
        );
    }
    assert_eq!(
        jx::compile("1e999").unwrap_err().kind,
        jx::ErrorKind::NumericRange
    );
    for source in [
        format!("{}1{}", "(".repeat(200), ")".repeat(200)),
        format!("{}1", "-".repeat(200)),
        vec!["1"; 200].join("+"),
    ] {
        assert_eq!(
            jx::compile(&source).unwrap_err().kind,
            jx::ErrorKind::DepthLimit
        );
    }
    let source = vec!["1"; 128].join("+");
    jx::compile(&source)
        .unwrap()
        .evaluate(b"null")
        .unwrap()
        .for_each(|value| assert!(matches!(value, jx::Value::Number(128.0))))
        .unwrap();
}

fn rendered(source: &str, input: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    jx::compile(source)
        .unwrap()
        .evaluate(input)
        .unwrap()
        .for_each(|value| value.write_compact(&mut bytes).unwrap())
        .unwrap();
    bytes
}

#[test]
fn duplicate_keys_unicode_and_binary64_boundaries() {
    for input in [
        br#"{"a":{"x":0,"\u0078":1},"b":{"x":1}}"#.as_slice(),
        br#"{"a":{"\ud800":1},"b":{"\ud800":1}}"#,
        br#"{"a":{"x":1},"b":{"x":0,"\u0078":1}}"#,
        br#"{"a":9007199254740993,"b":9007199254740992}"#,
        br#"{"a":1e999,"b":1e999}"#,
    ] {
        assert_eq!(rendered("a = b", input), b"true");
    }
    assert_eq!(
        rendered("a", br#"{"a":9007199254740993}"#),
        b"9007199254740993"
    );
    assert_eq!(
        rendered("a + 0", br#"{"a":9007199254740993}"#),
        b"9007199254740992"
    );
    assert_eq!(rendered("-0", b"null"), b"0");
    assert_eq!(rendered("'a\n\"b'", b"null"), br#""a\u000a\"b""#);
    assert_eq!(rendered(r#""\ud800""#, b"null"), br#""\ud800""#);
    assert_eq!(
        jx::compile("a or true")
            .unwrap()
            .evaluate(br#"{"a":[true,1e999]}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::NumericRange
    );
}

#[test]
fn computed_scalars_and_literal_strings_need_no_owned_output() {
    let expression = jx::compile("'text'").unwrap();
    expression
        .evaluate(b"null")
        .unwrap()
        .for_each(|value| {
            assert!(matches!(value, jx::Value::StringLiteral(_)));
        })
        .unwrap();
    assert_eq!(
        jx::compile("1 + 2")
            .unwrap()
            .evaluate(b"null")
            .unwrap()
            .try_for_each(|_| Err("stop")),
        Err(jx::ConsumeError::Consumer("stop"))
    );
}

#[test]
fn structural_equality_and_boolean_casting_respect_input_depth_limits() {
    let array = format!(
        "{}1{}",
        "[".repeat(jx::MAX_DEPTH - 1),
        "]".repeat(jx::MAX_DEPTH - 1)
    );
    let input = format!(r#"{{"a":{array},"b":{array}}}"#);
    assert_eq!(rendered("a = b", input.as_bytes()), b"true");
    assert_eq!(rendered("a and b", input.as_bytes()), b"true");
}

#[test]
fn expression_whitespace_matches_the_language_tokenizer() {
    assert!(jx::compile("1\u{c} + 2").is_err());
    assert_eq!(rendered("1\u{b} + 2", b"null"), b"3");
}
