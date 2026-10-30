use serde_json::Value;

#[test]
fn builtin_semantics() {
    let cases: Value =
        serde_json::from_str(include_str!("../../../tests/semantics/builtins.json")).unwrap();
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
fn builtin_retention_keeps_input_borrowing_and_evaluates_arguments_once() {
    let input = br#"{"a":[{"n":1},{"n":2}]}"#;
    let expression = jx::compile("$filter(a,function($v){$v.n>1})").unwrap();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|v| {
            assert_eq!(
                v.as_raw().unwrap().as_bytes().as_ptr(),
                input[14..].as_ptr()
            );
        })
        .unwrap();
    let expression = jx::compile("($n:=0;$map($n:=$n+1,function($v){[$v,$n]}))").unwrap();
    let mut output = Vec::new();
    expression
        .evaluate(b"null")
        .unwrap()
        .for_each(|v| {
            let mut bytes = Vec::new();
            v.write_compact(&mut bytes).unwrap();
            output.push(serde_json::from_slice::<Value>(&bytes).unwrap());
        })
        .unwrap();
    assert_eq!(output, vec![serde_json::json!([1, 1])]);
}

#[test]
fn validation_and_argument_errors_precede_callback_execution() {
    for source in ["$map(a,function(){1+null})", "$average(a)", "$trim(a)"] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(br#"{"a":[],"bad":[0,]}"#)
                .unwrap_err()
                .kind,
            jx::ErrorKind::InvalidJson
        );
    }
    // The callback would fail with a type error; the complete argument fails first.
    let expression = jx::compile("$map([1,2].($=2?$sqrt(-1):$),function(){1+null})").unwrap();
    assert_eq!(
        expression.evaluate(b"null").unwrap_err().kind,
        jx::ErrorKind::NumericRange
    );
    let expression = jx::compile("$map([1,2,3],function($v){$v*2})").unwrap();
    let mut seen = 0;
    assert_eq!(
        expression.evaluate(b"null").unwrap().try_for_each(|_| {
            seen += 1;
            Err("stop")
        }),
        Err(jx::ConsumeError::Consumer("stop"))
    );
    assert_eq!(seen, 1);
}

