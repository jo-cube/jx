use serde_json::Value;

#[test]
fn pure_region_results_and_fallbacks_match_readable_cases() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/regions.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let expression = jx::compile(case["expr"].as_str().unwrap()).unwrap();
        let input = serde_json::to_vec(&case["data"]).unwrap();
        let mut items = Vec::new();
        let result = expression.evaluate(&input).and_then(|result| {
            result.for_each(|value| {
                let mut bytes = Vec::new();
                value.write_compact(&mut bytes).unwrap();
                items.push(serde_json::from_slice::<Value>(&bytes).unwrap());
            })
        });
        if let Some(error) = case.get("error") {
            assert_eq!(
                format!("{:?}", result.unwrap_err().kind),
                error.as_str().unwrap(),
                "{case}"
            );
            assert!(items.is_empty(), "partial output: {case}");
        } else {
            result.unwrap();
            assert_eq!(Value::Array(items), case["items"], "{case}");
        }
    }
}

#[test]
fn validation_precedes_planned_calls_and_fold_output() {
    for source in [
        "function($r){$r.a+$r.b+$r.a}($)",
        "$sum($map(rows,function($r){$r.a*$r.b+$r.a}))",
    ] {
        let expression = jx::compile(source).unwrap();
        assert_eq!(
            expression
                .evaluate(br#"{"a":1,"b":2,"rows":[{"a":1,"b":2}],"bad":[0,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
    let expression = jx::compile("$sum($map(rows,function($r){$r.a*$r.b+$r.a}))").unwrap();
    assert_eq!(
        expression
            .evaluate(br#"{"rows":[{"a":1,"b":2},{"a":1,"b":null}]}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::TypeError
    );
}

#[test]
fn effectful_callbacks_keep_random_draw_order() {
    let first = jx::compile("$sum($map(rows,function($r){$random()+$r.a+$r.a}))").unwrap();
    let second = jx::compile("$sum(rows.($random()+a+a))").unwrap();
    let input = br#"{"rows":[{"a":1},{"a":2},{"a":3}]}"#;
    let read = |expr: &jx::Expression| {
        let random = jx::Random::seeded(7);
        let mut values = Vec::new();
        expr.evaluate_with_random(input, &random)
            .unwrap()
            .for_each(|value| {
                let jx::Value::Number(n) = value else {
                    panic!("numeric sum")
                };
                values.push(n)
            })
            .unwrap();
        values
    };
    assert_eq!(read(&first), read(&second));
}
