use jx::{ConsumeError, Error, Evaluation, EvaluationOptions, InputPlan, Value};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn outcome(result: Result<Evaluation<'_, '_>, Error>) -> Result<Vec<Vec<u8>>, Error> {
    let mut items = Vec::new();
    result?.for_each(|value| {
        let mut bytes = Vec::new();
        match value {
            Value::Number(n) => bytes.extend(n.to_bits().to_le_bytes()),
            value => value.write_compact(&mut bytes).unwrap(),
        }
        items.push(bytes);
    })?;
    Ok(items)
}

#[test]
fn shared_paths_preserve_values_cardinality_and_exact_errors() {
    let expressions = [
        "a",
        "a.x",
        "b",
        "a = b",
        "a.x = b.x",
        "a+b",
        "(a+b)*(a-b)",
        "$string(a)",
        "$number(a)",
        "a ? b : a.x",
        "a and b",
        "a or b",
        "a ?? b",
        "$number(a) ?? $number(b)",
        "a ?: b",
        "$exists(a) ? a : b",
        "a.x ?? b.x",
        "a ?? (b ?? a.x)",
        "{'a':a,'x':a.x,'b':b}",
        "[a,b,a.x]",
        "a&':'&$string(b)&':'&a",
        "$lookup({'x':1,'undefined':2},a)",
        "$$.a",
        "rows.a",
        "$sum(rows.a)",
        "function($x){$x}(a)",
        "($x:=a; $x)",
    ]
    .map(|s| jx::compile(s).unwrap());
    let plan = InputPlan::new(&expressions);
    let atoms = [
        "null",
        "false",
        "true",
        "0",
        "-0",
        "2",
        "1e999",
        "[]",
        "[1]",
        "[[1],[2]]",
        "{}",
        r#"{"x":2}"#,
        r#""x""#,
        r#""\ud800""#,
    ];
    for a in atoms {
        for b in atoms {
            let object = format!(r#"{{"a":{a},"b":{b},"rows":[{{"a":{a}}},{{"a":{b}}}]}}"#);
            for input in [
                object.clone(),
                format!("[{object}]"),
                format!("[{object},{object}]"),
                format!(r#"{{"a":{a},"b":{b},"\u0061":null}}"#),
                format!(r#"{{"b":{b}}}"#),
            ] {
                let raw = jx::validate(input.as_bytes()).unwrap();
                let prepared = plan.prepare(input.as_bytes()).unwrap();
                let validated = plan.prepare_validated(raw);
                for (index, expression) in expressions.iter().enumerate() {
                    let expected = outcome(expression.evaluate_validated(raw));
                    assert_eq!(expected, outcome(expression.evaluate(input.as_bytes())));
                    assert_eq!(
                        outcome(prepared.evaluate(index)),
                        expected,
                        "{index}: {input}"
                    );
                    assert_eq!(
                        outcome(validated.evaluate(index)),
                        expected,
                        "{index}: {input}"
                    );
                }
            }
        }
    }
}

#[test]
fn duplicate_parents_replace_missing_and_deferred_descendants() {
    let expressions = ["a.x", "a.y", "a.x = b", "b", "a"].map(|s| jx::compile(s).unwrap());
    let plan = InputPlan::new(&expressions);
    for input in [
        r#"{"a":{"x":1,"y":2},"a":{"x":3},"b":3}"#,
        r#"{"a":[{"x":1}],"\u0061":{"y":2},"b":3}"#,
        r#"{"a":{"x":1},"a":[{"x":[2,3]},{"x":4}],"b":3}"#,
        r#"{"a":{"x":1},"a":null,"b":3}"#,
        r#"{"a":{"x":1,"x":2,"\u0078":3},"b":3}"#,
    ] {
        let prepared = plan.prepare(input.as_bytes()).unwrap();
        for (index, expression) in expressions.iter().enumerate() {
            assert_eq!(
                outcome(prepared.evaluate(index)),
                outcome(expression.evaluate(input.as_bytes())),
                "{input}"
            );
        }
    }
}

#[test]
fn lazy_failures_and_consumer_termination_keep_their_original_timing() {
    let expressions = ["id = 1", "rows[$number(x)>0].x"].map(|s| jx::compile(s).unwrap());
    let plan = InputPlan::new(&expressions);
    let input = br#"{"id":1,"rows":[{"x":"1"},{"x":"bad"}]}"#;
    let prepared = plan.prepare(input).unwrap();
    let mut seen = Vec::new();
    let error = prepared
        .evaluate(1)
        .unwrap()
        .for_each(|v| seen.push(v.as_str().unwrap().unwrap().into_owned()))
        .unwrap_err();
    assert_eq!(seen, ["1"]);
    assert_eq!(error, outcome(expressions[1].evaluate(input)).unwrap_err());
    let mut calls = 0;
    assert_eq!(
        prepared.evaluate(1).unwrap().try_for_each(|_| {
            calls += 1;
            Err("stop")
        }),
        Err(ConsumeError::Consumer("stop"))
    );
    assert_eq!(calls, 1);
    assert_eq!(
        prepared
            .evaluate(0)
            .unwrap()
            .single()
            .unwrap()
            .unwrap()
            .as_bool(),
        Some(true)
    );
}

#[test]
fn preparation_validates_unused_input_before_effects_or_expression_errors() {
    let expressions = [
        jx::compile("a = 1").unwrap(),
        jx::compile("$error('stop')").unwrap(),
    ];
    let plan = InputPlan::new(&expressions);
    for input in [
        b"{\"a\":1,\"unused\":[1,]}".as_slice(),
        b"{\"a\":1} trailing",
        b"{\"a\":1,\"unused\":\"\\z\"}",
        b"{\"a\":1,\"unused\":\"\xff\"}",
    ] {
        let error = match plan.prepare(input) {
            Ok(_) => panic!("invalid input accepted"),
            Err(e) => e,
        };
        assert_eq!(error, jx::validate(input).unwrap_err());
    }
    let empty = InputPlan::new([]);
    assert!(empty.prepare(b"[1,]").is_err());
}

#[test]
fn each_evaluation_has_independent_bindings_randomness_and_effects() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let host = jx::HostFunction::new(0, move |_, _| {
        Ok(Some(Value::Number(
            count.fetch_add(1, Ordering::SeqCst) as f64
        )))
    });
    let host = host.value();
    let expressions = [
        jx::compile("id = 1").unwrap(),
        jx::compile("($x:=id; $x)").unwrap(),
        jx::compile("$x").unwrap(),
        jx::compile("$random()").unwrap(),
        jx::CompileOptions::default()
            .binding("next")
            .compile("$next()")
            .unwrap(),
    ];
    let plan = InputPlan::new(&expressions);
    let prepared = plan.prepare(br#"{"id":1}"#).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for _ in 0..2 {
        assert_eq!(
            prepared
                .evaluate(1)
                .unwrap()
                .single()
                .unwrap()
                .unwrap()
                .as_number(),
            Some(1.)
        );
        assert!(prepared.evaluate(2).unwrap().single().unwrap().is_none());
        let options = || EvaluationOptions {
            random: Some(jx::Random::seeded(9)),
            ..Default::default()
        };
        assert_eq!(
            outcome(prepared.evaluate_with(3, options())),
            outcome(expressions[3].evaluate_with(Some(br#"{"id":1}"#), options()))
        );
    }
    for expected in 0..2 {
        assert_eq!(
            prepared
                .evaluate_with(
                    4,
                    EvaluationOptions {
                        bindings: vec![("next", host.clone())],
                        ..Default::default()
                    }
                )
                .unwrap()
                .single()
                .unwrap()
                .unwrap()
                .as_number(),
            Some(f64::from(expected))
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn focus_bindings_and_controls_use_each_expressions_existing_contract() {
    let expressions = [
        jx::CompileOptions::default()
            .binding("scale")
            .compile("a*$scale")
            .unwrap(),
        jx::compile("a").unwrap(),
    ];
    let plan = InputPlan::new(&expressions);
    let input = br#"{"a":2}"#;
    let prepared = plan.prepare(input).unwrap();
    let options = || EvaluationOptions {
        bindings: vec![("scale", Value::Number(3.))],
        ..Default::default()
    };
    assert_eq!(
        outcome(prepared.evaluate_with(0, options())),
        outcome(expressions[0].evaluate_with(Some(input), options()))
    );
    let options = || EvaluationOptions {
        focus: Some(Value::from_json(br#"{"a":7}"#).unwrap()),
        ..Default::default()
    };
    assert_eq!(
        outcome(prepared.evaluate_with(1, options())),
        outcome(expressions[1].evaluate_with(Some(input), options()))
    );
    let cancelled = jx::Cancellation::default();
    cancelled.cancel();
    let options = || EvaluationOptions {
        cancellation: Some(cancelled.clone()),
        ..Default::default()
    };
    assert_eq!(
        outcome(prepared.evaluate_with(1, options())),
        outcome(expressions[1].evaluate_with(Some(input), options()))
    );
    assert_eq!(
        prepared
            .evaluate(1)
            .unwrap()
            .single()
            .unwrap()
            .unwrap()
            .as_number(),
        Some(2.)
    );
}

#[test]
fn validated_root_outlives_preparation_without_copying_or_normalization() {
    let expressions = [jx::compile("a").unwrap()];
    let input = br#"  { "a":1, "a":2, "unused": [3, 4] }  "#;
    let expected = jx::validate(input).unwrap();
    for validated in [false, true] {
        let raw = {
            let plan = InputPlan::new(&expressions);
            let prepared = if validated {
                plan.prepare_validated(expected)
            } else {
                plan.prepare(input).unwrap()
            };
            prepared.as_raw()
        };
        assert_eq!(raw, expected);
        assert_eq!(raw.as_bytes().as_ptr(), expected.as_bytes().as_ptr());
        let mut output = Vec::new();
        raw.write_compact(&mut output).unwrap();
        assert_eq!(output, br#"{"a":1,"a":2,"unused":[3,4]}"#);
    }
}

#[test]
fn results_outlive_preparation_and_keep_original_borrowing() {
    let expressions = [jx::compile("a").unwrap(), jx::compile("{'v':a}").unwrap()];
    let input = br#"{"a": { "untouched": 1 }}"#;
    let (evaluation, value) = {
        let plan = InputPlan::new(&expressions);
        let prepared = plan.prepare(input).unwrap();
        (
            prepared.evaluate(0).unwrap(),
            prepared.evaluate(1).unwrap().single().unwrap().unwrap(),
        )
    };
    let raw = evaluation.single().unwrap().unwrap().as_raw().unwrap();
    assert_eq!(raw.as_str(), r#"{ "untouched": 1 }"#);
    assert_eq!(raw.as_bytes().as_ptr(), input[6..].as_ptr());
    assert_eq!(value.get("v").unwrap().as_raw().unwrap(), raw);
}

#[test]
fn excess_demands_fall_back_without_changing_expression_order() {
    let expressions = (0..40)
        .map(|i| jx::compile(&format!("field{i}")).unwrap())
        .collect::<Vec<_>>();
    let input = format!(
        "{{{}}}",
        (0..40)
            .map(|i| format!("\"field{i}\":{i}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let plan = InputPlan::new(&expressions);
    let prepared = plan.prepare(input.as_bytes()).unwrap();
    for i in (0..40).rev() {
        assert_eq!(
            prepared
                .evaluate(i)
                .unwrap()
                .single()
                .unwrap()
                .unwrap()
                .as_number(),
            Some(i as f64)
        );
    }
}

#[cfg(feature = "jit")]
#[test]
fn shared_numeric_plans_preserve_native_execution_and_guard_fallback() {
    let mut expressions = [
        jx::compile("(a+b)*(a-b)").unwrap(),
        jx::compile("a+b").unwrap(),
        jx::compile("a=2").unwrap(),
    ];
    assert!(expressions[0].enable_native().kernels > 0);
    let plan = InputPlan::new(&expressions);
    for input in [
        r#"{"a":2,"b":3}"#,
        r#"{"a":null,"b":3}"#,
        r#"{"a":[1],"b":2}"#,
        r#"[{"a":2,"b":3}]"#,
    ] {
        let prepared = plan.prepare(input.as_bytes()).unwrap();
        for (index, expression) in expressions.iter().enumerate() {
            assert_eq!(
                outcome(prepared.evaluate(index)),
                outcome(expression.evaluate(input.as_bytes()))
            );
        }
    }
}

#[test]
fn immutable_lookup_fallbacks_preserve_missing_and_lazy_errors() {
    let options = jx::CompileOptions::default().constant_binding(
        "config",
        jx::OwnedValue::from_json(br#"{"map":{"acme-prod":7,"backup":8}}"#).unwrap(),
    );
    let expressions = [
        "$lookup($config.map,key) ?? $lookup($config.map,fallback)",
        "$lookup($config.map,a&'-'&b) ?? $lookup($config.map,fallback)",
        "flag ? ($lookup($config.map,key) ?? $lookup($config.map,fallback)) : 9",
        "$lookup($config.map,key) ?? $number(fallback)",
        "$lookup($config.map,key) ?? $error('untaken')",
        "$number(key) ?? $error('fallback')",
    ]
    .map(|source| options.compile(source).unwrap());
    let plan = InputPlan::new(&expressions);
    for (input, expected) in [
        (
            br#"{"key":"acme-prod","a":"acme","b":"prod","fallback":"invalid","flag":true}"#
                .as_slice(),
            Some(7.),
        ),
        (
            br#"{"key":"missing","a":"missing","b":"prod","fallback":"backup","flag":true}"#,
            Some(8.),
        ),
        (
            br#"{"key":"missing","a":"missing","b":"prod","fallback":"absent","flag":true}"#,
            None,
        ),
    ] {
        let prepared = plan.prepare(input).unwrap();
        for index in 0..3 {
            assert_eq!(
                prepared
                    .evaluate(index)
                    .unwrap()
                    .single()
                    .unwrap()
                    .and_then(|v| v.as_number()),
                expected
            );
        }
        for (index, expression) in expressions.iter().enumerate() {
            assert_eq!(
                outcome(prepared.evaluate(index)),
                outcome(expression.evaluate(input))
            );
        }
    }
    let input = br#"{"key":"acme-prod","fallback":"invalid","flag":false}"#;
    let prepared = plan.prepare(input).unwrap();
    assert_eq!(
        prepared
            .evaluate(2)
            .unwrap()
            .single()
            .unwrap()
            .unwrap()
            .as_number(),
        Some(9.)
    );
    for index in [3, 4] {
        assert_eq!(
            prepared
                .evaluate(index)
                .unwrap()
                .single()
                .unwrap()
                .unwrap()
                .as_number(),
            Some(7.)
        );
    }
    assert_eq!(
        outcome(prepared.evaluate(5)).unwrap_err().kind,
        jx::ErrorKind::TypeError
    );
}

#[test]
fn missing_fallbacks_keep_primary_reexecution_effect_order_and_exists_shadowing() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let next = jx::HostFunction::new(0, move |_, _| {
        Ok(Some(Value::Number(
            count.fetch_add(1, Ordering::SeqCst) as f64
        )))
    })
    .value();
    let expressions = [
        jx::CompileOptions::default()
            .binding("next")
            .compile("$next() ?? $error('untaken')")
            .unwrap(),
        jx::compile("($exists:=function($v){false}; a ?? b)").unwrap(),
        jx::compile("($exists:=function($v){true}; missing ?? b)").unwrap(),
        jx::compile("($x:=a ?? b; $x)").unwrap(),
        jx::compile("$x ?? b").unwrap(),
        jx::compile("a ?? b").unwrap(),
    ];
    let plan = InputPlan::new(&expressions);
    let prepared = plan.prepare(br#"{"a":1,"b":2}"#).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    for expected in [1., 3.] {
        let options = EvaluationOptions {
            bindings: vec![("next", next.clone())],
            ..Default::default()
        };
        assert_eq!(
            prepared
                .evaluate_with(0, options)
                .unwrap()
                .single()
                .unwrap()
                .unwrap()
                .as_number(),
            Some(expected)
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    for _ in 0..2 {
        for (index, expected) in [
            (1, Some(2.)),
            (2, None),
            (3, Some(1.)),
            (4, Some(2.)),
            (5, Some(1.)),
        ] {
            assert_eq!(
                prepared
                    .evaluate(index)
                    .unwrap()
                    .single()
                    .unwrap()
                    .and_then(|v| v.as_number()),
                expected
            );
        }
    }
}