#[test]
fn computed_strings_preserve_json_encoding_and_surrogate_units() {
    for (source, input, expected) in [
        (
            "$uppercase($)",
            r#""a\ud800b\udc00""#,
            r#""A\ud800B\udc00""#,
        ),
        ("$join($split($,''),'')", r#""a\ud800b""#, r#""a\ud800b""#),
        ("$uppercase($)", r#""a\"b\\c\n""#, r#""A\"B\\C\u000a""#),
    ] {
        let mut output = Vec::new();
        jx::compile(source)
            .unwrap()
            .evaluate(input.as_bytes())
            .unwrap()
            .for_each(|v| v.write_compact(&mut output).unwrap())
            .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }
}

#[test]
fn missing_matchers_and_host_exceptions_are_explicit() {
    // Upstream throws uncoded JavaScript exceptions for these missing matchers.
    for source in [
        "$contains('abc',missing)",
        "$split('abc',missing)",
        "$substringAfter('undefined',missing)",
    ] {
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
fn joins_preserve_encoding_and_utf16_boundaries() {
    for source in [
        "$join(items,sep)",
        "$join([items[0],$substring(items[1],0)],sep)",
    ] {
        let expression = jx::compile(source).unwrap();
        for (input, expected) in [
            (r#"{"items":["a","b"],"sep":"|"}"#, r#""a|b""#),
            (r#"{"items":["é","😀"],"sep":"中"}"#, r#""é中😀""#),
            (r#"{"items":["\u0061","\/"],"sep":""}"#, r#""a/""#),
            (r#"{"items":["a","b"],"sep":"\u002f"}"#, r#""a/b""#),
            (r#"{"items":["\"","\\"],"sep":"\n"}"#, r#""\"\u000a\\""#),
            (
                r#"{"items":["\t","\u0000"],"sep":"\r"}"#,
                r#""\u0009\u000d\u0000""#,
            ),
            (r#"{"items":["\ud83d","\ude00"],"sep":""}"#, r#""😀""#),
            (r#"{"items":["\ud83d",""],"sep":"\ude00"}"#, r#""😀""#),
            (r#"{"items":["","\ude00"],"sep":"\ud83d"}"#, r#""😀""#),
            (r#"{"items":["\ud800","x"],"sep":""}"#, r#""\ud800x""#),
            (
                r#"{"items":["\udfff","\ud800"],"sep":""}"#,
                r#""\udfff\ud800""#,
            ),
            (r#"{"items":["",""],"sep":""}"#, r#""""#),
        ] {
            let value = expression
                .evaluate(input.as_bytes())
                .unwrap()
                .single()
                .unwrap()
                .unwrap();
            let mut output = Vec::new();
            value.write_compact(&mut output).unwrap();
            assert_eq!(output, expected.as_bytes(), "{source}: {input}");
        }
    }
}

#[test]
fn joins_preserve_cardinality_validation_and_argument_order() {
    let expression = jx::compile("$join(items,sep)").unwrap();
    for (input, expected) in [
        (r#"{}"#, None),
        (r#"{"items":[]}"#, Some("")),
        (r#"{"items":"x","sep":"unused"}"#, Some("x")),
        (r#"{"items":["x"]}"#, Some("x")),
        (r#"{"items":"","sep":"unused"}"#, Some("")),
        (r#"{"items":["","",""],"sep":"é"}"#, Some("éé")),
        (r#"{"items":["a","b"]}"#, Some("ab")),
    ] {
        let value = expression
            .evaluate(input.as_bytes())
            .unwrap()
            .single()
            .unwrap();
        assert_eq!(
            value
                .as_ref()
                .map(|v| v.as_str().unwrap().unwrap().into_owned()),
            expected.map(str::to_owned),
            "{input}"
        );
    }
    for input in [
        r#"{"items":null}"#,
        r#"{"items":["a",1]}"#,
        r#"{"items":[[]]}"#,
        r#"{"items":[],"sep":1}"#,
        r#"{"sep":null}"#,
    ] {
        assert_eq!(
            expression
                .evaluate(input.as_bytes())
                .and_then(|v| v.single())
                .unwrap_err()
                .kind,
            jx::ErrorKind::TypeError,
            "{input}"
        );
    }
    for source in ["$join([1],$sqrt(-1))", "$join($sqrt(-1),1)"] {
        assert_eq!(
            jx::compile(source)
                .unwrap()
                .evaluate(b"null")
                .and_then(|v| v.single())
                .unwrap_err()
                .kind,
            jx::ErrorKind::NumericRange
        );
    }
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = calls.clone();
    let probe = jx::HostFunction::new(1, move |args, _| {
        let n = args[0].as_ref().unwrap().as_number().unwrap() as u8;
        seen.lock().unwrap().push(n);
        Ok(Some(jx::Value::from_string(n.to_string())))
    });
    let expression = jx::CompileOptions::default()
        .binding("probe")
        .compile("$join([$probe(1),$probe(2)],$probe(3))")
        .unwrap();
    let value = expression
        .evaluate_with(
            Some(b"null"),
            jx::EvaluationOptions {
                bindings: vec![("probe", probe.value())],
                ..Default::default()
            },
        )
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    assert_eq!(value.as_str().unwrap().unwrap(), "132");
    assert_eq!(*calls.lock().unwrap(), [1, 2, 3]);
}

#[test]
fn collection_keys_preserve_order_and_replacement_across_index_promotion() {
    let expression = jx::compile("{'merged':$merge(a),'keys':$keys(a)}").unwrap();
    for count in [31, 32, 33, 64] {
        let mut objects = (0..count)
            .map(|i| format!(r#"{{"key{i:03}":{i}}}"#))
            .collect::<Vec<_>>();
        let keys = (0..count)
            .map(|i| format!(r#""key{i:03}""#))
            .collect::<Vec<_>>()
            .join(",");
        for duplicates in [false, true] {
            if duplicates {
                objects.extend(
                    (0..count)
                        .rev()
                        .map(|i| format!(r#"{{"key{i:03}":{}}}"#, i + 100)),
                );
            }
            let input = format!(r#"{{"a":[{}]}}"#, objects.join(","));
            let members = (0..count)
                .map(|i| format!(r#""key{i:03}":{}"#, i + if duplicates { 100 } else { 0 }))
                .collect::<Vec<_>>()
                .join(",");
            let expected = format!(r#"{{"merged":{{{members}}},"keys":[{keys}]}}"#);
            let value = expression
                .evaluate(input.as_bytes())
                .unwrap()
                .single()
                .unwrap()
                .unwrap();
            let mut output = Vec::new();
            value.write_compact(&mut output).unwrap();
            assert_eq!(
                output,
                expected.as_bytes(),
                "{count}, duplicates={duplicates}"
            );
        }
    }
}

#[test]
fn indexed_collection_keys_preserve_utf16_equality_and_encoded_spelling() {
    let mut objects = (0..32)
        .map(|i| format!(r#"{{"k{i:03}":{i}}}"#))
        .collect::<Vec<_>>();
    objects.extend(
        [
            r#"{"a":1,"\u0061":2,"\u0032":"two","10":"ten"}"#,
            r#"{"a":3,"é":4,"\u00e9":5}"#,
            r#"{"\ud800":6,"\udc00":7,"\\ud800":8}"#,
            r#"{"\uD800":9,"\uDC00":10,"é":11}"#,
            r#"{"":12,"😀":13}"#,
            r#"{"\ud83d\ude00":14,"\ue000":15}"#,
        ]
        .map(str::to_owned),
    );
    let input = format!(r#"{{"a":[{}]}}"#, objects.join(","));
    let mut members = vec![r#""\u0032":"two""#.to_owned(), r#""10":"ten""#.to_owned()];
    members.extend((0..32).map(|i| format!(r#""k{i:03}":{i}"#)));
    members.extend(
        [
            r#""\u0061":3"#,
            r#""\u00e9":11"#,
            r#""\ud800":9"#,
            r#""\udc00":10"#,
            r#""\\ud800":8"#,
            r#""":15"#,
            r#""😀":14"#,
        ]
        .map(str::to_owned),
    );
    let mut keys = vec![r#""\u0032""#.to_owned(), r#""10""#.to_owned()];
    keys.extend((0..32).map(|i| format!(r#""k{i:03}""#)));
    keys.extend(
        [
            r#""\u0061""#,
            r#""\u00e9""#,
            r#""\ud800""#,
            r#""\udc00""#,
            r#""\\ud800""#,
            r#""""#,
            r#""😀""#,
        ]
        .map(str::to_owned),
    );
    let expected = format!(
        r#"{{"merged":{{{}}},"keys":[{}]}}"#,
        members.join(","),
        keys.join(",")
    );
    let expression = jx::compile("{'merged':$merge(a),'keys':$keys(a)}").unwrap();
    let value = expression
        .evaluate(input.as_bytes())
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    let mut output = Vec::new();
    value.write_compact(&mut output).unwrap();
    assert_eq!(output, expected.as_bytes());
    assert_eq!(
        expression
            .evaluate(format!("{input} false").as_bytes())
            .unwrap_err()
            .kind,
        jx::ErrorKind::InvalidJson
    );
}

#[test]
fn collection_indexes_do_not_escape_mixed_containers_or_workspace_reuse() {
    let expression = jx::compile(
        "($m := $merge($append(a,{'key000':payload,'made':payload})); \
         {'merged':$m,'keys':$keys([a,[$m]])})",
    )
    .unwrap();
    let members = (0..33)
        .map(|i| {
            format!(
                r#""key{i:03}":{}"#,
                if i == 0 {
                    r#"{"n":1.00e+0}"#.to_owned()
                } else {
                    i.to_string()
                }
            )
        })
        .chain([r#""made":{"n":1.00e+0}"#.to_owned()])
        .collect::<Vec<_>>()
        .join(",");
    let keys = (0..33)
        .map(|i| format!(r#""key{i:03}""#))
        .chain([r#""made""#.to_owned()])
        .collect::<Vec<_>>()
        .join(",");
    let expected = format!(r#"{{"merged":{{{members}}},"keys":[{keys}]}}"#);
    let owned = {
        let objects = (0..33)
            .map(|i| format!(r#"{{"key{i:03}":{i}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let input = format!(r#"{{"a":[{objects}],"payload":{{"n":1.00e+0}}}}"#);
        let mut workspace = jx::execution::Workspace::default();
        let value = expression
            .evaluate_with_workspace(Some(input.as_bytes()), Default::default(), &mut workspace)
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        let copy = value.clone();
        let owned = value.to_owned().unwrap();
        let raw = value
            .get("merged")
            .unwrap()
            .get("key000")
            .unwrap()
            .as_raw()
            .unwrap();
        assert_eq!(
            raw.as_bytes().as_ptr(),
            input[input.find(r#"{"n"#).unwrap()..].as_ptr()
        );
        let next = expression
            .evaluate_with_workspace(
                Some(br#"{"a":[],"payload":null}"#),
                Default::default(),
                &mut workspace,
            )
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        let mut output = Vec::new();
        next.write_compact(&mut output).unwrap();
        assert_eq!(
            output,
            br#"{"merged":{"key000":null,"made":null},"keys":["key000","made"]}"#
        );
        workspace.clear();
        drop(workspace);
        for retained in [value, copy] {
            output.clear();
            retained.write_compact(&mut output).unwrap();
            assert_eq!(output, expected.as_bytes());
        }
        owned
    };
    let mut output = Vec::new();
    owned.as_value().write_compact(&mut output).unwrap();
    assert_eq!(output, expected.replace("1.00e+0", "1").as_bytes());
    let expression = jx::compile("{'merged':$merge([$]),'keys':$keys($)}").unwrap();
    let value = expression
        .evaluate_with(
            None,
            jx::EvaluationOptions {
                focus: owned.as_value().get("merged"),
                ..Default::default()
            },
        )
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    output.clear();
    value.write_compact(&mut output).unwrap();
    assert_eq!(output, expected.replace("1.00e+0", "1").as_bytes());
}
