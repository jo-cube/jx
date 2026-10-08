use super::*;

#[test]
fn traversal_preserves_raw_tokens_and_escaped_boundaries() {
    let tokens = [
        "null",
        "true",
        "false",
        "-0",
        "1.2300E-040",
        "1e999",
        r#""é中😀""#,
        r#""\ud800\udc00\udfff""#,
        r#""brackets: [}] and quote: \" and slash: \\""#,
        r#"{ "a": [true, {"[\\\"]}": "\\\"[}]"}], "b": [] }"#,
        "[ [], {}, [1, [2, 3]], null ]",
    ];
    let input = format!("[ \n {} \t ]", tokens.join(" ,\r\n "));
    let raw = crate::validate(input.as_bytes()).unwrap();
    assert_eq!(
        raw.elements().map(RawJson::as_str).collect::<Vec<_>>(),
        tokens
    );
    let object = format!(
        "{{ {} }}",
        tokens
            .iter()
            .enumerate()
            .map(|(i, v)| format!(r#""key{i}" : {v}"#))
            .collect::<Vec<_>>()
            .join(" , ")
    );
    let raw = crate::validate(object.as_bytes()).unwrap();
    for (i, (key, value)) in raw.members().enumerate() {
        assert_eq!(key, format!("key{i}"));
        assert_eq!(value.as_str(), tokens[i]);
        assert_eq!(raw.field(&format!("key{i}")).unwrap(), value);
    }
    assert!(raw.field("missing").is_none());
}

#[test]
fn validated_paths_and_captures_agree_with_the_input_scanner() {
    let fields = ["a", "a.x", "a.x.y", "b", "missing"]
        .map(|path| path.split('.').map(Box::<str>::from).collect::<Vec<_>>());
    let mut demand = Demand::default();
    for (slot, path) in fields.iter().enumerate() {
        demand.insert(path, slot);
    }
    for input in [
        r#"{"a":{"x":{"y":"[}]\"\\é"}},"b":[true,null,-0,1e999]}"#,
        r#"{"a":{"x":1},"\u0061":{"x":{"y":2}}}"#,
        r#"{"a":{"x":{"y":1}},"a":{},"b":""}"#,
        r#"{"a":{"x":{"y":1}},"a":[],"b":[[]]}"#,
        r#"{"a":{"x":[{"y":1},null,{"y":2}]}}"#,
        r#"[{"a":{"x":{"y":1}}},{"a":null}]"#,
        "[]",
        "{}",
        "null",
        "true",
        "-1.2e+30",
        r#""[}]\\\"""#,
    ] {
        let mut expected = Captures::default();
        let raw = capture(input.as_bytes(), &demand, &mut expected).unwrap();
        let mut actual = Captures::default();
        raw.capture(&demand, &mut actual);
        for (slot, path) in fields.iter().enumerate() {
            assert_eq!(actual.get(slot), expected.get(slot), "{input}: {path:?}");
            assert_eq!(
                raw.select(path),
                select(input.as_bytes(), path).unwrap(),
                "{input}: {path:?}"
            );
        }
        let array = format!("[ {input}, {input} ]");
        let raw = crate::validate(array.as_bytes()).unwrap();
        let mut items = raw.elements();
        for _ in 0..2 {
            assert_eq!(
                items.next_captured(&demand, &mut actual).unwrap().as_str(),
                input
            );
            for slot in 0..fields.len() {
                assert_eq!(actual.get(slot), expected.get(slot), "{input}");
            }
        }
        assert!(items.next_captured(&demand, &mut actual).is_none());
    }
}

#[test]
fn maximum_depth_traversal_and_early_stop_preserve_boundaries() {
    let nested = format!(
        "{}{{\"x\":7}}{}",
        "[".repeat(MAX_DEPTH - 2),
        "]".repeat(MAX_DEPTH - 2)
    );
    let input = format!("[{nested},9]");
    let raw = crate::validate(input.as_bytes()).unwrap();
    let mut elements = raw.elements();
    assert_eq!(elements.next().unwrap().as_str(), nested);
    assert_eq!(elements.next().unwrap().as_str(), "9");
    assert!(elements.next().is_none());
    let mut calls = 0;
    assert_eq!(
        raw.try_for_each_flattened(|value| {
            calls += 1;
            assert_eq!(value.field("x").unwrap().as_str(), "7");
            Err(())
        }),
        Err(())
    );
    assert_eq!(calls, 1);
}

#[test]
fn generated_containers_match_decoded_json_after_validation() {
    fn generated(seed: &mut u64, depth: usize) -> serde_json::Value {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        let choice = *seed;
        match choice % if depth == 0 { 4 } else { 6 } {
            0 => serde_json::Value::Null,
            1 => serde_json::json!(choice as i64),
            2 => serde_json::json!(
                ["é中😀[}]", "\"\\\n\t\0", "", "\\\\\""][(choice >> 8) as usize % 4]
            ),
            3 => serde_json::json!((choice >> 8).is_multiple_of(2)),
            4 => serde_json::Value::Array(
                (0..choice % 5)
                    .map(|_| generated(seed, depth - 1))
                    .collect(),
            ),
            _ => serde_json::Value::Object(
                (0..choice % 5)
                    .map(|i| (format!("é[{i}]\\\""), generated(seed, depth - 1)))
                    .collect(),
            ),
        }
    }
    fn compare(raw: RawJson<'_>, expected: &serde_json::Value) {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(raw.as_str()).unwrap(),
            *expected
        );
        match expected {
            serde_json::Value::Array(values) => {
                let items = raw.elements().collect::<Vec<_>>();
                assert_eq!(items.len(), values.len());
                for (item, value) in items.into_iter().zip(values) {
                    compare(item, value);
                }
            }
            serde_json::Value::Object(values) => {
                assert_eq!(raw.members().count(), values.len());
                for (key, value) in values {
                    compare(raw.field(key).unwrap(), value);
                }
                for (key, value) in raw.members() {
                    let key: String = serde_json::from_str(&format!("\"{key}\"")).unwrap();
                    compare(value, &values[&key]);
                }
            }
            _ => {}
        }
    }
    let mut seed = 0x23_c0ffee_u64;
    for _ in 0..1024 {
        let value = generated(&mut seed, 5);
        for input in [
            serde_json::to_string(&value).unwrap(),
            serde_json::to_string_pretty(&value).unwrap(),
        ] {
            compare(crate::validate(input.as_bytes()).unwrap(), &value);
        }
    }
}

#[test]
fn long_strings_preserve_escaped_and_nested_subtree_boundaries() {
    let fields = vec![Box::<str>::from("id")];
    let mut demand = Demand::default();
    demand.insert(&fields, 0);
    for length in (0..40).chain([127, 128, 129, 1023, 1024, 1025]) {
        let padding = "x".repeat(length);
        for body in [
            "",
            "short",
            "é",
            "plain ASCII",
            "é中😀",
            r#"\"\\\/\b\f\n\r\t\u0000"#,
            r#"\ud800\udc00\udfff"#,
            r#"[}]\"\\\"\\\\"#,
        ] {
            let token = format!(r#""{padding}{body}{padding}""#);
            let subtree = format!(
                r#"{{"text":{token},"nested":[{token},{{"text":{token}}}],"n":-1.2300e+4}}"#
            );
            let input = format!(r#"[{token},{subtree},{token},true,null]"#);
            let raw = crate::validate(input.as_bytes()).unwrap();
            assert_eq!(
                raw.elements().map(RawJson::as_str).collect::<Vec<_>>(),
                [
                    token.as_str(),
                    subtree.as_str(),
                    token.as_str(),
                    "true",
                    "null"
                ]
            );
            let input = format!(r#"{{"id":1,"ignored":{subtree},"\u0069d":2,"tail":{token}}}"#);
            let raw = crate::validate(input.as_bytes()).unwrap();
            assert_eq!(raw.field("id").unwrap().as_str(), "2");
            assert_eq!(raw.field("ignored").unwrap().as_str(), subtree);
            assert_eq!(raw.field("tail").unwrap().as_str(), token);
            assert_eq!(
                raw.select(&fields),
                select(input.as_bytes(), &fields).unwrap()
            );
            let mut expected = Captures::default();
            capture(input.as_bytes(), &demand, &mut expected).unwrap();
            let mut actual = Captures::default();
            raw.capture(&demand, &mut actual);
            assert_eq!(actual.get(0), expected.get(0));
        }
    }
}
