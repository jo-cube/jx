use serde_json::Value;

#[test]
fn dynamic_evaluation_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/effects.json")).unwrap();
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

fn numbers(source: &str, random: &jx::Random) -> Vec<f64> {
    let expression = jx::compile(source).unwrap();
    let mut numbers = Vec::new();
    expression
        .evaluate_with_random(b"null", random)
        .unwrap()
        .for_each(|v| {
            if let jx::Value::Number(n) = v {
                numbers.push(n);
            } else if let jx::Value::Array(array) = v {
                for v in array.as_slice() {
                    let jx::Value::Number(n) = v else {
                        panic!("expected computed number")
                    };
                    numbers.push(*n);
                }
            } else {
                panic!("expected random numbers")
            }
        })
        .unwrap();
    numbers
}
#[test]
fn injected_randomness_is_shared_and_not_replayed() {
    let random = jx::Random::seeded(7);
    let expected = numbers("[1..8].$random()", &random);
    let random = jx::Random::seeded(7);
    assert_eq!(
        numbers("[$random(),$eval(\"$random()\"),$random()]", &random),
        expected[..3]
    );
    assert_eq!(
        numbers("($r:=$random;[$r(),$r()])", &random),
        expected[3..5]
    );
    assert_eq!(
        numbers("($v:=$random();[$v,$v])", &random),
        vec![expected[5]; 2]
    );
    let fresh = jx::Random::seeded(3);
    let draws = numbers("[1..4].$random()", &jx::Random::seeded(3));
    assert_eq!(
        numbers("[1..2][$random()>=0].$random()", &fresh),
        draws[2..]
    );
    let repeated = jx::compile("$random()").unwrap();
    let stream = jx::Random::seeded(7);
    for n in &expected {
        repeated
            .evaluate_with_random(b"null", &stream)
            .unwrap()
            .for_each(|v| assert!(matches!(v,jx::Value::Number(x) if x==*n)))
            .unwrap();
    }
    assert!(expected.iter().all(|n| (0.0..1.0).contains(n)));
    assert!(expected.windows(2).all(|v| v[0] != v[1]));
}
#[test]
fn skipped_effects_and_trivial_shuffles_do_not_draw() {
    let source = jx::Random::seeded(11);
    for expr in [
        "false?$random():7",
        "$shuffle(missing)",
        "$shuffle([])",
        "$shuffle([1])",
        "$shuffle(1)",
        "$eval(missing)",
    ] {
        jx::compile(expr)
            .unwrap()
            .evaluate_with_random(b"null", &source)
            .unwrap()
            .for_each(|_| {})
            .unwrap();
    }
    assert_eq!(
        numbers("$random()", &source),
        numbers("$random()", &jx::Random::seeded(11))
    );
}
#[test]
fn dynamic_randomness_shares_the_injected_source() {
    let expression = jx::compile("[$random(),$eval(code),$random()]").unwrap();
    let mut actual = Vec::new();
    expression
        .evaluate_with_random(br#"{"code":"$random()"}"#, &jx::Random::seeded(8))
        .unwrap()
        .for_each(|v| {
            let jx::Value::Array(array) = v else {
                panic!("expected array")
            };
            for v in array.as_slice() {
                let jx::Value::Number(n) = v else {
                    panic!("expected number")
                };
                actual.push(*n);
            }
        })
        .unwrap();
    assert_eq!(actual, numbers("[1..3].$random()", &jx::Random::seeded(8)));
}
#[test]
fn dynamic_results_borrow_input_and_validation_precedes_effects() {
    let expression = jx::compile("$eval(code)").unwrap();
    let input = br#"{"code":"obj", "obj": {"n": 7}}"#;
    let offset = input.windows(8).position(|s| s == br#"{"n": 7}"#).unwrap();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|v| {
            assert_eq!(
                v.as_raw().unwrap().as_bytes().as_ptr(),
                input[offset..].as_ptr()
            )
        })
        .unwrap();
    let random = jx::Random::seeded(2);
    for expr in [
        "$random()",
        "$eval(\"#\")",
        "$eval(code)",
        "$shuffle([1,2])",
    ] {
        assert_eq!(
            jx::compile(expr)
                .unwrap()
                .evaluate_with_random(br#"{"bad":[0,]}"#, &random)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
    assert_eq!(
        numbers("$random()", &random),
        numbers("$random()", &jx::Random::seeded(2))
    );
}

#[test]
fn dynamic_errors_preserve_effects_and_shuffle_uses_one_shared_stream() {
    let random = jx::Random::seeded(17);
    let expected = numbers("[1..8].$random()", &jx::Random::seeded(17));
    let expression = jx::compile("$eval(code)").unwrap();
    assert_eq!(
        expression
            .evaluate_with_random(br#"{"code":"($random();$error('bad'))"}"#, &random)
            .unwrap_err()
            .kind,
        jx::ErrorKind::EvalError
    );
    assert_eq!(numbers("$random()", &random), expected[1..2]);
    let mut permutation = Vec::new();
    for (index, draw) in expected[2..6].iter().enumerate() {
        permutation.push((index + 1) as f64);
        permutation.swap(index, (draw * (index + 1) as f64) as usize);
    }
    assert_eq!(numbers("$shuffle([1,2,3,4])", &random), permutation);
    assert_eq!(numbers("$random()", &random), expected[6..7]);
}

#[test]
fn compatibility_boundaries_are_explicit() {
    assert_eq!(
        jx::compile("$eval(code)")
            .unwrap()
            .evaluate(br#"{"code":"\"\ud800\""}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::EvalSyntax
    );
    assert_eq!(
        jx::compile("$shuffle(?)(\"abc\")")
            .unwrap()
            .evaluate(b"null")
            .unwrap_err()
            .kind,
        jx::ErrorKind::UnsupportedExpression
    );
}

#[test]
fn eval_errors_keep_the_nested_diagnostic() {
    for source in ["$eval('$error(\"inner\")')", "$eval(code)"] {
        let error = jx::compile(source)
            .unwrap()
            .evaluate(br#"{"code":"$error('inner')"}"#)
            .unwrap_err();
        assert_eq!(error.kind, jx::ErrorKind::EvalError);
        let message = error.to_string();
        assert!(
            message.contains("D3121") && message.contains("inner") && message.contains("UserError"),
            "{message}"
        );
    }
    let error = jx::compile("$eval('1+')")
        .unwrap()
        .evaluate(b"null")
        .unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::EvalSyntax);
    assert!(error.to_string().contains("D3120"));
}
