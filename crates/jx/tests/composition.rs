use serde_json::Value;

#[test]
fn conversion_and_composition_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/composition.json")).unwrap();
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
fn string_identity_and_partial_arguments_keep_borrowing() {
    let input = br#"{"text":"a\u0062","rows":[{"n":1},{"n":2}]}"#;
    for source in [
        "$string(text)",
        "missing & text",
        "text & ''",
        "text ~> $string",
        "text ~> $string()",
    ] {
        jx::compile(source)
            .unwrap()
            .evaluate(input)
            .unwrap()
            .for_each(|value| {
                let raw = value.as_raw().expect("a string remains borrowed");
                assert_eq!(raw.as_bytes(), br#""a\u0062""#);
                assert_eq!(raw.as_bytes().as_ptr(), input[8..].as_ptr());
            })
            .unwrap();
    }
    let expr = jx::compile("($id:=function($x,$y){$y};$p:=$id(?,rows[0]);[$p(1),$p(2)])").unwrap();
    expr.evaluate(input)
        .unwrap()
        .for_each(|value| {
            let jx::Value::Array(array) = value else {
                panic!("constructed array")
            };
            let first = array.as_slice()[0].as_raw().unwrap();
            let second = array.as_slice()[1].as_raw().unwrap();
            assert_eq!(first.as_bytes(), br#"{"n":1}"#);
            assert_eq!(first.as_bytes().as_ptr(), second.as_bytes().as_ptr());
        })
        .unwrap();
}

#[test]
fn jsonata_stringification_is_distinct_from_output_serialization() {
    let input = br#"{"x":1.2000,"s":"\u0061\/\n\ud800","x":2.500,"2":2,"1":1}"#;
    let expr = jx::compile("$string($)").unwrap();
    expr.evaluate(input)
        .unwrap()
        .for_each(|value| {
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            let actual: String = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(actual, "{\"1\":1,\"2\":2,\"x\":2.5,\"s\":\"a/\\n\\ud800\"}");
        })
        .unwrap();
    let expr = jx::compile("$").unwrap();
    expr.evaluate(input)
        .unwrap()
        .for_each(|value| {
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            assert_eq!(bytes, input);
        })
        .unwrap();
}

#[test]
fn validation_and_error_order_are_preserved() {
    for source in [
        "$string($)",
        "$number(text)",
        "text & 1",
        "text ~> $uppercase()",
        "($f:=$substring(?,0,2);$f(text))",
    ] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"text":"hi","bad":[1,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
    for source in [
        "$string(1/0) & $sqrt(-1)",
        "missing($sqrt(-1),?)",
        "missing ~> missing($sqrt(-1))",
    ] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(b"null")
                .unwrap_err()
                .kind,
            jx::ErrorKind::NumericRange,
            "{source}"
        );
    }
    // Untyped native coercions and the reference's default-parameter partial bug
    // remain explicit instead of silently applying ordinary-call signatures.
    for source in [
        "$count(?)(2)",
        "$abs(?)('2')",
        "$map(?,$abs)(2)",
        "$string(?)(1)",
    ] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(b"null")
                .unwrap_err()
                .kind,
            jx::ErrorKind::UnsupportedExpression,
            "{source}"
        );
    }
    for source in ["?", "[?]", "1 ~", "$sum(?+1)"] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
}

#[test]
fn concatenation_preserves_escape_and_surrogate_boundaries() {
    for (input, expected) in [
        (r#"{"a":"\ud83d","b":"\ude00"}"#, "😀"),
        (r#"{"a":"a\\","b":"\"b\n"}"#, "a\\\"b\n"),
    ] {
        jx::compile("a & b")
            .unwrap()
            .evaluate(input.as_bytes())
            .unwrap()
            .for_each(|value| {
                let mut bytes = Vec::new();
                value.write_compact(&mut bytes).unwrap();
                assert_eq!(serde_json::from_slice::<String>(&bytes).unwrap(), expected);
            })
            .unwrap();
    }
}

#[test]
fn composed_calls_keep_resource_limits_and_record_isolation() {
    let expression = jx::compile(
        "($f:=function($x){$x};$g:=$reduce([1..100].$f,function($a,$b){$a~>$b});$g(1))",
    )
    .unwrap();
    assert_eq!(
        expression.evaluate(b"null").unwrap_err().kind,
        jx::ErrorKind::DepthLimit
    );
    let expression = jx::compile("($p:=function($a,$b){$a+$b}(?,n);$p(2))").unwrap();
    for (input, expected) in [
        (br#"{"n":1}"#.as_slice(), 3.0),
        (br#"{"n":3}"#.as_slice(), 5.0),
    ] {
        expression
            .evaluate(input)
            .unwrap()
            .for_each(|value| assert!(matches!(value,jx::Value::Number(n) if n == expected)))
            .unwrap();
    }
}

#[test]
fn direct_path_conversion_matches_dynamic_calls() {
    fn snapshot(source: &str, input: &[u8]) -> Result<Vec<Vec<u8>>, jx::ErrorKind> {
        let expression = jx::compile(source).unwrap();
        let mut values = Vec::new();
        expression
            .evaluate(input)
            .and_then(|result| {
                result.for_each(|value| {
                    let mut bytes = Vec::new();
                    value.write_compact(&mut bytes).unwrap();
                    values.push(bytes);
                })
            })
            .map_err(|error| error.kind)?;
        Ok(values)
    }
    for input in [
        br#"{"a":{"b":"12.5"}}"#.as_slice(),
        br#"{"a":{"b":null}}"#,
        br#"{"a":[{"b":1},{"b":2}]}"#,
        br#"{"a":[{"b":1},{}]}"#,
        br#"{"a":{"b":[1]}}"#,
        br#"{"a":{"b":1,"b":2},"a":{"b":3}}"#,
        br#"[{"a":{"b":1}},{"a":{"b":2}}]"#,
        br#"[{"a":[{"b":true},{}]}]"#,
        br#"{}"#,
        br#"null"#,
        br#"{"a":{"b":"12"},"unselected":[1,]}"#,
        br#"{"a":{"b":"12"}} trailing"#,
    ] {
        for builtin in ["number", "string"] {
            let direct = format!("${builtin}(a.b)");
            let dynamic = format!("($f:=${builtin};$f(a.b))");
            assert_eq!(
                snapshot(&direct, input),
                snapshot(&dynamic, input),
                "{direct} on {}",
                String::from_utf8_lossy(input)
            );
        }
    }
}
