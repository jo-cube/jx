use serde_json::Value;

#[test]
fn function_runtime_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/functions.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let source = case["expr"].as_str().unwrap();
        let compiled = jx::compile(source);
        if case["phase"] == "compile" {
            assert_eq!(
                format!("{:?}", compiled.unwrap_err().kind),
                case["error"].as_str().unwrap(),
                "{case}"
            );
            continue;
        }
        let expression = compiled.unwrap_or_else(|e| panic!("{case}: {e}"));
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
                format!("{:?}", result.expect_err(&case.to_string()).kind),
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
fn tail_borrowing_validation_and_reuse() {
    let expression =
        jx::compile("($f:=function($n,$x)<nx:x>{$n=0?$x:$f($n-1,$x)};$f(n,obj))").unwrap();
    let input = br#"{"n":100000,"obj":{"x":"keep\u0020bytes"}}"#;
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| {
            let offset = input.windows(5).position(|b| b == br#"{"x":"#).unwrap();
            assert_eq!(
                value.as_raw().unwrap().as_bytes().as_ptr(),
                input[offset..].as_ptr()
            );
        })
        .unwrap();
    assert_eq!(
        expression
            .evaluate(br#"{"n":1,"obj":{},"bad":[0,]}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::InvalidJson
    );
    for _ in 0..10 {
        expression
            .evaluate(br#"{"n":1,"obj":{}}"#)
            .unwrap()
            .for_each(|_| {})
            .unwrap();
    }
}

#[test]
fn infinite_tail_calls_use_execution_budget_and_release_the_record() {
    let expression = jx::compile("($f:=function($n){$n=0?0:$f($n)};$f($))").unwrap();
    assert_eq!(
        expression.evaluate(b"1").unwrap_err().kind,
        jx::ErrorKind::EvaluationLimit
    );
    expression
        .evaluate(b"0")
        .unwrap()
        .for_each(|value| assert!(matches!(value, jx::Value::Number(0.0))))
        .unwrap();
}

#[test]
fn ambiguous_signature_matching_is_bounded() {
    let params = (0..80)
        .map(|i| format!("$x{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let args = std::iter::repeat_n("1", 81).collect::<Vec<_>>().join(",");
    let expression = jx::compile(&format!(
        "function({params})<{}:n>{{1}}({args})",
        "n?".repeat(80)
    ))
    .unwrap();
    assert_eq!(
        expression.evaluate(b"null").unwrap_err().kind,
        jx::ErrorKind::TypeError
    );
}

#[test]
fn frame_recycling_preserves_escaped_callback_bindings() {
    let expression=jx::compile("($make:=function($x)<n:f>{function(){$x}};$fns:=$map(a,$make);$map($fns,function($f){$f()}))").unwrap();
    for _ in 0..10 {
        let mut output = Vec::new();
        expression
            .evaluate(br#"{"a":[1,2,3]}"#)
            .unwrap()
            .for_each(|value| {
                let mut bytes = Vec::new();
                value.write_compact(&mut bytes).unwrap();
                output.push(serde_json::from_slice::<Value>(&bytes).unwrap());
            })
            .unwrap();
        assert_eq!(
            output,
            vec![
                serde_json::json!(1),
                serde_json::json!(2),
                serde_json::json!(3)
            ]
        );
    }
    let duplicate = jx::compile("function($x,$x){($x:=3;$x)}(1,2)").unwrap();
    duplicate
        .evaluate(b"null")
        .unwrap()
        .for_each(|value| assert!(matches!(value, jx::Value::Number(3.0))))
        .unwrap();
}
