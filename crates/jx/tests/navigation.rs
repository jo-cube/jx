use serde_json::Value;

#[test]
fn navigation_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/navigation.json")).unwrap();
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
fn validation_borrowing_and_cancellation() {
    for source in ["*", "**", "**.id", "a[]", "a^(id)", "a{\"x\":id}", "[1..5]"] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"a":1,"bad":[0,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
    let input = br#"{"a":{"id":123},"b":{"id":456}}"#;
    let expression = jx::compile("**.id").unwrap();
    let mut values = Vec::new();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| values.push(value.as_raw().unwrap()))
        .unwrap();
    assert_eq!(values[0].as_bytes().as_ptr(), input[11..].as_ptr());
    assert_eq!(values[1].as_bytes().as_ptr(), input[26..].as_ptr());
    let mut seen = 0;
    assert_eq!(
        expression.evaluate(input).unwrap().try_for_each(|_| {
            seen += 1;
            Err("done")
        }),
        Err(jx::ConsumeError::Consumer("done"))
    );
    assert_eq!(seen, 1);
    let levels = jx::MAX_DEPTH - 1;
    let input = format!("{}7{}", "[".repeat(levels), "]".repeat(levels));
    jx::compile("**")
        .unwrap()
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|v| assert_eq!(v.as_raw().unwrap().as_str(), "7"))
        .unwrap();
}

#[test]
fn range_limits_and_deferred_tuple_navigation_are_explicit() {
    for expr in ["[0..10000000]", "[9007199254740992..9007199254740992]"] {
        assert_eq!(
            jx::compile(expr)
                .unwrap()
                .evaluate(b"null")
                .unwrap_err()
                .kind,
            jx::ErrorKind::NumericRange
        );
    }
    for expr in ["1..3", "a[1..3]", "a{\"x\":1}{\"y\":2}"] {
        assert_eq!(
            jx::compile(expr).unwrap_err().kind,
            jx::ErrorKind::UnsupportedExpression
        );
    }
}

#[test]
fn nested_fallbacks_compile_without_expanding_the_left_tree() {
    for operator in ["??", "?:"] {
        let mut source = "missing".to_owned();
        for _ in 0..24 {
            source = format!("({source} {operator} missing)");
        }
        jx::compile(&source)
            .unwrap()
            .evaluate(b"{}")
            .unwrap()
            .for_each(|_| panic!("all branches are missing"))
            .unwrap();
    }
}

#[test]
fn object_enumeration_keeps_positions_and_last_spelling_across_promotion() {
    let expression = jx::compile("[$spread($)]").unwrap();
    let names = ["d", "c", "b", "a", "e"];
    for count in [0, 1, 3, 4, 5] {
        for duplicates in [false, true] {
            let mut members = Vec::new();
            for (i, name) in names[..count].iter().enumerate() {
                members.push(format!(r#""{name}":{i}"#));
                if duplicates {
                    members.push(format!(r#""{name}":{}"#, 100 + i));
                }
            }
            if duplicates && count != 0 {
                // Preserve the first position even when its spelling changes last.
                members.push(r#""\u0064":999"#.to_owned());
            }
            let input = format!("{{{}}}", members.join(","));
            let expected = names[..count]
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    if duplicates && i == 0 {
                        r#"{"\u0064":999}"#.to_owned()
                    } else {
                        format!(r#"{{"{name}":{}}}"#, i + if duplicates { 100 } else { 0 })
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            let mut output = Vec::new();
            expression
                .evaluate(input.as_bytes())
                .unwrap()
                .single()
                .unwrap()
                .unwrap()
                .write_compact(&mut output)
                .unwrap();
            assert_eq!(output, format!("[{expected}]").as_bytes(), "{input}");
        }
    }
}

#[test]
fn object_enumeration_preserves_utf16_names_and_integer_order_across_storage() {
    let input = r#"{"\ud800":5,"\uD800":6,"\udc00":8,"\uDC00":9,"😀":3,"\ud83d\ude00":4,"b":1,"\u0062":2,"10":10,"\u0032":20,"01":7}"#;
    let expected = r#"[{"\u0032":20},{"10":10},{"\uD800":6},{"\uDC00":9},{"\ud83d\ude00":4},{"\u0062":2},{"01":7}]"#;
    let owned = jx::OwnedValue::from_json(input.as_bytes()).unwrap();
    let constant = jx::CompileOptions::default()
        .constant_binding("object", owned.clone())
        .compile("[$spread($object)]")
        .unwrap();
    for source in ["[$spread($)]", "[$spread($clone($))]"] {
        let expression = jx::compile(source).unwrap();
        for focus in [None, Some(owned.as_value())] {
            let value = expression
                .evaluate_with(
                    Some(input.as_bytes()),
                    jx::EvaluationOptions {
                        focus,
                        ..Default::default()
                    },
                )
                .unwrap()
                .single()
                .unwrap()
                .unwrap();
            let mut output = Vec::new();
            value.write_compact(&mut output).unwrap();
            assert_eq!(output, expected.as_bytes(), "{source}");
        }
    }
    let mut output = Vec::new();
    constant
        .evaluate(b"null")
        .unwrap()
        .single()
        .unwrap()
        .unwrap()
        .write_compact(&mut output)
        .unwrap();
    assert_eq!(output, expected.as_bytes());
}

#[test]
fn nested_object_enumeration_retains_borrowed_values_across_workspace_reuse() {
    let expression = jx::compile("{'v':[({'a':left,'b':right}).*.*]}").unwrap();
    let expected = br#"{"v":[{"n":1},{"n":2},{"n":3}]}"#;
    let owned = {
        let input = br#"{"left":{"long_common_prefix_a":{"n":1},"long_common_prefix_b":{"n":2}},"right":{"different_prefix":{"n":3}}}"#.to_vec();
        let mut workspace = jx::execution::Workspace::default();
        let retained = expression
            .evaluate_with_workspace(Some(&input), Default::default(), &mut workspace)
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        let first = retained
            .get("v")
            .unwrap()
            .array_items()
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(
            first.as_raw().unwrap().as_bytes().as_ptr(),
            input[input
                .windows(7)
                .position(|bytes| bytes == br#"{"n":1}"#)
                .unwrap()..]
                .as_ptr()
        );
        let owned = retained.to_owned().unwrap();
        let next = expression
            .evaluate_with_workspace(
                Some(br#"{"left":{},"right":{}}"#),
                Default::default(),
                &mut workspace,
            )
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        let mut output = Vec::new();
        next.write_compact(&mut output).unwrap();
        assert_eq!(output, br#"{"v":[]}"#);
        workspace.clear();
        drop(workspace);
        output.clear();
        retained.write_compact(&mut output).unwrap();
        assert_eq!(output, expected);
        owned
    };
    let mut output = Vec::new();
    owned.as_value().write_compact(&mut output).unwrap();
    assert_eq!(output, expected);
}
