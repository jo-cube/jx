use serde_json::Value;

#[test]
fn lexical_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/lexical.json")).unwrap();
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
fn retained_bindings_preserve_input_borrowing() {
    let input = br#"{"value":{"text":"keep\u0020bytes","n":9007199254740993}}"#;
    let expression = jx::compile("($x:=value;$f:=function(){$x};$f())").unwrap();
    let mut result = None;
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| result = value.as_raw())
        .unwrap();
    let raw = result.unwrap();
    assert_eq!(raw.as_bytes().as_ptr(), input[9..].as_ptr());
    drop(expression);
    assert_eq!(
        raw.as_str(),
        r#"{"text":"keep\u0020bytes","n":9007199254740993}"#
    );
}

#[test]
fn evaluations_isolate_scope_and_validate_before_execution() {
    let expression = jx::compile("($x:=a;$f:=function(){$x+1};$f())").unwrap();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let expression = &expression;
            scope.spawn(move || {
                for n in 0..100 {
                    let input = format!("{{\"a\":{n}}}");
                    expression.evaluate(input.as_bytes()).unwrap().for_each(|value| {
                        assert!(matches!(value, jx::Value::Number(number) if number == f64::from(n+1)));
                    }).unwrap();
                }
            });
        }
    });
    assert_eq!(
        expression
            .evaluate(br#"{"a":1,"bad":[0,]}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::InvalidJson
    );
}

#[test]
fn deferred_features_and_constructor_binding_races_are_explicit() {
    for source in [
        "function($x)<n:n>{$x}",
        "($x:=0;[$x:=1,$x:=2,$x])",
        "{\"x\": $x:=1, \"y\":$x}",
    ] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression,
            "{source}"
        );
    }
    for source in ["($f:=$random;$f())", "$shuffle([2,1])"] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(b"null")
                .unwrap_err()
                .kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
    for source in ["$unknown()", "$sum(1)()", "($sum:=missing;$sum(1))"] {
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

#[test]
fn recursion_is_bounded_and_an_error_does_not_poison_reuse() {
    let expression = jx::compile("($f:=function($n){$n=0?0:$f($n-1)};$f($))").unwrap();
    assert_eq!(
        expression.evaluate(b"1000").unwrap_err().kind,
        jx::ErrorKind::DepthLimit
    );
    expression
        .evaluate(b"5")
        .unwrap()
        .for_each(|v| assert!(matches!(v, jx::Value::Number(0.0))))
        .unwrap();
}

#[test]
fn function_results_are_opaque_and_not_json() {
    for source in ["function(){1}", "$sum", "($f:=function(){$f};$f)"] {
        jx::compile(source)
            .unwrap()
            .evaluate(b"null")
            .unwrap()
            .for_each(|value| {
                assert!(matches!(value, jx::Value::Function(_)));
                assert_eq!(
                    value.write_compact(Vec::new()).unwrap_err().kind(),
                    std::io::ErrorKind::InvalidInput
                );
            })
            .unwrap();
    }
}

#[test]
fn retained_output_obeys_consumer_cancellation() {
    let expression = jx::compile("($x:=a.b;$x)").unwrap();
    let mut count = 0;
    let result = expression
        .evaluate(br#"{"a":[{"b":1},{"b":2}]}"#)
        .unwrap()
        .try_for_each(|_| {
            count += 1;
            Err("stop")
        });
    assert_eq!(count, 1);
    assert_eq!(result, Err(jx::ConsumeError::Consumer("stop")));
}

#[test]
fn recursion_accounts_for_large_function_bodies() {
    let body = std::iter::repeat_n("1", 90)
        .chain(["$f()"])
        .collect::<Vec<_>>()
        .join("+");
    let expression = jx::compile(&format!("($f:=function(){{{body}}};$f())")).unwrap();
    assert_eq!(
        expression.evaluate(b"null").unwrap_err().kind,
        jx::ErrorKind::DepthLimit
    );
}
