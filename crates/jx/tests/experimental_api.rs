#![cfg(feature = "experimental")]

use jx::{
    CompileOptions, ConsumeError, ErrorKind, EvaluationOptions, OwnedValue, execution::Acquisition,
};

fn owned(e: jx::Evaluation<'_, '_>) -> String {
    format!("{:?}", e.collect_owned().unwrap())
}

#[test]
fn reuse_cloning_and_shared_inputs_keep_new_callable_state_record_local() {
    let options = CompileOptions::default()
        .constant_binding("table", OwnedValue::from_json(br#"{"a":2}"#).unwrap());
    let sources = [
        "{'matches':$match(text,/[0-9]+/),'rate':$lookup($table,id)}",
        "$ ~> |items|{'v':v+1}|",
        "($make:=$eval('function($x){function($y){$x+$y}}');$make(2)(3))",
    ];
    let programs = sources.map(|s| options.compile_program(s).unwrap());
    let clones = programs.clone();
    let references = sources.map(|s| options.compile(s).unwrap());
    let mut acquisition = Acquisition::new(programs.iter());
    let mut cloned_acquisition = acquisition.clone();
    for input in [
        br#"{"text":"a12 b34","id":"a","items":[{"v":1},{"v":2}]}"#.as_slice(),
        br#"{}"#,
        br#"{"text":"x5","id":"missing","items":[{"v":3}]}"#,
        br#"{"text":"a12 b34","id":"a","items":[{"v":1},{"v":2}]}"#,
    ] {
        let raw = jx::validate(input).unwrap();
        let document = acquisition.prepare(input).unwrap();
        let validated = cloned_acquisition.prepare_validated(raw);
        for ((program, clone), reference) in programs.iter().zip(&clones).zip(&references) {
            let expected = owned(reference.evaluate(input).unwrap());
            for actual in [
                program.evaluate(&document),
                clone.evaluate(&document),
                clone.evaluate(&validated),
                program.evaluate_validated(raw),
                program.evaluate_bytes(input),
            ] {
                assert_eq!(owned(actual.unwrap()), expected);
            }
            let detached = program
                .evaluate(&document)
                .unwrap()
                .collect_owned()
                .unwrap();
            assert_eq!(format!("{detached:?}"), expected);
        }
    }
    assert_eq!(
        acquisition
            .prepare(br#"{"text":1} trailing"#)
            .unwrap_err()
            .kind,
        ErrorKind::InvalidJson
    );
    let document = acquisition.prepare(br#"{}"#).unwrap();
    assert_eq!(
        owned(programs[0].evaluate(&document).unwrap()),
        owned(references[0].evaluate(b"{}").unwrap())
    );
}

#[test]
fn dynamic_closures_remain_local_and_failures_preserve_cancellation_order() {
    let producer = CompileOptions::default()
        .compile_program("$eval('function($x){$x+1}')")
        .unwrap();
    let function = producer
        .evaluate_without_input()
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    assert!(function.to_owned().is_err());
    let consumer = CompileOptions::default()
        .binding("f")
        .compile_program("$f(2)")
        .unwrap();
    assert_eq!(
        consumer
            .evaluate_with(
                None,
                EvaluationOptions {
                    bindings: vec![("f", function)],
                    ..Default::default()
                }
            )
            .unwrap_err()
            .kind,
        ErrorKind::BindingError
    );
    let cancellation = jx::Cancellation::default();
    cancellation.cancel();
    let settings = || EvaluationOptions {
        cancellation: Some(cancellation.clone()),
        ..Default::default()
    };
    assert_eq!(
        producer
            .evaluate_with(Some(b"invalid"), settings())
            .unwrap_err()
            .kind,
        ErrorKind::InvalidJson
    );
    assert_eq!(
        producer
            .evaluate_with(Some(b"{}"), settings())
            .unwrap_err()
            .kind,
        ErrorKind::Cancelled
    );
}

#[test]
fn new_operations_preserve_consumer_prefixes_and_eager_error_boundaries() {
    for source in [
        "items.($replace(text,/[0-9]+/,'X'))",
        "items.($formatNumber(n,'0.0'))",
        "items.($eval(source))",
        "items.($ ~> |$|{'n':n+1}|)",
    ] {
        let options = CompileOptions::default();
        let old = options.compile(source).unwrap();
        let program = options.compile_program(source).unwrap();
        for input in [br#"{"items":[{"text":"a1","n":1,"source":"n+1"},{"text":"b2","n":2,"source":"n+1"}]}"#.as_slice(), br#"{"items":[{"text":"a1","n":1,"source":"n+1"},{"text":null,"n":null,"source":"$error('later')"}]}"#] {
            let consume = |result: Result<jx::Evaluation<'_, '_>, jx::Error>| {
                let mut prefix = Vec::new();
                let stopped = result.map_err(ConsumeError::Evaluation).and_then(|e| e.try_for_each(|v| { prefix.push(v.to_owned().unwrap()); Err("stop") }));
                (format!("{prefix:?}"), stopped)
            };
            let expected = consume(old.evaluate(input));
            assert_eq!(consume(program.evaluate_bytes(input)), expected, "{source}");
            let mut acquisition = Acquisition::new([&program]);
            let doc = acquisition.prepare(input).unwrap();
            assert_eq!(consume(program.evaluate(&doc)), expected, "{source}");
        }
    }
}

#[test]
fn dynamic_syntax_errors_keep_reference_phase_and_source() {
    let source = "$eval(source)";
    let options = CompileOptions::default();
    let program = options.compile_program(source).unwrap();
    let reference = options.compile(source).unwrap();
    for input in [
        br#"{"source":"(a+b).%"}"#.as_slice(),
        br#"{"source":"$eval('(a+b).%')"}"#,
    ] {
        let error = program
            .evaluate_bytes(input)
            .and_then(|e| e.collect_owned())
            .unwrap_err();
        let expected = reference
            .evaluate(input)
            .and_then(|e| e.collect_owned())
            .unwrap_err();
        assert_eq!(error, expected);
        assert_eq!(error.phase, jx::Phase::Evaluation);
        assert_eq!(error.source, jx::Source::Expression);
        assert_eq!(error.cause().unwrap().source, jx::Source::DynamicExpression);
    }
}

#[test]
fn contextual_inputs_reuse_settings_clones_and_terminal_delivery() {
    let options = CompileOptions::default()
        .binding("scale")
        .constant_binding("table", OwnedValue::from_json(br#"{"a":"A"}"#).unwrap());
    let sources = [
        "groups#$g.items#$i.{'g':$g,'i':$i,'name':%.name,'v':v*$scale,'label':$lookup($table,id)}",
        "groups.items.($p:=%;function(){$p.name}) ~> $map(function($f){$f()})",
        r#"$eval('groups.items[%.name="a"].v')"#,
    ];
    let programs = sources.map(|q| options.compile_program(q).unwrap());
    let references = sources.map(|q| options.compile(q).unwrap());
    let mut acquisition = Acquisition::new(programs.iter());
    for input in [
        br#"{}"#.as_slice(),
        br#"{"groups":[{"name":"a","items":[{"v":1,"id":"a"},{"v":2}]}]}"#,
        br#"{"groups":[{"name":"b","items":[{"v":3}]}]}"#,
    ] {
        let raw = jx::validate(input).unwrap();
        let doc = acquisition.prepare_validated(raw);
        let settings = || EvaluationOptions {
            bindings: vec![("scale", jx::Value::Number(2.0))],
            ..Default::default()
        };
        for (p, r) in programs.iter().zip(&references) {
            let expected = owned(r.evaluate_with(Some(input), settings()).unwrap());
            assert_eq!(
                owned(p.evaluate_with(Some(input), settings()).unwrap()),
                expected
            );
            assert_eq!(
                owned(p.evaluate_validated_with(Some(raw), settings()).unwrap()),
                expected
            );
            assert_eq!(
                owned(p.clone().evaluate_prepared_with(&doc, settings()).unwrap()),
                expected
            );
            let mut values = Vec::new();
            p.try_for_each_with(&doc, settings(), |v| {
                values.push(v.to_owned().unwrap());
                Ok::<_, ()>(())
            })
            .unwrap();
            assert_eq!(format!("{values:?}"), expected);
            let mut bytes = b"prefix".to_vec();
            let present = p.write_json_with(&doc, settings(), &mut bytes).unwrap();
            let detached = values
                .iter()
                .map(|v| {
                    serde_json::from_slice::<serde_json::Value>(&{
                        let mut b = Vec::new();
                        v.as_value().write_compact(&mut b).unwrap();
                        b
                    })
                    .unwrap()
                })
                .collect::<Vec<_>>();
            assert_eq!(present, !detached.is_empty());
            if present {
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&bytes[6..]).unwrap(),
                    if detached.len() == 1 {
                        detached[0].clone()
                    } else {
                        serde_json::Value::Array(detached)
                    }
                );
            } else {
                assert_eq!(bytes, b"prefix");
            }
        }
    }
}
#[test]
fn tuple_cancellation_and_error_precedence_match_the_reference() {
    let options = CompileOptions::default();
    for source in [
        "items#$i.(v)",
        "items#$i.($i=0?v:$error('later'))",
        "items#$i[$i=1?$error('predicate'):true].($error('mapper'))",
        "(items#$i).($i)",
    ] {
        let p = options.compile_program(source).unwrap();
        let r = options.compile(source).unwrap();
        let input = br#"{"items":[{"v":1},{"v":2},{"v":3}]}"#;
        let mut a = Acquisition::new([&p]);
        let doc = a.prepare(input).unwrap();
        let consume = |e: Result<jx::Evaluation<'_, '_>, jx::Error>| {
            let mut values = Vec::new();
            let result = e.map_err(ConsumeError::Evaluation).and_then(|e| {
                e.try_for_each(|v| {
                    values.push(v.to_owned().unwrap());
                    Err("stop")
                })
            });
            (format!("{values:?}"), result)
        };
        assert_eq!(
            consume(p.evaluate_bytes(input)),
            consume(r.evaluate(input)),
            "{source}"
        );
        let mut prefix = Vec::new();
        let stopped = p.try_for_each(&doc, |v| {
            prefix.push(v.to_owned().unwrap());
            Err("stop")
        });
        assert_eq!(
            (format!("{prefix:?}"), stopped),
            consume(r.evaluate(input)),
            "prepared direct: {source}"
        );
        assert_eq!(
            consume(p.evaluate_prepared_with(&doc, Default::default())),
            consume(r.evaluate_with(Some(input), Default::default())),
            "{source}"
        );
        let cancellation = jx::Cancellation::default();
        let settings = || EvaluationOptions {
            cancellation: Some(cancellation.clone()),
            ..Default::default()
        };
        let expected = consume(r.evaluate_with(Some(input), settings()));
        let actual = consume(p.evaluate_prepared_with(&doc, settings()));
        assert_eq!(actual, expected, "{source}");
        cancellation.cancel();
        let mut output = b"prefix".to_vec();
        assert!(
            matches!(p.write_json_with(&doc,settings(),&mut output),Err(ConsumeError::Evaluation(e)) if e.kind==ErrorKind::Cancelled)
        );
        assert_eq!(output, b"prefix");
    }
}
#[cfg(feature = "jit")]
#[test]
fn native_enabled_reference_compiles_to_the_same_opt_in_ir() {
    for source in ["n*2+1", "items#$i.{'i':$i,'v':v}", "items.v"] {
        let mut reference = jx::compile(source).unwrap();
        reference.enable_native();
        let p = jx::execution::Program::compile(&reference).unwrap();
        for input in [br#"{}"#.as_slice(), br#"{"n":3,"items":[{"v":2},{"v":4}]}"#] {
            assert_eq!(
                owned(p.evaluate_bytes(input).unwrap()),
                owned(reference.evaluate(input).unwrap())
            );
        }
    }
}

#[test]
fn contextual_focus_and_seeded_delivery_preserve_root_and_prefixes() {
    let source = "items#$i.{'i':$i,'root':$$.name,'v':v}";
    let options = CompileOptions::default();
    let p = options.compile_program(source).unwrap();
    let r = options.compile(source).unwrap();
    let root = br#"{"name":"root"}"#;
    let focus = OwnedValue::from_json(br#"{"items":[{"v":1},{"v":2}]}"#).unwrap();
    let settings = || EvaluationOptions {
        focus: Some(focus.as_value()),
        ..Default::default()
    };
    let mut acquisition = Acquisition::new([&p]);
    let doc = acquisition.prepare(root).unwrap();
    assert_eq!(
        owned(p.evaluate_prepared_with(&doc, settings()).unwrap()),
        owned(r.evaluate_with(Some(root), settings()).unwrap())
    );
    let source = "items#$i.($i=0?$random():$error('later'))";
    let p = options.compile_program(source).unwrap();
    let r = options.compile(source).unwrap();
    let input = br#"{"items":[1,2]}"#;
    let mut a = Acquisition::new([&p]);
    let doc = a.prepare(input).unwrap();
    for controlled in [false, true] {
        let settings = || EvaluationOptions {
            random: Some(jx::Random::seeded(42)),
            cancellation: controlled.then(jx::Cancellation::default),
            ..Default::default()
        };
        let consume = |e: Result<jx::Evaluation<'_, '_>, jx::Error>| {
            let mut prefix = Vec::new();
            let result = e.map_err(ConsumeError::Evaluation).and_then(|e| {
                e.try_for_each(|v| {
                    prefix.push(v.as_number());
                    Err("stop")
                })
            });
            (prefix, result)
        };
        assert_eq!(
            consume(p.evaluate_prepared_with(&doc, settings())),
            consume(r.evaluate_with(Some(input), settings()))
        );
    }
}

#[test]
fn contextual_retention_charges_intermediate_items_before_collection() {
    let p = CompileOptions::default()
        .compile_program("items#$i[-1]")
        .unwrap();
    let input = format!("{{\"items\":[{}]}}", vec!["1"; 128].join(","));
    let error = p
        .evaluate_with(
            Some(input.as_bytes()),
            EvaluationOptions {
                limits: Some(jx::Limits {
                    max_items: 3,
                    ..Default::default()
                }),
                ..Default::default()
            },
        )
        .and_then(|e| e.collect_owned())
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::EvaluationLimit);
    assert!(error.message.contains("item"));
}

#[test]
fn immediate_callback_ports_preserve_runtime_bindings_and_eager_failures() {
    let options = CompileOptions::default().binding("scale");
    let source = "($unused:=function(){1};$map(items,function($v){$v.n*$scale}))";
    let reference = options.compile(source).unwrap();
    let program = options.compile_program(source).unwrap();
    let mut acquisition = Acquisition::new([&program]);
    for input in [
        br#"{}"#.as_slice(),
        br#"{"items":[]}"#,
        br#"{"items":[{"n":2},{"n":3}]}"#,
        br#"{"items":[{"n":2},{"n":null}]}"#,
    ] {
        for scale in [2.0, 3.0] {
            let settings = || EvaluationOptions {
                bindings: vec![("scale", jx::Value::Number(scale))],
                ..Default::default()
            };
            let expected = reference
                .evaluate_with(Some(input), settings())
                .and_then(|e| e.collect_owned());
            let document = acquisition.prepare(input).unwrap();
            let actual = program
                .evaluate_prepared_with(&document, settings())
                .and_then(|e| e.collect_owned());
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
            let expected = reference
                .evaluate_with(Some(input), settings())
                .map_err(ConsumeError::Evaluation)
                .and_then(|e| e.try_for_each(|_| Err("stop")));
            assert_eq!(
                program.try_for_each_with(&document, settings(), |_| Err("stop")),
                expected
            );
        }
    }
}
