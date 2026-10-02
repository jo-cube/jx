use serde_json::Value;

#[test]
fn runtime_boundary_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/runtime.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let expression = jx::compile(case["expr"].as_str().unwrap()).unwrap();
        let input = serde_json::to_vec(&case["data"]).unwrap();
        let mut items = Vec::new();
        expression
            .evaluate(&input)
            .unwrap_or_else(|e| panic!("{case}: {e}"))
            .for_each(|v| {
                let mut bytes = Vec::new();
                v.write_compact(&mut bytes).unwrap();
                items.push(serde_json::from_slice::<Value>(&bytes).unwrap());
            })
            .unwrap();
        assert_eq!(Value::Array(items), case["items"], "{case}");
    }
}

#[test]
fn transient_and_escaped_callbacks_preserve_borrowed_input() {
    let input = br#"{"a":[{"n":"keep\u0020bytes"},{"n":"also borrowed"}]}"#;
    for source in [
        "$map(a,function($x){($f:=function(){$x.n};$f())})",
        "($fns:=$map(a,function($x){function(){$x.n}});$map($fns,function($f){$f()}))",
    ] {
        let expression = jx::compile(source).unwrap();
        let mut count = 0;
        expression
            .evaluate(input)
            .unwrap()
            .for_each(|v| {
                let bytes = v.as_raw().unwrap().as_bytes();
                assert!(bytes.as_ptr() >= input.as_ptr());
                assert!(bytes.as_ptr_range().end <= input.as_ptr_range().end);
                count += 1;
            })
            .unwrap();
        assert_eq!(count, 2);
    }
}

#[test]
fn dynamic_plan_guards_preserve_errors_and_record_reuse() {
    let expression = jx::compile("($f:=$eval(code);$map(a,$f))").unwrap();
    for code in ["function($x){$x*2}", "function($x)<n:n>{$x*2}"] {
        for value in [
            serde_json::json!("bad"),
            serde_json::json!([]),
            serde_json::json!({}),
        ] {
            let input =
                serde_json::to_vec(&serde_json::json!({"a":[1,value,3],"code":code})).unwrap();
            assert_eq!(
                expression.evaluate(&input).unwrap_err().kind,
                jx::ErrorKind::TypeError
            );
        }
        let input = serde_json::to_vec(&serde_json::json!({"a":[1,2,3],"code":code})).unwrap();
        expression
            .evaluate(&input)
            .unwrap()
            .for_each(|_| {})
            .unwrap();
    }
    assert_eq!(
        expression
            .evaluate(br#"{"a":[1],"code":"function($x){$x*2}","bad":[0,]}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::InvalidJson
    );
}

#[test]
fn temporary_callback_captures_do_not_replay_effects() {
    fn draws(source: &str, seed: u64) -> Vec<f64> {
        let expression = jx::compile(source).unwrap();
        let mut result = Vec::new();
        expression
            .evaluate_with_random(b"null", &jx::Random::seeded(seed))
            .unwrap()
            .for_each(|v| {
                if let jx::Value::Number(n) = v {
                    result.push(n);
                } else if let jx::Value::Array(a) = v {
                    for v in a.as_slice() {
                        let jx::Value::Number(n) = v else {
                            panic!("number")
                        };
                        result.push(*n);
                    }
                } else {
                    panic!("number or array")
                }
            })
            .unwrap();
        result
    }
    assert_eq!(
        draws(
            "$map([1..3],function($x){($f:=function(){$random()};[$f(),$f()])})",
            7
        ),
        draws("[1..6].$random()", 7)
    );
}
