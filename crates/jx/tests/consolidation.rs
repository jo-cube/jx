use serde_json::{Value, json};

fn items(source: &str, input: &[u8]) -> Result<Value, jx::Error> {
    let expression = jx::compile(source)?;
    let mut items = Vec::new();
    expression.evaluate(input)?.for_each(|value| {
        let mut bytes = Vec::new();
        value.write_compact(&mut bytes).unwrap();
        items.push(serde_json::from_slice::<Value>(&bytes).unwrap());
    })?;
    Ok(Value::Array(items))
}

#[test]
fn nested_lookup_preserves_array_boundaries_and_decoded_duplicate_keys() {
    for depth in [0, 1, 8, 64] {
        let input = format!(
            r#"{}[null,[],{{"id":0,"\u0069d":[[1],[2]]}},[{{"id":[[3]]}}]]{}"#,
            "[".repeat(depth),
            "]".repeat(depth)
        );
        assert_eq!(
            items("id", input.as_bytes()).unwrap(),
            json!([[1], [2], [3]])
        );
        assert_eq!(
            items("$lookup($,'id')", input.as_bytes()).unwrap(),
            json!([[1], [2], [3]])
        );
        assert_eq!(
            items("**.id", input.as_bytes()).unwrap(),
            json!([[1], [2], [3]])
        );
    }
}

#[test]
fn traversal_cancellation_still_follows_complete_validation() {
    let expression = jx::compile("id").unwrap();
    let input = br#"[[[{"id":1},{"id":2}]]]"#;
    let mut count = 0;
    let result = expression.evaluate(input).unwrap().try_for_each(|value| {
        assert_eq!(value.as_raw().unwrap().as_str(), "1");
        count += 1;
        Err(7)
    });
    assert_eq!(result, Err(jx::ConsumeError::Consumer(7)));
    assert_eq!(count, 1);
    assert_eq!(
        expression
            .evaluate(br#"[[[{"id":1}]],{"ignored":[1,]}]"#)
            .unwrap_err()
            .kind,
        jx::ErrorKind::InvalidJson
    );
}

#[test]
fn immediate_calls_preserve_rebinding_and_escaping_captures() {
    for (source, expected) in [
        ("($x:=1;function($a){$x+$a}($x:=3))", json!([6])),
        (
            "($make:=function($x){function(){$x}};$f:=$make(7);$f())",
            json!([7]),
        ),
        (
            "($f:=function($x){(function(){$x}())+0};$map([1,2,3],$f))",
            json!([1, 2, 3]),
        ),
        (
            "($fs:=$map([1,2,3],function($x){function($y){function(){$x+$y}}(10)});$map($fs,function($f){$f()}))",
            json!([11, 12, 13]),
        ),
        ("function($x){$eval('$x')+0}(7)", json!([7])),
    ] {
        assert_eq!(items(source, b"null").unwrap(), expected, "{source}");
    }
}

#[test]
fn ordering_and_grouping_preserve_upstream_outcomes() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/consolidation.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let result = items(
            case["expr"].as_str().unwrap(),
            &serde_json::to_vec(&case["data"]).unwrap(),
        );
        if let Some(error) = case.get("error") {
            assert_eq!(
                format!("{:?}", result.unwrap_err().kind),
                error.as_str().unwrap(),
                "{case}"
            );
        } else {
            assert_eq!(result.unwrap(), case["items"], "{case}");
        }
    }
}

#[test]
fn wide_groups_compare_encoded_keys_without_losing_member_order() {
    let mut rows = (0..40)
        .map(|i| format!(r#"{{"k":"k{i}","v":{i}}}"#))
        .collect::<Vec<_>>();
    rows.extend([
        r#"{"k":"k0","v":1000}"#.into(),
        r#"{"k":"\u006b0","v":1001}"#.into(),
        r#"{"k":"😀","v":1002}"#.into(),
        r#"{"k":"\ud83d\ude00","v":1003}"#.into(),
        r#"{"k":"\ud800","v":1004}"#.into(),
        r#"{"k":"\ud800","v":1005}"#.into(),
    ]);
    let input = format!(r#"{{"rows":[{}]}}"#, rows.join(","));
    assert_eq!(
        items("(rows{k:v}).k0", input.as_bytes()).unwrap(),
        json!([0, 1000, 1001])
    );
    assert_eq!(
        items("$lookup(rows{k:v},'😀')", input.as_bytes()).unwrap(),
        json!([1002, 1003])
    );
    assert_eq!(
        items("$lookup(rows{k:v},'\\ud800')", input.as_bytes()).unwrap(),
        json!([1004, 1005])
    );
    assert_eq!(
        items("rows{k:v,'k0':v}", input.as_bytes())
            .unwrap_err()
            .kind,
        jx::ErrorKind::DuplicateKey
    );
}
