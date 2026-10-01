use serde_json::Value;

#[test]
fn everyday_helper_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/helpers.json")).unwrap();
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
fn helpers_preserve_borrowing_and_validation() {
    for (source, input, expected) in [
        ("$single(a)", r#"{"a":[{"id":1}]}"#, r#"{"id":1}"#),
        ("$pad(a,1)", r#"{"a":"abc"}"#, r#""abc""#),
        ("$encodeUrlComponent(a)", r#"{"a":"abc"}"#, r#""abc""#),
        ("$decodeUrl(a)", r#"{"a":"abc"}"#, r#""abc""#),
    ] {
        jx::compile(source)
            .unwrap()
            .evaluate(input.as_bytes())
            .unwrap()
            .for_each(|v| {
                let raw = v.as_raw().unwrap();
                assert_eq!(raw.as_str(), expected);
                assert!(raw.as_bytes().as_ptr() >= input.as_ptr());
                assert!(raw.as_bytes().as_ptr() < input.as_ptr().wrapping_add(input.len()));
            })
            .unwrap();
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"a":1,"bad":[0,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
}
#[test]
fn user_messages_and_comment_errors_are_explicit() {
    let error = jx::compile("$error(message)")
        .unwrap()
        .evaluate(br#"{"message":"chosen message"}"#)
        .unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::UserError);
    assert_eq!(error.message, "chosen message");
    for source in ["/*", "1 /*", "/* nested /* */ */"] {
        assert_eq!(
            jx::compile(source).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
}

#[test]
fn predicates_after_grouping_require_a_path_or_boundary() {
    for source in [
        "[1,2,3]{'num':$}[true]",
        "$a{'num':$}[][true]",
        "(a){'num':$}[0]",
    ] {
        assert!(jx::compile(source).is_err(), "{source}");
    }
    for source in [
        "([1,2,3]{'num':$})[true]",
        "[1,2,3]{'num':$}[]",
        "a{'num':v}[0]",
        "a^(v){'num':v}[0]",
        "$a.v{'num':$}[0]",
    ] {
        assert!(jx::compile(source).is_ok(), "{source}");
    }
}
