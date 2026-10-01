use serde_json::Value;

#[test]
fn numeric_and_date_picture_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/formatting.json")).unwrap();
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
fn pictures_do_not_change_validation_or_argument_order() {
    for source in [
        "$formatNumber(n,'0.00')",
        "$fromMillis(t,'[Y]')",
        "$toMillis(date,'[Y]')",
        "false ? $fromMillis(1,'[Y') : 7",
    ] {
        let expr = jx::compile(source).unwrap();
        assert_eq!(
            expr.evaluate(br#"{"n":1,"t":0,"date":"2018","bad":[0,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
    let expr = jx::compile("$formatNumber(missing,$error('argument evaluated'))").unwrap();
    let error = expr.evaluate(b"null").unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::UserError);
    assert_eq!(error.message, "argument evaluated");
    let expr = jx::compile("$fromMillis(t,'[Y0001]-[M01]-[D01]')").unwrap();
    for _ in 0..100 {
        expr.evaluate(br#"{"t":1526947200000}"#)
            .unwrap()
            .for_each(|v| {
                let mut out = Vec::new();
                v.write_compact(&mut out).unwrap();
                assert_eq!(out, b"\"2018-05-22\"");
            })
            .unwrap();
    }
}
