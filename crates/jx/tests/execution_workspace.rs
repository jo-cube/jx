use jx::{CompileOptions, EvaluationOptions, execution::Workspace};

fn json(e: jx::Evaluation<'_, '_>) -> Vec<u8> {
    format!("{:?}", e.collect_owned().unwrap()).into_bytes()
}

#[test]
fn ordinary_facade_workspace_and_independent_preparations_agree() {
    let options = CompileOptions::default();
    let sources = [
        "{'a':a,'b':b,'sum':a+b}",
        "items.v",
        "$sum(items[v>0].(v*2))",
    ];
    let expressions = sources.map(|s| options.compile(s).unwrap());
    let references = sources.map(|s| options.compile(s).unwrap());
    let plan = jx::InputPlan::new(&expressions);
    let mut workspace = Workspace::default();
    for input in [
        br#"{"a":2,"b":3,"items":[{"v":1},{"v":2}]}"#.as_slice(),
        b" \t\r\n{\"a\":2,\"b\":3,\"items\":[{\"v\":1}]} \n",
        b"{}",
        b"[]",
        b"null",
    ] {
        let prepared = plan.prepare(input).unwrap();
        let validated = plan.prepare_validated(jx::validate(input).unwrap());
        for (i, (e, r)) in expressions.iter().zip(&references).enumerate() {
            let expected = json(r.evaluate(input).unwrap());
            assert_eq!(json(e.evaluate(input).unwrap()), expected);
            assert_eq!(
                json(
                    e.evaluate_with(
                        Some(input),
                        EvaluationOptions {
                            focus: Some(jx::Value::Raw(validated.as_raw())),
                            ..Default::default()
                        }
                    )
                    .unwrap()
                ),
                expected
            );
            assert_eq!(
                json(e.evaluate_validated(validated.as_raw()).unwrap()),
                expected
            );
            assert_eq!(
                json(
                    e.evaluate_with_workspace(
                        Some(input),
                        EvaluationOptions::default(),
                        &mut workspace
                    )
                    .unwrap()
                ),
                expected
            );
            assert_eq!(json(prepared.evaluate(i).unwrap()), expected);
            assert_eq!(json(validated.evaluate(i).unwrap()), expected);
        }
    }
}

#[test]
fn deferred_values_borrow_input_and_survive_workspace_reuse_and_drop() {
    let e = CompileOptions::default().compile("items.v").unwrap();
    let input = br#"{"items":[{"v":{"x":1}},{"v":{"x":2}}]}"#;
    let pending = {
        let mut w = Workspace::default();
        let pending = e
            .evaluate_with_workspace(Some(input), Default::default(), &mut w)
            .unwrap();
        assert!(w.retained_bytes() <= 128);
        assert_eq!(
            json(
                e.evaluate_with_workspace(Some(b"{}"), Default::default(), &mut w)
                    .unwrap()
            ),
            b"[]"
        );
        w.clear();
        assert_eq!(w.retained_bytes(), 0);
        pending
    };
    assert_eq!(
        json(pending),
        json(jx::compile("items.v").unwrap().evaluate(input).unwrap())
    );
    let raw_expression = CompileOptions::default().compile("payload").unwrap();
    let input = br#"{"payload":{"x":1}}"#;
    let mut w = Workspace::default();
    let value = raw_expression
        .evaluate_with_workspace(Some(input), Default::default(), &mut w)
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    let bytes = value.as_raw().unwrap().as_bytes();
    assert_eq!(bytes.as_ptr(), input[11..].as_ptr());
    let owned = {
        let input = input.to_vec();
        raw_expression
            .evaluate(&input)
            .unwrap()
            .collect_owned()
            .unwrap()
    };
    assert_eq!(owned.len(), 1);
}

