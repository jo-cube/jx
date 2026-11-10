use jx::{CompileOptions, EvaluationOptions, InputDependency, OwnedValue, Value};

#[test]
fn independent_expressions_preserve_expected_results_with_any_original_input() {
    let options = CompileOptions::default()
        .binding("record")
        .constant_binding(
            "config",
            OwnedValue::from_json(br#"{"limit":2,"fallback":"default"}"#).unwrap(),
        );
    let record = br#"{"partition":2,"enabled":true,"key":"abc","text":"3","words":["hello","world"],"headers":[{"name":"trace","value":"a"},{"name":"trace","value":null},{"name":"other","value":"b"}],"rows":[{"n":3},{"n":1}],"raw":{ "n":9007199254740993, "s":"\u0061" }}"#.to_vec();
    let binding = Value::from_json(&record).unwrap();
    let cases: &[(&str, &[u8])] = &[
        ("42", b"42"),
        ("null", b"null"),
        ("[]", b"[]"),
        ("$absent", b""),
        ("$config.limit", b"2"),
        ("$record.partition", b"2"),
        ("$record.partition = $config.limit", b"true"),
        ("{'partition':$record.partition,'label':'fixed'}", br#"{"partition":2,"label":"fixed"}"#),
        ("$record.headers[name='trace'].value", br#"["a",null]"#),
        ("$record.headers[name='absent'].value", b""),
        ("$record.headers[name='other'].value", br#""b""#),
        ("$record.headers.{'name':name,'value':value}", br#"[{"name":"trace","value":"a"},{"name":"trace","value":null},{"name":"other","value":"b"}]"#),
        ("($r:=$record; $r.partition)", b"2"),
        ("($record:={'partition':9}; $record.partition)", b"9"),
        ("($config:={'limit':8}; $config.limit)", b"8"),
        ("$record.absent", b""),
        ("$number($record.text)", b"3"),
        ("$length($record.key)", b"3"),
        ("$record.headers.*", br#"["trace","a","trace",null,"other","b"]"#),
        ("($x:=$record.partition; ($x:=$x+1; $x))", b"3"),
        ("$record.headers.($length(name))", b"[5,5,5]"),
        ("$record.words.($substring(1,2))", br#"["el","or"]"#),
        ("$map($record.rows,function($v){$v.n})", b"[3,1]"),
        ("$record.enabled ? $record.key : $config.fallback", br#""abc""#),
        ("$record.raw", br#"{"n":9007199254740993,"s":"\u0061"}"#),
        ("$record.rows^(n).n", b"[1,3]"),
        ("[1..$record.partition]", b"[1,2]"),
    ];
    let inputs: &[Option<&[u8]>] = &[
        None,
        Some(b"null"),
        Some(b"7"),
        Some(br#""hello""#),
        Some(b"[]"),
        Some(br#"[{"id":1},{"id":2}]"#),
        Some(br#"{"id":"payload","partition":99,"headers":[]}"#),
        Some(&record),
    ];
    let mut output = Vec::with_capacity(512);
    for &(source, expected) in cases {
        let expression = options.compile(source).unwrap();
        assert_eq!(
            expression.input_dependency(),
            InputDependency::Independent,
            "{source}"
        );
        assert_eq!(
            expression.clone().input_dependency(),
            InputDependency::Independent
        );
        for _ in 0..2 {
            for &input in inputs {
                output.clear();
                output.extend_from_slice(b"prefix");
                let present = expression
                    .evaluate_with(
                        input,
                        EvaluationOptions {
                            bindings: vec![("record", binding.clone())],
                            ..Default::default()
                        },
                    )
                    .unwrap()
                    .write_json(&mut output)
                    .unwrap();
                assert_eq!(present, !expected.is_empty(), "{source}: {input:?}");
                assert_eq!(&output[..6], b"prefix");
                assert_eq!(&output[6..], expected, "{source}: {input:?}");
            }
        }
    }
}

#[test]
fn input_observers_and_uncertain_programs_remain_dependent() {
    let options = CompileOptions::default()
        .binding("record")
        .binding("callback");
    for source in [
        "$",
        "$$",
        "id",
        "*",
        "**",
        "$exists($$)",
        "$length()",
        "$lookup('id')",
        "$substring(1,2)",
        "($p:=$$; $p.id)",
        "$record.enabled ? $record.partition : id",
        "$record.enabled ? 1 : function(){$$}",
        "$record.enabled and $exists($$)",
        "$record.headers.($$.id)",
        "$record.headers[name=$$.wanted]",
        "($record:=id; $record.partition)",
        "function(){$record.partition}",
        "($f:=function(){$record.partition}; $f())",
        "$eval('$record.partition')",
        "$eval($record.code)",
        "$callback($record.partition)",
        "$map($record.rows,$callback)",
        "($f:=$length; $f($record.key))",
        "{$record.key:$record.partition}",
        "{'nested':{$record.key:$record.partition}}",
        "$record.rows#$i.{'index':$i,'n':n}",
        "$record ~> |$|{'added':true}|",
        "$string($record.partition)",
    ] {
        assert_eq!(
            options.compile(source).unwrap().input_dependency(),
            InputDependency::MayRequireInput,
            "{source}"
        );
    }
}

#[test]
fn external_builtin_shadowing_can_observe_the_original_root() {
    let expression = CompileOptions::default()
        .binding("length")
        .compile("$length('abc')")
        .unwrap();
    assert_eq!(
        expression.input_dependency(),
        InputDependency::MayRequireInput
    );
    let function = jx::HostFunction::new(1, |_, context| {
        Ok(Some(Value::Boolean(context.root().is_missing())))
    })
    .value();
    for (input, expected) in [
        (None, b"true".as_slice()),
        (Some(b"null".as_slice()), b"false"),
    ] {
        let mut output = Vec::new();
        expression
            .evaluate_with(
                input,
                EvaluationOptions {
                    bindings: vec![("length", function.clone())],
                    ..Default::default()
                },
            )
            .unwrap()
            .write_json(&mut output)
            .unwrap();
        assert_eq!(output, expected);
    }
}

#[test]
fn independence_does_not_disable_validation_of_supplied_input() {
    let expression = jx::compile("1").unwrap();
    assert_eq!(expression.input_dependency(), InputDependency::Independent);
    assert_eq!(
        expression.evaluate(b"not json").unwrap_err().kind,
        jx::ErrorKind::InvalidJson
    );
    let mut output = Vec::new();
    assert!(
        expression
            .evaluate_with(None, EvaluationOptions::default())
            .unwrap()
            .write_json(&mut output)
            .unwrap()
    );
    assert_eq!(output, b"1");
}

#[test]
fn independent_expressions_can_fail_without_observing_input() {
    for (source, expected) in [
        ("$number('not numeric')", jx::ErrorKind::TypeError),
        ("$error('intentional')", jx::ErrorKind::UserError),
    ] {
        let expression = jx::compile(source).unwrap();
        assert_eq!(expression.input_dependency(), InputDependency::Independent);
        for input in [None, Some(b"null".as_slice()), Some(br#"{"n":99}"#)] {
            let mut output = b"prefix".to_vec();
            let error = expression
                .evaluate_with(input, EvaluationOptions::default())
                .map_err(jx::ConsumeError::Evaluation)
                .and_then(|evaluation| evaluation.write_json(&mut output))
                .unwrap_err();
            assert!(matches!(error, jx::ConsumeError::Evaluation(error) if error.kind == expected));
            assert_eq!(output, b"prefix");
        }
    }
    let expression = CompileOptions::default()
        .binding("value")
        .compile("$value")
        .unwrap();
    assert_eq!(expression.input_dependency(), InputDependency::Independent);
    let function = jx::HostFunction::new(0, |_, _| Ok(Some(Value::Null))).value();
    let mut output = b"prefix".to_vec();
    let error = expression
        .evaluate_with(
            None,
            EvaluationOptions {
                bindings: vec![("value", function)],
                ..Default::default()
            },
        )
        .unwrap()
        .write_json(&mut output)
        .unwrap_err();
    assert!(
        matches!(error, jx::ConsumeError::Consumer(error) if error.kind() == std::io::ErrorKind::InvalidInput)
    );
    assert_eq!(output, b"prefix");
}

#[test]
fn optimized_callbacks_inherit_the_enclosing_focus() {
    let options = CompileOptions::default().binding("record");
    for source in [
        "$map($record.rows,function($v){$exists($)})",
        "$filter($record.rows,function($v){$boolean($)})",
        "{'nested':$map($record.rows,function($v){$exists($)})}",
    ] {
        let expression = options.compile(source).unwrap();
        assert_eq!(
            expression.input_dependency(),
            InputDependency::MayRequireInput,
            "{source}"
        );
    }
    let expression = options
        .compile("$map($record.rows,function($v){$exists($)})")
        .unwrap();
    for (input, expected) in [
        (None, b"[false,false]".as_slice()),
        (Some(b"null".as_slice()), b"[true,true]"),
    ] {
        let mut output = Vec::new();
        expression
            .evaluate_with(
                input,
                EvaluationOptions {
                    bindings: vec![("record", Value::from_json(br#"{"rows":[1,2]}"#).unwrap())],
                    ..Default::default()
                },
            )
            .unwrap()
            .write_json(&mut output)
            .unwrap();
        assert_eq!(output, expected);
    }
}
