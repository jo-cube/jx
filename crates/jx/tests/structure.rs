use serde_json::Value;

#[test]
fn parent_and_transform_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/structure.json")).unwrap();
    let source_cases: Vec<std::collections::BTreeMap<String, Box<serde_json::value::RawValue>>> =
        serde_json::from_str(include_str!("../../../tests/semantics/structure.json")).unwrap();
    for (at, case) in cases.as_array().unwrap().iter().enumerate() {
        let source = case["expr"].as_str().unwrap();
        let expression = jx::compile(source);
        if case["phase"] == "compile" {
            assert_eq!(
                format!("{:?}", expression.unwrap_err().kind),
                case["error"].as_str().unwrap(),
                "{case}"
            );
            continue;
        }
        let expression = expression.unwrap_or_else(|e| panic!("{source}: {e}"));
        let input = source_cases[at]["data"].get().as_bytes();
        let mut items = Vec::<Value>::new();
        let result = expression.evaluate(input).and_then(|result| {
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
            result.unwrap_or_else(|e| panic!("{case}: {e}"));
            assert_eq!(Value::Array(items), case["items"], "{case}");
        }
    }
}

#[test]
fn undeducible_parents_fail_without_recursive_analysis() {
    for source in [
        "%",
        "$.%",
        "$$.%",
        "a.b.%.%.%",
        "a.**.%",
        "($x:=a;$x.%)",
        "{}.%%",
    ] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression,
            "{source}"
        );
    }
}

#[test]
fn clones_and_updates_keep_unrelated_leaves_borrowed() {
    let input = br#"{"a":{"x":1},"keep":{"text":"original"}}"#;
    for source in ["$clone($).keep.text", "($ ~> |a|{'x':x+1}|).keep.text"] {
        let expression = jx::compile(source).unwrap();
        expression
            .evaluate(input)
            .unwrap()
            .for_each(|value| {
                let raw = value
                    .as_raw()
                    .expect("unchanged strings still borrow input");
                assert_eq!(raw.as_bytes(), br#""original""#);
                assert_eq!(raw.as_bytes().as_ptr(), input[28..].as_ptr());
            })
            .unwrap();
    }
    let expression = jx::compile("($copy := $ ~> |a|{'x':x+1}|; [$copy.a.x,a.x])").unwrap();
    let mut bytes = Vec::new();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| value.write_compact(&mut bytes).unwrap())
        .unwrap();
    assert_eq!(bytes, b"[2,1]");
}

#[test]
fn clone_conversion_preserves_utf16_and_checks_all_numeric_leaves() {
    let expression = jx::compile("$clone($).text").unwrap();
    let mut bytes = Vec::new();
    expression
        .evaluate(br#"{"text":"\ud800","number":1.2345678901234567}"#)
        .unwrap()
        .for_each(|value| value.write_compact(&mut bytes).unwrap())
        .unwrap();
    assert_eq!(bytes, br#""\ud800""#);
    for source in ["$clone($).text", "$ ~> |missing|{}|"] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"text":"ok","unused":1e999}"#)
                .and_then(|result| result.for_each(|_| {}))
                .unwrap_err()
                .kind,
            jx::ErrorKind::NumericRange
        );
    }
    let expression =
        jx::compile("$clone({'nan':$number('oops0b1'),'function':function(){1}})").unwrap();
    let mut bytes = Vec::new();
    expression
        .evaluate(b"null")
        .unwrap()
        .for_each(|value| value.write_compact(&mut bytes).unwrap())
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap(),
        serde_json::json!({"nan":null,"function":""})
    );
}

#[test]
fn validation_cancellation_and_record_isolation() {
    let expression = jx::compile("a.b.{ 'parent':%.id,'x':x }").unwrap();
    for input in [
        br#"{"a":{"b":[{"x":1}]},"unused":[1,]}"#.as_slice(),
        b"{\"a\":{},\"unused\":\"\xff\"}",
    ] {
        let error = expression.evaluate(input).unwrap_err();
        assert_eq!(error, jx::validate(input).unwrap_err());
    }
    let error = expression
        .evaluate(br#"{"a":{"id":1,"b":[{"x":2},{"x":3}]}}"#)
        .unwrap()
        .try_for_each(|_| Err("stop"))
        .unwrap_err();
    assert_eq!(error, jx::ConsumeError::Consumer("stop"));
    for input in [
        br#"{"a":{"id":1,"b":[{"x":2}]}}"#.as_slice(),
        br#"{"a":{"id":2,"b":[{"x":3}]}}"#,
    ] {
        let mut count = 0;
        expression
            .evaluate(input)
            .unwrap()
            .for_each(|_| count += 1)
            .unwrap();
        assert_eq!(count, 1);
    }
    for source in ["$ ~> |a|{}|", "$clone($)"] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"a":{},"unused":nul}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
}

#[test]
fn unsupported_non_json_transform_edges_are_explicit() {
    for source in [
        "$ ~> |$|{'self':$}|",
        "($clone:=function($x){$x};$ ~> |a|{}|)",
    ] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"a":{}}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
    assert_eq!(
        jx::compile("$ ~> |a.x|{'y':1}|")
            .unwrap()
            .evaluate(br#"{"a":{"x":1}}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::TypeError
    );
}

#[test]
fn transform_alias_boundaries_are_explicit() {
    for source in ["$ ~> |$saved:=a|{}|", "$ ~> |a|$saved:=$|"] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
    for source in [
        "($old:=a;$ ~> |$old|{'x':2}|)",
        "$ ~> |a|{'f':function(){x}}|",
    ] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"a":{"x":1}}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
}

#[test]
fn resolved_parent_paths_count_toward_the_function_stack_budget() {
    let path = std::iter::repeat_n("a", 24).collect::<Vec<_>>().join(".");
    let source = format!("($f:=function($n){{$n=0?0:({path}).%.($f($n-1))}};$f(20))");
    let mut input = serde_json::json!({});
    for _ in 0..24 {
        input = serde_json::json!({"a":input});
    }
    let expression = jx::compile(&source).unwrap();
    assert_eq!(
        expression
            .evaluate(&serde_json::to_vec(&input).unwrap())
            .and_then(|result| result.for_each(|_| {}))
            .unwrap_err()
            .kind,
        jx::ErrorKind::DepthLimit
    );
}