#[test]
fn workspace_releases_failure_state_and_keeps_validation_and_effect_order() {
    use jx::{ErrorKind, Value};
    let options = CompileOptions::default().binding("host");
    let e = options.compile("$host(a)").unwrap();
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    let host = jx::HostFunction::new(1, move |args, _| {
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(args[0].clone())
    });
    let settings = || EvaluationOptions {
        bindings: vec![("host", host.value())],
        ..Default::default()
    };
    let mut w = Workspace::default();
    assert_eq!(
        e.evaluate_with_workspace(Some(br#"{"a":1} trailing"#), settings(), &mut w)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidJson
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert_eq!(
        json(
            e.evaluate_with_workspace(Some(b"{}"), settings(), &mut w)
                .unwrap()
        ),
        b"[]"
    );
    assert_eq!(
        json(
            e.evaluate_with_workspace(Some(br#"{"a":2}"#), settings(), &mut w)
                .unwrap()
        ),
        b"[OwnedValue { data: Number(2.0) }]"
    );
    let cancelled = jx::Cancellation::default();
    cancelled.cancel();
    let settings = || EvaluationOptions {
        cancellation: Some(cancelled.clone()),
        bindings: vec![("host", Value::Null)],
        ..Default::default()
    };
    assert_eq!(
        e.evaluate_with_workspace(Some(b"invalid"), settings(), &mut w)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidJson
    );
    assert_eq!(
        e.evaluate_with_workspace(Some(b"{}"), settings(), &mut w)
            .unwrap_err()
            .kind,
        ErrorKind::Cancelled
    );
    let failing = CompileOptions::default()
        .compile("items.($error('failed'))")
        .unwrap();
    assert!(
        failing
            .evaluate_with_workspace(Some(br#"{"items":[1]}"#), Default::default(), &mut w)
            .and_then(|e| e.collect_owned())
            .is_err()
    );
    assert_eq!(
        json(
            e.evaluate_with_workspace(
                None,
                EvaluationOptions {
                    bindings: vec![("host", host.value())],
                    ..Default::default()
                },
                &mut w
            )
            .unwrap()
        ),
        b"[]"
    );
}

#[test]
fn programs_templates_and_clones_are_concurrent_but_scratch_is_exclusive() {
    fn share<T: Send + Sync>() {}
    share::<jx::Expression>();
    share::<jx::execution::Program>();
    share::<jx::execution::InputTemplate>();
    share::<Workspace>();
    let options = CompileOptions::default()
        .constant_binding("table", jx::OwnedValue::from_json(br#"{"a":7}"#).unwrap());
    let e = options
        .compile("{'x':v+v,'label':$lookup($table,id)}")
        .unwrap();
    let e2 = e.clone();
    let template = e.execution_program().input_template().clone();
    std::thread::scope(|scope| {
        for e in [&e, &e2] {
            let template = &template;
            scope.spawn(move || {
                let mut w = Workspace::default();
                for n in 0..64 {
                    let input = format!("{{\"id\":\"a\",\"v\":{n}}}");
                    let expected = json(e.evaluate(input.as_bytes()).unwrap());
                    assert_eq!(
                        json(
                            e.evaluate_with_workspace(
                                Some(input.as_bytes()),
                                Default::default(),
                                &mut w
                            )
                            .unwrap()
                        ),
                        expected
                    );
                    let d = template.prepare(input.as_bytes(), &mut w).unwrap();
                    assert_eq!(json(e.execution_program().evaluate(&d).unwrap()), expected);
                }
            });
        }
    });
}

#[test]
fn mixed_shared_plan_settings_focus_and_cancellation_preserve_delivery() {
    let options = CompileOptions::default().binding("scale");
    let queries = [
        "items#$i.{'i':$i,'parent':%.name,'v':v*$scale}",
        "items.v",
        "$eval(source)",
    ];
    let expressions = [
        options.compile(queries[0]).unwrap(),
        options.compile(queries[1]).unwrap(),
        options.compile(queries[2]).unwrap(),
    ];
    let references = queries.map(|q| options.compile(q).unwrap());
    let plan = jx::InputPlan::new(&expressions);
    let input = br#"{"name":"root","source":"items.v","items":[{"v":1},{"v":2}]}"#;
    let settings = || EvaluationOptions {
        bindings: vec![("scale", jx::Value::Number(3.0))],
        ..Default::default()
    };
    let d1 = plan.prepare(input).unwrap();
    let d2 = plan.prepare(b"{}").unwrap();
    for (i, reference) in references.iter().enumerate() {
        assert_eq!(
            json(d1.evaluate_with(i, settings()).unwrap()),
            json(reference.evaluate_with(Some(input), settings()).unwrap())
        );
        assert_eq!(
            json(d2.evaluate_with(i, settings()).unwrap()),
            json(reference.evaluate_with(Some(b"{}"), settings()).unwrap())
        );
    }
    let e = options
        .compile("items.($exists(v)?v:$error('later'))")
        .unwrap();
    let r = options
        .compile("items.($exists(v)?v:$error('later'))")
        .unwrap();
    let input = br#"{"items":[{"v":1},{}]}"#;
    let consume = |result: Result<jx::Evaluation<'_, '_>, jx::Error>| {
        let mut values = Vec::new();
        let stopped = result.map_err(jx::ConsumeError::Evaluation).and_then(|e| {
            e.try_for_each(|v| {
                values.push(format!("{v:?}"));
                Err("stop")
            })
        });
        (values, stopped)
    };
    assert_eq!(consume(e.evaluate(input)), consume(r.evaluate(input)));
    let focus = jx::OwnedValue::from_json(br#"{"items":[{"v":9}]}"#).unwrap();
    let settings = || EvaluationOptions {
        focus: Some(focus.as_value()),
        ..Default::default()
    };
    assert_eq!(
        json(e.evaluate_with(Some(input), settings()).unwrap()),
        json(r.evaluate_with(Some(input), settings()).unwrap())
    );
}

#[test]
fn facade_preserves_root_array_paths_tail_focus_shadowable_fallbacks_and_budgets() {
    for source in [
        "$.a",
        "$.a[]",
        "($f:=function(){$abs()};n.$f())",
        "($exists:=function($v){false}; a ?? b)",
    ] {
        let e = jx::compile(source).unwrap();
        let r = jx::compile(source).unwrap();
        for input in [br#"[{"a":[1]}]"#.as_slice(), br#"{"a":1,"b":2,"n":7}"#] {
            let actual = e.evaluate(input).and_then(|e| e.collect_owned());
            let expected = r.evaluate(input).and_then(|e| e.collect_owned());
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
        }
    }
    for source in ["$sum(rows.a)", "$sum(rows)"] {
        let e = jx::compile(source).unwrap();
        let input = br#"{"rows":[{"a":1},{"a":2}]}"#;
        let result = e
            .evaluate_with(
                Some(input),
                EvaluationOptions {
                    limits: Some(jx::Limits {
                        max_items: 1,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )
            .and_then(|e| e.collect_owned());
        assert_eq!(result.unwrap_err().kind, jx::ErrorKind::EvaluationLimit);
    }
}

#[test]
fn preserved_filter_candidates_are_not_normalized_before_construction() {
    // General predicates transport native candidates; literal positions and
    // parenthesized value boundaries normalize before postfix preservation.
    for source in [
        "($f:=function(){$undefined};$string([$f()[true][]]))",
        "($f:=function(){$undefined};$string([$f()[0][]]))",
        "($f:=function(){$undefined};$string([$f()[false][]]))",
        "($f:=function(){$undefined};$count($f()[true][]))",
        "($f:=function(){$undefined};$p:=function(){true};$string([$f()[$p()][]]))",
        "($f:=function(){$undefined};$p:=function(){0};$string([$f()[$p()][]]))",
        "($f:=function(){$undefined};$p:=function(){true};$string([$f()[$p()][true][]]))",
        "($f:=function(){$undefined};$p:=function(){true};$string([$f()[$p()][0][]]))",
        "$string([missing[true][]])",
        "$string([function(){1},0/0,missing[true][]])",
        "(missing[true])[]",
        "items[true][]",
    ] {
        let e = jx::compile(source).unwrap();
        let r = jx::compile(source).unwrap();
        for input in [
            b"{}".as_slice(),
            br#"{"items":[[1],[2]]}"#,
            br#"{"items":[]}"#,
        ] {
            assert_eq!(
                json(e.evaluate(input).unwrap()),
                json(r.evaluate(input).unwrap()),
                "{source}"
            );
        }
    }
}

#[test]
fn workspace_heap_tracks_demands_and_never_record_size() {
    let query = format!(
        "{{{}}}",
        (0..40)
            .map(|i| format!("'v{i}':metrics.v{i}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let e = jx::compile(&query).unwrap();
    let mut workspace = Workspace::default();
    assert_eq!(workspace.retained_bytes(), 0);
    let small = br#"{"metrics":{"v0":1}}"#;
    let expected = json(
        e.evaluate_with_workspace(Some(small), Default::default(), &mut workspace)
            .unwrap(),
    );
    let capacity = workspace.retained_bytes();
    assert!(capacity > 128 && capacity < 2048);
    let large = format!(
        "{{\"metrics\":{{\"v0\":1}},\"unused\":\"{}\"}}",
        "x".repeat(1024 * 1024)
    );
    assert_eq!(
        json(
            e.evaluate_with_workspace(Some(large.as_bytes()), Default::default(), &mut workspace)
                .unwrap()
        ),
        expected
    );
    assert_eq!(workspace.retained_bytes(), capacity);
    assert!(
        e.evaluate_with_workspace(Some(b"invalid"), Default::default(), &mut workspace)
            .is_err()
    );
    assert_eq!(workspace.retained_bytes(), capacity);
    workspace.clear();
    assert_eq!(workspace.retained_bytes(), 0);
}

#[test]
fn joined_strings_own_their_bytes_across_input_drop_and_workspace_reuse() {
    let expression = jx::compile("$join([items[0],$lowercase(items[1]),'literal'],sep)").unwrap();
    let mut workspace = Workspace::default();
    for (input, expected) in [
        (r#"{"items":["é","B"],"sep":"|"}"#, r#""é|b|literal""#),
        (
            r#"{"items":["\ud800","B"],"sep":"|"}"#,
            r#""\ud800|b|literal""#,
        ),
    ] {
        let (string, snapshot) = {
            let input = input.as_bytes().to_vec();
            let value = expression
                .evaluate_with_workspace(Some(&input), Default::default(), &mut workspace)
                .unwrap()
                .single()
                .unwrap()
                .unwrap();
            let snapshot = value.to_owned().unwrap();
            let jx::Value::String(string) = value else {
                panic!("join must produce an owned string")
            };
            (string.clone(), snapshot)
        };
        expression
            .evaluate_with_workspace(
                Some(br#"{"items":["X","Y"],"sep":""}"#),
                Default::default(),
                &mut workspace,
            )
            .unwrap()
            .single()
            .unwrap();
        workspace.clear();
        for value in [jx::Value::String(string), snapshot.as_value()] {
            let mut output = Vec::new();
            value.write_compact(&mut output).unwrap();
            assert_eq!(output, expected.as_bytes());
        }
    }
}
