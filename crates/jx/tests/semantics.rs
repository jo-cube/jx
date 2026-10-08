use jx::{ErrorKind, compile};

fn selected(expression: &str, input: &str) -> Option<String> {
    let mut selected = None;
    compile(expression)
        .unwrap()
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|value| {
            assert!(selected.is_none(), "expected at most one raw value");
            selected = Some(value.as_raw().unwrap().as_str().to_owned());
        })
        .unwrap();
    selected
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
        ("a /* comment */", r#"{"a":1}"#),
        ("é", r#"{"é":1}"#),
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
    for source in ["", ".", "a.", "a..b", "a /* unclosed", "a b", "`unclosed"] {
        assert_eq!(
            compile(source).unwrap_err().kind,
            ErrorKind::UnsupportedExpression,
            "{source}"
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
    let mut selected = None;
    {
        let expression = compile("a").unwrap();
        expression
            .evaluate(input)
            .unwrap()
            .for_each(|value| selected = value.as_raw())
            .unwrap();
    }
    assert_eq!(selected.unwrap().as_bytes().as_ptr(), input[6..].as_ptr());
}

#[test]
fn compile_once_reuse_across_records_and_threads() {
    let expression = compile("a.b").unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let expression = &expression;
            scope.spawn(move || {
                for _ in 0..100 {
                    let mut count = 0;
                    expression
                        .evaluate(br#"{"a":{"b":7}}"#)
                        .unwrap()
                        .for_each(|value| {
                            count += 1;
                            assert_eq!(value.as_raw().unwrap().as_str(), "7");
                        })
                        .unwrap();
                    assert_eq!(count, 1);
                    expression
                        .evaluate(b"{}")
                        .unwrap()
                        .for_each(|_| panic!("missing"))
                        .unwrap();
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

#[test]
fn concatenation_keeps_encoding_conversion_and_empty_borrowing() {
    let input = r#"{"a":"\ud83d","b":"\ude00","empty":"","text":"\n\\é","n":2,"object":{"x":1}}"#
        .as_bytes();
    for (source, expected) in [
        ("a & empty & b", r#""\ud83d\ude00""#),
        ("a & missing & b", r#""\ud83d\ude00""#),
        ("text & '-' & n & '-' & null", r#""\n\\é-2-null""#),
        ("missing & missing & missing", r#""""#),
        ("object & '-' & [1,2]", r#""{\"x\":1}-[1,2]""#),
    ] {
        let expression = compile(source).unwrap();
        let plan = jx::InputPlan::new([&expression]);
        let prepared = plan.prepare(input).unwrap();
        for evaluation in [expression.evaluate(input), prepared.evaluate(0)] {
            let value = evaluation.unwrap().single().unwrap().unwrap();
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            assert_eq!(bytes, expected.as_bytes(), "{source}");
            jx::validate(&bytes).unwrap();
        }
    }
    for source in [
        "missing & missing & text",
        "empty & missing & text",
        "empty & missing & empty",
    ] {
        let expression = compile(source).unwrap();
        let value = expression
            .evaluate(input)
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        let selected = compile(if source.ends_with("text") {
            "text"
        } else {
            "empty"
        })
        .unwrap();
        let expected = selected
            .evaluate(input)
            .unwrap()
            .single()
            .unwrap()
            .unwrap()
            .as_raw()
            .unwrap();
        assert_eq!(
            value.as_raw().unwrap().as_bytes().as_ptr(),
            expected.as_bytes().as_ptr()
        );
    }
}

#[test]
fn concatenation_results_survive_retention_and_later_evaluations() {
    let expression = compile("($s:=a & '-' & b & '-' & c; [$s, function(){ $s }()])").unwrap();
    let first = expression
        .evaluate(br#"{"a":"one","b":"two","c":"three"}"#)
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    let copy = first.clone();
    expression
        .evaluate(br#"{"a":"other","b":"record","c":"value"}"#)
        .unwrap()
        .single()
        .unwrap();
    for value in [first, copy] {
        let mut bytes = Vec::new();
        value.write_compact(&mut bytes).unwrap();
        assert_eq!(bytes, br#"["one-two-three","one-two-three"]"#);
    }
    for count in [2, 3, 5, 8, 9, 16, 32] {
        let expression = compile(
            &std::iter::repeat_n("a", count)
                .collect::<Vec<_>>()
                .join(" & "),
        )
        .unwrap();
        let value = expression
            .evaluate(r#"{"a":"é"}"#.as_bytes())
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        assert_eq!(value.as_str().unwrap().unwrap(), "é".repeat(count));
    }
}

#[test]
fn concatenation_preserves_operand_effects_before_conversion_errors() {
    use std::sync::{Arc, Mutex};
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    let host = jx::HostFunction::new(1, move |args, _| {
        let n = args[0].as_ref().unwrap().as_number().unwrap();
        observed.lock().unwrap().push(n as u32);
        Ok(Some(jx::Value::Number(n)))
    })
    .value();
    let options = jx::CompileOptions::default().binding("step");
    for (source, expected_calls, error_offset) in [
        ("$step(1) & $step(2) & $step(3)", vec![1, 2, 3], None),
        ("(1/0) & $step(2) & $step(3)", vec![2], Some(6)),
        ("$step(1) & (1/0) & $step(3)", vec![1], Some(9)),
        (
            "$step(1) & ($step(2) & (1/0)) & $step(3)",
            vec![1, 2],
            Some(21),
        ),
    ] {
        calls.lock().unwrap().clear();
        let expression = options.compile(source).unwrap();
        let result = expression.evaluate_with(
            Some(b"{}"),
            jx::EvaluationOptions {
                bindings: vec![("step", host.clone())],
                ..Default::default()
            },
        );
        if let Some(offset) = error_offset {
            let error = result.unwrap_err();
            assert_eq!(error.kind, ErrorKind::NumericRange);
            assert_eq!(error.offset, offset, "{source}");
        } else {
            assert_eq!(
                result
                    .unwrap()
                    .single()
                    .unwrap()
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .unwrap(),
                "123"
            );
        }
        assert_eq!(*calls.lock().unwrap(), expected_calls, "{source}");
    }
    calls.lock().unwrap().clear();
    let expression = options
        .compile("$lookup({'12':7},$step(1)&$step(2)) ?? $error('untaken')")
        .unwrap();
    let value = expression
        .evaluate_with(
            Some(b"{}"),
            jx::EvaluationOptions {
                bindings: vec![("step", host)],
                ..Default::default()
            },
        )
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    assert_eq!(value.as_number(), Some(7.));
    assert_eq!(*calls.lock().unwrap(), vec![1, 2, 1, 2]);
}
