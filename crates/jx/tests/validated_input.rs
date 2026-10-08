use jx::{CompileOptions, Error, ErrorKind, Evaluation, EvaluationOptions, Expression, Value};

fn items(evaluation: Result<Evaluation<'_, '_>, Error>) -> Result<Vec<String>, Error> {
    let mut items = Vec::new();
    evaluation?.for_each(|value| {
        let mut bytes = Vec::new();
        value.write_compact(&mut bytes).unwrap();
        items.push(String::from_utf8(bytes).unwrap());
    })?;
    Ok(items)
}

fn parity(expression: &Expression, input: &[u8]) {
    let raw = jx::validate(input).unwrap();
    let expected = items(expression.evaluate(input));
    assert_eq!(items(expression.evaluate_validated(raw)), expected);
    assert_eq!(
        items(expression.evaluate_validated_with(Some(raw), EvaluationOptions::default())),
        expected
    );
}

#[test]
fn one_validated_input_serves_independent_scalar_and_path_expressions() {
    let input = br#" {"a":1,"a":2,"nested":{"n":9007199254740993},"label":"a\u0062"} "#;
    let raw = jx::validate(input).unwrap();
    for (source, expected) in [
        ("a", vec!["2"]),
        ("nested.n", vec!["9007199254740993"]),
        ("label", vec![r#""a\u0062""#]),
        ("missing", vec![]),
        ("7", vec!["7"]),
        ("$number(a)", vec!["2"]),
        ("$lookup({'hit':7},label)", vec![]),
    ] {
        let expression = jx::compile(source).unwrap();
        assert_eq!(items(expression.evaluate_validated(raw)).unwrap(), expected);
        parity(&expression, input);
    }
    let nested = Value::Raw(raw).get("nested").unwrap().as_raw().unwrap();
    assert_eq!(
        items(jx::compile("n").unwrap().evaluate_validated(nested)).unwrap(),
        ["9007199254740993"]
    );
}

#[test]
fn captured_and_planned_expressions_preserve_results_and_fallback_errors() {
    for source in [
        "((a+b)*(a-b)+(a*a+b*b))/(a+1)-b*3",
        "{'raw':a,'n':a*a+b*b,'label':label}",
        "a>b and b>0 ? a : b",
        "label & label & label",
        "$sum(rows[n>0].(n*n+n+n))",
        "function($x,$y){$x+$y}(a,b)",
        "function($x,$y){[$x,$y]}(a.n,b.n)",
        "$lookup({'hit':7},label)",
        "$number(a)",
    ] {
        let expression = jx::compile(source).unwrap();
        let check = |expression: &Expression| {
            for input in [
                r#"{"a":7,"b":3,"label":"hit","rows":[{"n":2},{"n":3}]}"#,
                r#"{"a":null,"b":"x","label":false,"rows":[{"n":null}]}"#,
                r#"{"a":[1,2],"b":1,"label":"hit","rows":[]}"#,
                r#"{"a":[{"n":1},{"n":2}],"b":{"n":3},"label":"miss"}"#,
                r#"[{"a":7,"b":3},{"a":2,"b":1}]"#,
                r#"{"a":9007199254740993,"b":1,"a":2,"label":"a\u0062"}"#,
                "{}",
                "null",
            ] {
                parity(expression, input.as_bytes());
            }
        };
        check(&expression);
        #[cfg(feature = "jit")]
        {
            let mut native = expression.clone();
            native.enable_native();
            check(&native);
        }
    }
}

#[test]
fn missing_arrays_and_sequences_keep_their_distinct_cardinality() {
    let input = br#"{"rows":[{"n":1},{"n":2}]}"#;
    for (source, expected) in [
        ("missing", vec![]),
        ("$", vec![r#"{"rows":[{"n":1},{"n":2}]}"#]),
        ("rows.n", vec!["1", "2"]),
        ("[rows.n]", vec!["[1,2]"]),
        ("rows[n>1].n", vec!["2"]),
        ("[1..3]", vec!["[1,2,3]"]),
    ] {
        let expression = jx::compile(source).unwrap();
        assert_eq!(
            items(expression.evaluate_validated(jx::validate(input).unwrap())).unwrap(),
            expected
        );
        parity(&expression, input);
    }
}

#[test]
fn evaluation_errors_and_unreached_branches_match() {
    let input = br#"{"a":2}"#;
    for source in ["a+null", "$error('stop')", "$number('bad')"] {
        let expression = jx::compile(source).unwrap();
        let error = expression.evaluate(input).unwrap_err();
        assert_eq!(error.phase, jx::Phase::Evaluation);
        assert_eq!(
            expression
                .evaluate_validated(jx::validate(input).unwrap())
                .unwrap_err(),
            error
        );
        parity(&expression, input);
    }
    let expression = jx::compile("false ? $error('unreached') : a").unwrap();
    parity(&expression, input);
    assert_eq!(
        items(expression.evaluate_validated(jx::validate(input).unwrap())).unwrap(),
        ["2"]
    );
}

#[test]
fn lazy_failures_follow_prior_results_and_consumers_can_stop_before_them() {
    let expression = jx::compile("rows.(n*n+n+n)").unwrap();
    let input = br#"{"rows":[{"n":2},{"n":null}]}"#;
    let raw = jx::validate(input).unwrap();
    let consume = |evaluation: Evaluation<'_, '_>| {
        let mut seen = Vec::new();
        let error = evaluation
            .for_each(|value| seen.push(value.as_number().unwrap()))
            .unwrap_err();
        (seen, error)
    };
    let expected = consume(expression.evaluate(input).unwrap());
    assert_eq!(expected.0, [8.]);
    assert_eq!(expected.1.kind, ErrorKind::TypeError);
    assert_eq!(
        consume(expression.evaluate_validated(raw).unwrap()),
        expected
    );
    assert_eq!(
        expression
            .evaluate_validated(raw)
            .unwrap()
            .try_for_each(|_| Err(7)),
        Err(jx::ConsumeError::Consumer(7))
    );
    let raw = jx::validate(br#"{"rows":[{"n":2},{"n":3},{"n":null}]}"#).unwrap();
    assert_eq!(
        expression
            .evaluate_validated(raw)
            .unwrap()
            .single()
            .unwrap_err()
            .kind,
        ErrorKind::CardinalityError
    );
}

#[test]
fn options_preserve_bindings_focus_absent_input_and_binding_errors() {
    let expression = CompileOptions::default()
        .binding("v")
        .compile("[$v.n,$,$$]")
        .unwrap();
    let input = b"3";
    let options = || EvaluationOptions {
        bindings: vec![("v", Value::from_json(br#"{"n":7}"#).unwrap())],
        focus: Some(Value::Number(9.)),
        ..Default::default()
    };
    for input in [Some(input.as_slice()), None] {
        let expected = items(expression.evaluate_with(input, options()));
        assert_eq!(
            items(
                expression
                    .evaluate_validated_with(input.map(|v| jx::validate(v).unwrap()), options())
            ),
            expected
        );
    }
    let expression = jx::compile("[$exists($),$exists($$)]").unwrap();
    assert_eq!(
        items(expression.evaluate_validated_with(None, EvaluationOptions::default())).unwrap(),
        ["[false,false]"]
    );
    let expression = jx::compile("$v").unwrap();
    assert_eq!(
        items(expression.evaluate_validated_with(Some(jx::validate(input).unwrap()), options())),
        items(expression.evaluate_with(Some(input), options()))
    );
}

#[test]
fn random_sources_and_controls_keep_options_semantics() {
    let input = br#"{"a":2}"#;
    let raw = jx::validate(input).unwrap();
    for source in ["$random()+a", "function($x){$random()+$x}(a)", "a*a+a+a"] {
        let expression = jx::compile(source).unwrap();
        let first = jx::Random::seeded(9);
        let second = jx::Random::seeded(9);
        for _ in 0..8 {
            assert_eq!(
                items(expression.evaluate_with(
                    Some(input),
                    EvaluationOptions {
                        random: Some(first.clone()),
                        ..Default::default()
                    }
                )),
                items(expression.evaluate_validated_with(
                    Some(raw),
                    EvaluationOptions {
                        random: Some(second.clone()),
                        ..Default::default()
                    }
                ))
            );
        }
    }
    let expression = jx::compile("rows.n").unwrap();
    let input = br#"{"rows":[{"n":1},{"n":2}]}"#;
    let options = || EvaluationOptions {
        limits: Some(jx::Limits {
            max_results: 1,
            ..Default::default()
        }),
        ..Default::default()
    };
    let consume = |evaluation: Evaluation<'_, '_>| {
        let mut seen = Vec::new();
        let error = evaluation
            .for_each(|v| seen.push(v.as_number().unwrap()))
            .unwrap_err();
        (seen, error)
    };
    let expected = consume(expression.evaluate_with(Some(input), options()).unwrap());
    assert_eq!(expected.0, [1.]);
    assert_eq!(expected.1.kind, ErrorKind::EvaluationLimit);
    assert_eq!(
        consume(
            expression
                .evaluate_validated_with(Some(jx::validate(input).unwrap()), options())
                .unwrap()
        ),
        expected
    );
}

#[test]
fn validated_results_borrow_source_bytes_and_owned_snapshots_detach() {
    let expression = jx::compile("{'raw':n,'computed':n+1}").unwrap();
    let owned = {
        let input = br#"{"n":9007199254740993}"#.to_vec();
        let value = expression
            .evaluate_validated(jx::validate(&input).unwrap())
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        let raw = value.get("raw").unwrap().as_raw().unwrap();
        assert_eq!(raw.as_bytes().as_ptr(), input[5..].as_ptr());
        assert_eq!(raw.as_str(), "9007199254740993");
        value.to_owned().unwrap()
    };
    assert_eq!(
        owned.as_value().get("raw").unwrap().as_number(),
        Some(9007199254740992.)
    );
}

#[test]
fn malformed_input_is_rejected_at_the_validation_boundary() {
    for input in [
        b"{\"a\":1,\"ignored\":[0,]}".as_slice(),
        b"1 trailing",
        b"\"\xff\"",
    ] {
        let error = jx::validate(input).unwrap_err();
        assert_eq!(error.phase, jx::Phase::Validation);
        assert_eq!(error.source, jx::Source::Input);
        for source in ["a", "1", "a+a+a+a", "$error('unreached')"] {
            let expression = jx::compile(source).unwrap();
            assert_eq!(expression.evaluate(input).unwrap_err(), error);
        }
    }
}
