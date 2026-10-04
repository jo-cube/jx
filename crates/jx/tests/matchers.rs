use serde_json::Value;

#[test]
fn matcher_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/matchers.json")).unwrap();
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
fn borrowing_and_continuation_retention() {
    let input = br#"{"text":"aba"}"#;
    for source in [
        "$replace(text,/none/,'x')",
        "$replace(text,/a/,'x',0)",
        "(/aba/)(text).match",
    ] {
        jx::compile(source)
            .unwrap()
            .evaluate(input)
            .unwrap()
            .for_each(|value| {
                let raw = value
                    .as_raw()
                    .expect("unchanged or whole matched text stays borrowed");
                assert_eq!(raw.as_bytes(), br#""aba""#);
                assert_eq!(raw.as_bytes().as_ptr(), input[8..].as_ptr());
            })
            .unwrap();
    }
    for source in [
        "($r:=/a/;$s:='aba';$m:=$r($s);$s:='xxx';$m.next().start)",
        "($r:=/a/;$m:=$r(text & '!');$r:=/b/;$m.next().start)",
    ] {
        jx::compile(source)
            .unwrap()
            .evaluate(input)
            .unwrap()
            .for_each(|value| {
                assert!(matches!(value, jx::Value::Number(2.0)));
            })
            .unwrap();
    }
}

#[test]
fn independent_evaluations_and_cloned_expressions_have_fresh_cursors() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<jx::Expression>();
    let expr = jx::compile("($r:=/a/;$m:=$r(text);$m.next().start)").unwrap();
    let cloned = expr.clone();
    for expr in [&expr, &cloned, &expr] {
        for (input, expected) in [
            (br#"{"text":"aaa"}"#.as_slice(), Some(1.0)),
            (br#"{"text":"a"}"#, None),
        ] {
            let mut values = Vec::new();
            expr.evaluate(input)
                .unwrap()
                .for_each(|v| values.push(v))
                .unwrap();
            assert_eq!(values.len(), usize::from(expected.is_some()));
            if let Some(n) = expected {
                assert!(matches!(values[0],jx::Value::Number(v) if v == n));
            }
        }
        assert_eq!(
            expr.evaluate(br#"{"text":false}"#).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
}

#[test]
fn complete_validation_precedes_matcher_execution() {
    for source in [
        "$contains(text,/a/)",
        "$match(text,/a/)",
        "$replace(text,/a/,'b')",
        "text ~> /a/",
        "(/a/)(text).match",
    ] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"text":"a","ignored":[1,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
}

#[test]
fn legacy_case_folding_and_native_coercion_are_explicitly_deferred() {
    for source in ["/ſ/i", "/ı/i", r"/\u0131/i", "/[Ā-ƀ]/i"] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
    for (source, input) in [
        ("$contains(text,/s/i)", r#"{"text":"sſ"}"#),
        ("$replace(text,/i/i,'x')", r#"{"text":"ı"}"#),
        ("(/a/)(text)", r#"{"text":12}"#),
    ] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(input.as_bytes())
                .unwrap_err()
                .kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
    for source in ["//", "/a", "/a/g", "/a/ii", "/a/mm", "/(/", "/[a-/"] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
}

#[test]
fn legacy_greek_simple_uppercase_aliases_remain_explicitly_unsupported() {
    for pattern in ["/ᾀ/i", "/ᾄ/i", "/ᾳ/i", "/\\u1f80/i", "/[ᾀ-᾿]/i"] {
        assert_eq!(
            jx::compile(pattern).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
    for unit in ['ᾀ', 'ᾔ', 'ᾧ', 'ᾳ', 'ῃ', 'ῳ'] {
        let expression = jx::compile("$contains(text,/./i)").unwrap();
        let input = serde_json::json!({"text":unit.to_string()}).to_string();
        assert_eq!(
            expression.evaluate(input.as_bytes()).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
}
