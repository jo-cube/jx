use serde_json::Value;

#[test]
fn acquisition_lookup_and_string_results() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/acquisition.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let expression = jx::compile(case["expr"].as_str().unwrap()).unwrap();
        let input = case.get("input").map_or_else(
            || serde_json::to_vec(&case["data"]).unwrap(),
            |s| s.as_str().unwrap().as_bytes().to_vec(),
        );
        let mut items = Vec::new();
        let result = expression.evaluate(&input).and_then(|result| {
            result.for_each(|v| {
                let mut bytes = Vec::new();
                v.write_compact(&mut bytes).unwrap();
                items.push(serde_json::from_slice::<Value>(&bytes).unwrap());
            })
        });
        if let Some(error) = case.get("error") {
            assert_eq!(
                format!("{:?}", result.unwrap_err().kind),
                error.as_str().unwrap(),
                "{case}"
            );
            assert!(items.is_empty(), "{case}");
        } else {
            result.unwrap();
            assert_eq!(Value::Array(items), case["items"], "{case}");
        }
    }
}

#[test]
fn captured_arguments_borrow_input_and_validation_precedes_effects() {
    let expr = jx::compile("function($x){$x}(a)").unwrap();
    let input = br#"{"a":{"label":"bytes"}}"#;
    expr.evaluate(input)
        .unwrap()
        .for_each(|v| {
            let raw = v.as_raw().unwrap();
            assert_eq!(raw.as_bytes(), br#"{"label":"bytes"}"#);
            let start = input.as_ptr() as usize;
            assert!((start..start + input.len()).contains(&(raw.as_bytes().as_ptr() as usize)));
        })
        .unwrap();
    for source in [
        "function($a,$b)<nn:n>{$a+$b}(a,b)",
        "function($a){$error('body')}(a)",
    ] {
        let expr = jx::compile(source).unwrap();
        for input in [
            br#"{"a":1,"b":null,"bad":[0,]}"#.as_slice(),
            b"{\"a\":1} trailing",
            b"{\"a\":1,\"bad\":\"\xff\"}",
        ] {
            assert_eq!(
                expr.evaluate(input).unwrap_err().kind,
                jx::ErrorKind::InvalidJson
            );
        }
    }
}
