use serde_json::{Value, json};

fn evaluate(source: &str, input: &Value) -> Value {
    let expression = jx::compile(source).unwrap();
    let input = serde_json::to_vec(input).unwrap();
    let mut output = Vec::new();
    expression
        .evaluate(&input)
        .unwrap()
        .for_each(|value| {
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            output.push(serde_json::from_slice::<Value>(&bytes).unwrap());
        })
        .unwrap();
    Value::Array(output)
}

#[test]
fn lookup_shapes_keys_and_calls() {
    for (source, input, expected) in [
        (r#"$lookup($,"a")"#, json!({"a":[1,2]}), json!([[1, 2]])),
        (r#"$lookup("a")"#, json!({"a":7}), json!([7])),
        (r#"$lookup(missing,"a")"#, json!({"a":7}), json!([])),
        (
            r#"$lookup({"undefined":3},missing)"#,
            json!(null),
            json!([3]),
        ),
        (
            r#"$lookup([{"a":[1,2]},{"a":[3]}],"a")"#,
            json!(null),
            json!([1, 2, 3]),
        ),
        (r#"$lookup([{"a":[]}],"a")"#, json!(null), json!([])),
        (r#"$lookup({"\ud800":2},"\ud800")"#, json!(null), json!([2])),
        (r#"($f:=$lookup;$f({"a":3},"a"))"#, json!(null), json!([3])),
        (
            r#"($lookup:=function($o,$k){9};$lookup({"a":1},"a"))"#,
            json!(null),
            json!([9]),
        ),
    ] {
        assert_eq!(evaluate(source, &input), expected, "{source}");
    }
    for source in ["$lookup()", "$lookup({},1)", "$lookup({},[],1)"] {
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
fn constant_construction_keeps_identity_and_shape() {
    for (source, expected) in [
        ("($f:=function(){[1]};$f() in [$f()])", json!([false])),
        (
            r#"($f:=function(){{"a":1}};$f() in [$f()])"#,
            json!([false]),
        ),
        (r#"($x:={"a":{"n":1}};$x.a in [$x.a])"#, json!([true])),
        ("[1,[2],([3])]", json!([[1, [2], 3]])),
        ("a[0+0][0]", json!([[1, 2]])),
        ("a[0][0]", json!([1])),
        ("(1/0) = (2/0)", json!([true])),
        ("(0/0) = (0/0)", json!([false])),
    ] {
        assert_eq!(
            evaluate(source, &json!({"a":[[1,2],[3]]})),
            expected,
            "{source}"
        );
    }
}

#[test]
fn constant_errors_remain_runtime_errors_after_validation() {
    for source in ["1+null", "$sum([1,true])", r#"{"a":1,"\u0061":2}"#] {
        let expression = jx::compile(source).unwrap();
        assert!(expression.evaluate(b"null").is_err(), "{source}");
        assert_eq!(
            expression.evaluate(b"[1,]").unwrap_err().kind,
            jx::ErrorKind::InvalidJson
        );
        assert_eq!(
            evaluate(&format!("false ? ({source}) : 7"), &json!(null)),
            json!([7])
        );
    }
}

#[test]
fn static_lookup_still_validates_and_normalizes_its_key() {
    let source = r#"$lookup({"a":1,"b":2,"undefined":3},key)"#;
    for (input, expected) in [
        (json!({"key":"a"}), json!([1])),
        (json!({}), json!([3])),
        (json!([{"key":"b"}]), json!([2])),
    ] {
        assert_eq!(evaluate(source, &input), expected);
    }
    let expression = jx::compile(source).unwrap();
    assert_eq!(
        expression
            .evaluate(br#"{"key":"a","bad":[1,]}"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::InvalidJson
    );
    assert_eq!(
        expression
            .evaluate(br#"[{"key":"a"},{"key":"b"}]"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::TypeError
    );
    assert_eq!(
        evaluate("($x:=[[1]];$x[0] in $x)", &json!(null)),
        json!([true])
    );
}
