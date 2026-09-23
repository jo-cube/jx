use jx::{ErrorKind, compile};

fn selected(expression: &str, input: &str) -> Option<String> {
    compile(expression)
        .unwrap()
        .evaluate(input.as_bytes())
        .unwrap()
        .next()
        .map(|value| value.as_str().to_owned())
}

#[test]
fn object_paths_missing_and_raw_values() {
    let cases = [
        ("$", "  [1, [2], null] \n", Some("[1, [2], null]")),
        ("$", "null", Some("null")),
        ("a", r#"{"a":null}"#, Some("null")),
        ("a.b", r#"{"a":{"b":42}}"#, Some("42")),
        (" $. a . b ", r#"{"a":{"b":42}}"#, Some("42")),
        ("a", r#"{"a":[]}"#, Some("[]")),
        ("a", r#"{"a":[1]}"#, Some("[1]")),
        ("a", r#"{"a":[[1], [2]]}"#, Some("[[1], [2]]")),
        ("a", r#"{"a": { "x":1 }}"#, Some(r#"{ "x":1 }"#)),
        ("a.b", r#"{"a":null}"#, None),
        ("a.b", r#"{"a":42}"#, None),
        ("a", "false", None),
        ("a", "{}", None),
        ("a.b", r#"{"a":{}}"#, None),
        ("a", r#"{"irrelevant":[1,{"a":2}]}"#, None),
        ("a", r#"{"a":1,"a":2}"#, Some("2")),
        ("a.b", r#"{"a":{"b":1},"a":{}}"#, None),
        ("a.b", r#"{"a":[],"a":{"b":3}}"#, Some("3")),
        ("a.b", r#"{"a":[],"a":0}"#, None),
        ("a", r#"{"a":9007199254740993}"#, Some("9007199254740993")),
    ];
    for (expression, input, expected) in cases {
        assert_eq!(
            selected(expression, input).as_deref(),
            expected,
            "{expression} / {input}"
        );
    }
}

#[test]
fn field_names_compare_decoded_escapes() {
    let cases = [
        ("a", r#"{"\u0061":1}"#),
        ("`a.b`", r#"{"a.b":1}"#),
        ("``", r#"{"":1}"#),
        ("`é😀`", r#"{"\u00e9\ud83d\ude00":1}"#),
        ("`é😀`", r#"{"é😀":1}"#),
        ("`a\nb`", r#"{"a\nb":1}"#),
        ("`a\tb`", r#"{"a\u0009b":1}"#),
        ("`a\\b`", r#"{"a\\b":1}"#),
        ("`a/b`", r#"{"a\/b":1}"#),
        ("`a\"b`", r#"{"a\"b":1}"#),
        ("`\u{8}\u{c}\r`", r#"{"\b\f\r":1}"#),
        ("a", r#"{"a":0,"\u0061":1}"#),
    ];
    for (expression, input) in cases {
        assert_eq!(
            selected(expression, input).as_deref(),
            Some("1"),
            "{expression}"
        );
    }
    assert_eq!(selected("`�`", r#"{"\ud800":1}"#), None);
}

#[test]
fn unsupported_semantics_fail_explicitly() {
    for source in [
        "",
        ".",
        "a.",
        "a..b",
        "$foo",
        "$$",
        "$a.b",
        "a[0]",
        "a+b",
        "true",
        "null",
        "1",
        "*",
        "a.*",
        "a and b",
        "(a)",
        "{\"x\":a}",
        "$sum(a)",
        "\"a\"",
        "a.\"b\"",
        "a /* comment */",
        "a b",
        "`unclosed",
        "é",
    ] {
        assert_eq!(
            compile(source).unwrap_err().kind,
            ErrorKind::UnsupportedExpression,
            "{source}"
        );
    }
    for input in [
        "[]",
        "[{}]",
        r#"{"a":[]}"#,
        r#"{"a":[{"b":1},{"b":2}]}"#,
        r#"{"a":{"b":1},"a":[]}"#,
    ] {
        assert_eq!(
            compile("a.b")
                .unwrap()
                .evaluate(input.as_bytes())
                .unwrap_err()
                .kind,
            ErrorKind::ArrayTraversal,
            "{input}"
        );
    }
}

#[test]
fn validate_before_returning_a_selected_value_or_semantic_error() {
    for input in [
        r#"{"a":1,"unused":[0,]}"#,
        r#"{"a":[],"unused":!}"#,
        "{} trailing",
    ] {
        for source in ["a", "a.b", "$"] {
            assert_eq!(
                compile(source)
                    .unwrap()
                    .evaluate(input.as_bytes())
                    .unwrap_err()
                    .kind,
                ErrorKind::InvalidJson
            );
        }
    }
}

#[test]
fn results_borrow_input_not_the_expression() {
    let input = br#" {"a":123} "#;
    let mut results = compile("a").unwrap().evaluate(input).unwrap();
    let value = results.next().unwrap();
    assert_eq!(value.as_bytes().as_ptr(), input[6..].as_ptr());
    assert!(results.next().is_none());
    assert!(results.next().is_none());
}

#[test]
fn compile_once_reuse_across_records_and_threads() {
    let expression = compile("a.b").unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let expression = &expression;
            scope.spawn(move || {
                for _ in 0..100 {
                    assert_eq!(
                        expression
                            .evaluate(br#"{"a":{"b":7}}"#)
                            .unwrap()
                            .next()
                            .unwrap()
                            .as_str(),
                        "7"
                    );
                    assert!(expression.evaluate(b"{}").unwrap().next().is_none());
                }
            });
        }
    });
}

#[test]
fn compact_serialization_preserves_string_and_number_bytes() {
    let input = br#" { "space": " a b ", "esc": "\" \\ \n", "n": -0.00e+02, "a": [ 1, null ] } "#;
    let value = jx::validate(input).unwrap();
    let mut output = Vec::new();
    value.write_compact(&mut output).unwrap();
    assert_eq!(
        output,
        br#"{"space":" a b ","esc":"\" \\ \n","n":-0.00e+02,"a":[1,null]}"#
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(input).unwrap(),
        serde_json::from_slice::<serde_json::Value>(&output).unwrap()
    );
}

#[test]
fn serialization_propagates_consumer_failure() {
    struct Fails;
    impl std::io::Write for Fails {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert_eq!(
        jx::validate(b"[1]")
            .unwrap()
            .write_compact(Fails)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::BrokenPipe
    );
}
