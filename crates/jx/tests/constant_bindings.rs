use jx::{CompileOptions, EvaluationOptions, Expression, OwnedValue, Value};

fn binding() -> OwnedValue {
    OwnedValue::from_json(br#"{"skip":"never","label":"retained","n":3,"nested":{"n":4},"rows":[{"n":5},{"n":6}],"empty":[],"null":null}"#).unwrap()
}
fn compile(source: &str) -> Expression {
    CompileOptions::default()
        .constant_binding("config", binding())
        .compile(source)
        .unwrap()
}
fn output(evaluation: jx::Evaluation<'_, '_>) -> Result<Vec<String>, jx::Error> {
    let mut values = Vec::new();
    evaluation.for_each(|value| {
        let mut bytes = Vec::new();
        value.write_compact(&mut bytes).unwrap();
        values.push(String::from_utf8(bytes).unwrap());
    })?;
    Ok(values)
}

#[test]
fn immutable_data_access_matches_runtime_bindings() {
    let config = binding();
    for source in [
        "$config",
        "$config.skip",
        "$config.missing",
        "$lookup($config, 'null')",
        "$config.empty",
        "$config.nested.n",
        "$config.rows.n",
        "$config.rows[0].n",
        "$lookup($config, 'label')",
        "$lookup($config, 'missing')",
        "$lookup($config.nested, 'n')",
        "$lookup($config, key)",
        "id=$config.skip",
        "{'id':id,'label':$config.label}",
        "[$config.rows]",
        "$config.rows[]",
        "$config.rows.n[]",
        "$config.rows[n>2].n",
        "items.($config.rows[n>2].n)",
        "$map(items,function($x){$config.rows[n>2].n})",
        "($config.rows).n",
        "items.($config.rows).n",
    ] {
        let constant = CompileOptions::default()
            .constant_binding("config", config.clone())
            .compile(source)
            .unwrap();
        let runtime = CompileOptions::default()
            .binding("config")
            .compile(source)
            .unwrap();
        for input in [
            br#"{"id":"x","key":"label"}"#.as_slice(),
            br#"{"id":"never","key":"missing"}"#,
            b"null",
            b"[]",
            br#"{"items":[[],[1,2],[[3]],{}]}"#,
        ] {
            let expected = output(
                runtime
                    .evaluate_with(
                        Some(input),
                        EvaluationOptions {
                            bindings: vec![("config", config.as_value())],
                            ..Default::default()
                        },
                    )
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(
                output(constant.evaluate(input).unwrap()).unwrap(),
                expected,
                "{source}"
            );
            assert_eq!(
                output(
                    constant
                        .evaluate_validated(jx::validate(input).unwrap())
                        .unwrap()
                )
                .unwrap(),
                expected,
                "{source}"
            );
        }
    }
}

#[test]
fn scalar_bindings_and_missing_are_owned() {
    for (value, expected) in [(Value::Number(3.), vec!["6"]), (Value::Undefined, vec![])] {
        let expression = CompileOptions::default()
            .constant_binding("n", value.to_owned().unwrap())
            .compile("$n*2")
            .unwrap();
        assert_eq!(
            output(expression.evaluate(b"null").unwrap()).unwrap(),
            expected
        );
    }
}

#[test]
fn local_assignments_and_parameters_shadow_constants_without_leaking() {
    for (source, expected) in [
        ("($config := {'n':9}; $config.n)", "9"),
        (
            "[$config.n, ($config := {'n':9}; $config.n), $config.n]",
            "[3,9,3]",
        ),
        (
            "($f := function($config){$config.n}; [$f({'n':8}),$config.n])",
            "[8,3]",
        ),
        (
            "($f := function(){ $config.n }; $config := {'n':9}; $f())",
            "9",
        ),
        ("($config := $config.n+1; $config)", "4"),
        ("[$map($config.rows,function($config){$config.n})]", "[5,6]"),
    ] {
        let expression = compile(source);
        for _ in 0..3 {
            assert_eq!(
                output(expression.evaluate(b"null").unwrap()).unwrap(),
                [expected],
                "{source}"
            );
        }
    }
}

#[test]
fn closures_and_tuple_bindings_keep_their_lexical_semantics() {
    for (source, expected) in [
        ("($f:=function($n){$n+$config.n}; $f(2))", "5"),
        ("($f:=function(){function(){ $config.n }}; $f()())", "3"),
        ("[$config.rows@$config.($config.n)]", "[5,6]"),
        ("[$config.rows#$config.($config)]", "[0,1]"),
    ] {
        assert_eq!(
            output(compile(source).evaluate(b"null").unwrap()).unwrap(),
            [expected],
            "{source}"
        );
    }
}

#[test]
fn dynamic_evaluation_inherits_and_can_shadow_the_constant_environment() {
    for (source, input, expected) in [
        ("$eval('$config.n')", "null", "3"),
        ("$eval(code)", r#"{"code":"$config.n"}"#, "3"),
        ("($config:={'n':9}; $eval('$config.n'))", "null", "9"),
        ("($eval('$config := 7'); $config)", "null", "7"),
        ("($eval(code); $config)", r#"{"code":"$config := 7"}"#, "7"),
        ("($f:=$eval('function(){ $config.n }'); $f())", "null", "3"),
    ] {
        assert_eq!(
            output(compile(source).evaluate(input.as_bytes()).unwrap()).unwrap(),
            [expected],
            "{source}"
        );
    }
}

#[test]
fn runtime_bindings_coexist_but_cannot_replace_constants() {
    let expression = CompileOptions::default()
        .constant_binding("config", binding())
        .binding("scale")
        .compile("$config.n*$scale")
        .unwrap();
    assert_eq!(
        output(
            expression
                .evaluate_with(
                    None,
                    EvaluationOptions {
                        bindings: vec![("scale", Value::Number(2.))],
                        ..Default::default()
                    }
                )
                .unwrap()
        )
        .unwrap(),
        ["6"]
    );
    let error = expression
        .evaluate_with(
            Some(b"null"),
            EvaluationOptions {
                bindings: vec![("config", Value::Number(2.))],
                ..Default::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::BindingError);
    for options in [
        CompileOptions::default()
            .constant_binding("config", binding())
            .binding("config"),
        CompileOptions::default()
            .constant_binding("config", binding())
            .constant_binding("config", binding()),
        CompileOptions::default().constant_binding("$config", binding()),
        CompileOptions::default().constant_binding("", binding()),
    ] {
        let error = options.compile("$config").unwrap_err();
        assert_eq!(error.kind, jx::ErrorKind::BindingError);
        assert_eq!(error.phase, jx::Phase::Compilation);
    }
}

#[test]
fn shared_owned_binding_and_compiled_expressions_are_thread_safe() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<CompileOptions>();
    send_sync::<Expression>();
    let config = std::sync::Arc::new(binding());
    let options = CompileOptions::default().constant_binding("config", config.clone());
    let expressions = [
        options.compile("id=$config.skip").unwrap(),
        options.compile("{'id':id,'label':$config.label}").unwrap(),
    ];
    drop(options);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                let plan = jx::InputPlan::new(&expressions);
                let input = plan.prepare(br#"{"id":"x"}"#).unwrap();
                assert_eq!(output(input.evaluate(0).unwrap()).unwrap(), ["false"]);
                assert_eq!(
                    output(input.evaluate(1).unwrap()).unwrap(),
                    [r#"{"id":"x","label":"retained"}"#]
                );
            });
        }
    });
}

#[test]
fn validation_and_lazy_failures_preserve_diagnostics() {
    let constant = compile("items.($config.n + n)");
    let runtime = CompileOptions::default()
        .binding("config")
        .compile("items.($config.n + n)")
        .unwrap();
    let config = binding();
    let input = br#"{"items":[{"n":1},{"n":"bad"}]}"#;
    let expected = runtime
        .evaluate_with(
            Some(input),
            EvaluationOptions {
                bindings: vec![("config", config.as_value())],
                ..Default::default()
            },
        )
        .and_then(output)
        .unwrap_err();
    let actual = output(constant.evaluate(input).unwrap()).unwrap_err();
    assert_eq!(actual, expected);
    let mut delivered = Vec::new();
    let failure = constant
        .evaluate(input)
        .unwrap()
        .for_each(|value| {
            delivered.push(value.as_number().unwrap());
        })
        .unwrap_err();
    assert_eq!(delivered, [4.]);
    assert_eq!(failure, expected);
    let literal = jx::compile("items.(3+n)").unwrap();
    assert_eq!(
        literal.evaluate(input).and_then(output).unwrap_err().kind,
        expected.kind
    );
    assert_eq!(
        constant.evaluate(br#"{"items":[0,]}"#).unwrap_err(),
        jx::validate(br#"{"items":[0,]}"#).unwrap_err()
    );
    let result = constant
        .evaluate(input)
        .unwrap()
        .try_for_each(|_| Err("stop"));
    assert_eq!(result, Err(jx::ConsumeError::Consumer("stop")));
}

#[test]
fn immutable_objects_keep_duplicate_key_and_utf16_semantics() {
    let config = OwnedValue::from_json(
        br#"{"k":1,"\u006b":2,"s":"\ud800","nested":[[1],[2]],"undefined":7}"#,
    )
    .unwrap();
    let constants = CompileOptions::default().constant_binding("config", config.clone());
    let runtime = CompileOptions::default().binding("config");
    for source in [
        "$config.k",
        "$lookup($config,'k')",
        "$config.s",
        "$config.nested[]",
        "$lookup($config,missing)",
    ] {
        let expected = output(
            runtime
                .compile(source)
                .unwrap()
                .evaluate_with(
                    Some(b"null"),
                    EvaluationOptions {
                        bindings: vec![("config", config.as_value())],
                        ..Default::default()
                    },
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            output(
                constants
                    .compile(source)
                    .unwrap()
                    .evaluate(b"null")
                    .unwrap()
            )
            .unwrap(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn transforms_and_dynamic_functions_do_not_mutate_constant_data() {
    let config = std::sync::Arc::new(binding());
    let constants = CompileOptions::default().constant_binding("config", config.clone());
    let runtime = CompileOptions::default().binding("config");
    for source in [
        "$config ~> |rows|{'n':n+1}|",
        "[$config ~> |rows|{'n':n+1}|, $config.rows.n]",
        "($f:=$eval(code); $f())",
    ] {
        let expected = runtime.compile(source).unwrap();
        let expression = constants.compile(source).unwrap();
        let input = br#"{"code":"function(){ $config.rows.n }"}"#;
        for _ in 0..2 {
            assert_eq!(
                output(expression.evaluate(input).unwrap()).unwrap(),
                output(
                    expected
                        .evaluate_with(
                            Some(input),
                            EvaluationOptions {
                                bindings: vec![("config", config.as_value())],
                                ..Default::default()
                            }
                        )
                        .unwrap()
                )
                .unwrap(),
                "{source}"
            );
        }
    }
    assert_eq!(config.as_value().get("n").unwrap().as_number(), Some(3.));
}

#[test]
fn declared_constant_names_shadow_builtins_even_when_missing() {
    for value in [Value::Undefined, Value::Number(3.)] {
        let constant = CompileOptions::default()
            .constant_binding("sum", value.to_owned().unwrap())
            .compile("$sum([1,2])")
            .unwrap();
        let runtime = CompileOptions::default()
            .binding("sum")
            .compile("$sum([1,2])")
            .unwrap();
        let expected = runtime
            .evaluate_with(
                Some(b"null"),
                EvaluationOptions {
                    bindings: vec![("sum", value)],
                    ..Default::default()
                },
            )
            .and_then(output)
            .unwrap_err();
        assert_eq!(
            constant.evaluate(b"null").and_then(output).unwrap_err(),
            expected
        );
    }
}

#[cfg(feature = "jit")]
#[test]
fn native_constants_and_guard_fallback_preserve_results_and_errors() {
    let mut expression = compile("price*$config.n+quantity>5");
    assert!(expression.enable_native().kernels > 0, "{expression:#?}");
    let control = compile("price*$config.n+quantity>5");
    for input in [
        br#"{"price":2,"quantity":4}"#.as_slice(),
        br#"{"price":"bad","quantity":4}"#,
        b"[]",
    ] {
        assert_eq!(
            expression.evaluate(input).and_then(output),
            control.evaluate(input).and_then(output)
        );
    }
}

#[test]
fn dynamically_returned_binding_keeps_its_owned_storage() {
    let config = std::sync::Arc::new(binding());
    let expression = CompileOptions::default()
        .constant_binding("config", config.clone())
        .compile("$eval(code)")
        .unwrap();
    let value = expression
        .evaluate(br#"{"code":"$config"}"#)
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    let original = config.as_value().get("skip").unwrap();
    let returned = value.get("skip").unwrap();
    assert_eq!(
        returned.as_str().unwrap().unwrap().as_ptr(),
        original.as_str().unwrap().unwrap().as_ptr()
    );
}

#[test]
fn dynamic_lookup_guard_failures_retain_source_diagnostics() {
    let constant = compile("$lookup($config,key)");
    let runtime = CompileOptions::default()
        .binding("config")
        .compile("$lookup($config,key)")
        .unwrap();
    let config = binding();
    for input in [
        br#"{"key":1}"#.as_slice(),
        br#"{"key":null}"#,
        br#"{"key":{}}"#,
        br#"{"key":[1,"n"]}"#,
    ] {
        let expected = runtime
            .evaluate_with(
                Some(input),
                EvaluationOptions {
                    bindings: vec![("config", config.as_value())],
                    ..Default::default()
                },
            )
            .and_then(output)
            .unwrap_err();
        assert_eq!(
            constant.evaluate(input).and_then(output).unwrap_err(),
            expected
        );
    }
}
