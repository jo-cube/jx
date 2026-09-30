use serde_json::Value;

#[test]
fn builtin_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/builtins.json")).unwrap();
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
fn builtin_retention_keeps_input_borrowing_and_evaluates_arguments_once() {
    let input = br#"{"a":[{"n":1},{"n":2}]}"#;
    let expression = jx::compile("$filter(a,function($v){$v.n>1})").unwrap();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|v| {
            assert_eq!(
                v.as_raw().unwrap().as_bytes().as_ptr(),
                input[14..].as_ptr()
            );
        })
        .unwrap();
    let expression = jx::compile("($n:=0;$map($n:=$n+1,function($v){[$v,$n]}))").unwrap();
    let mut output = Vec::new();
    expression
        .evaluate(b"null")
        .unwrap()
        .for_each(|v| {
            let mut bytes = Vec::new();
            v.write_compact(&mut bytes).unwrap();
            output.push(serde_json::from_slice::<Value>(&bytes).unwrap());
        })
        .unwrap();
    assert_eq!(output, vec![serde_json::json!([1, 1])]);
}

#[test]
fn validation_and_argument_errors_precede_callback_execution() {
    for source in ["$map(a,function(){1+null})", "$average(a)", "$trim(a)"] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"a":[],"bad":[0,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
    // The callback would fail with a type error; the complete argument fails first.
    let expression = jx::compile("$map([1,2].($=2?$sqrt(-1):$),function(){1+null})").unwrap();
    assert_eq!(
        expression.evaluate(b"null").unwrap_err().kind,
        jx::ErrorKind::NumericRange
    );
    let expression = jx::compile("$map([1,2,3],function($v){$v*2})").unwrap();
    let mut seen = 0;
    assert_eq!(
        expression.evaluate(b"null").unwrap().try_for_each(|_| {
            seen += 1;
            Err("stop")
        }),
        Err(jx::ConsumeError::Consumer("stop"))
    );
    assert_eq!(seen, 1);
}

#[test]
fn computed_strings_preserve_json_encoding_and_surrogate_units() {
    for (source, input, expected) in [
        (
            "$uppercase($)",
            r#""a\ud800b\udc00""#,
            r#""A\ud800B\udc00""#,
        ),
        ("$join($split($,''),'')", r#""a\ud800b""#, r#""a\ud800b""#),
        ("$uppercase($)", r#""a\"b\\c\n""#, r#""A\"B\\C\u000a""#),
    ] {
        let mut output = Vec::new();
        jx::compile(source)
            .unwrap()
            .evaluate(input.as_bytes())
            .unwrap()
            .for_each(|v| v.write_compact(&mut output).unwrap())
            .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }
}

#[test]
fn deferred_matchers_and_host_exceptions_are_explicit() {
    for source in [
        "$contains('abc',function(){()})",
        "$split('abc',function(){()})",
    ] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(b"null")
                .unwrap_err()
                .kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
    // Upstream throws uncoded JavaScript exceptions for these missing matchers.
    for source in [
        "$contains('abc',missing)",
        "$split('abc',missing)",
        "$substringAfter('undefined',missing)",
    ] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(b"null")
                .unwrap_err()
                .kind,
            jx::ErrorKind::TypeError
        );
    }
}
