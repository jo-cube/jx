use serde_json::Value;

#[test]
fn residual_compatibility() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../../../tests/semantics/compatibility.json")).unwrap();
    for case in cases {
        let source = case["expr"].as_str().unwrap();
        let expression = jx::compile(source).unwrap_or_else(|error| panic!("{case}: {error}"));
        let input = serde_json::to_vec(&case["data"]).unwrap();
        let mut items = Vec::new();
        let result = expression.evaluate(&input).and_then(|values| {
            values.for_each(|value| {
                let mut bytes = Vec::new();
                value.write_compact(&mut bytes).unwrap();
                items.push(serde_json::from_slice::<Value>(&bytes).unwrap());
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
fn native_array_coercion_stringifies_negative_zero() {
    for (source, negative) in [("$floor(?)(-0)", true), ("$floor(?)([-0])", false)] {
        let expression = jx::compile(source).unwrap();
        let value = expression
            .evaluate(b"null")
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        let number = value.as_number().unwrap();
        assert_eq!(number, 0.0);
        assert_eq!(number.is_sign_negative(), negative, "{source}");
    }
}

#[test]
fn date_literals_reject_known_legacy_case_fold_mismatches() {
    for literal in ["ı", "ſ", "ᾀ", "ᾳ"] {
        let source = format!("$toMillis(date,'[Y0001]{literal}[M01]-[D01]')");
        assert_eq!(
            jx::compile(&source)
                .unwrap()
                .evaluate(br#"{"date":"1970-01-01"}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::UnsupportedExpression
        );
        let expression = jx::compile("$toMillis(date,'[Y0001]-[M01]-[D01]')").unwrap();
        let input = serde_json::json!({"date":format!("1970{literal}01-01")}).to_string();
        assert_eq!(
            expression.evaluate(input.as_bytes()).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
}
