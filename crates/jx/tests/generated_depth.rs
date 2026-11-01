use jx::{ErrorKind, Value};
use std::process::Command;

fn nested(depth: usize, shape: &str) -> Value<'static, 'static> {
    let mut value = Value::Null;
    for level in 0..depth {
        value = if shape == "array" || (shape == "mixed" && level % 2 == 0) {
            Value::from_array(vec![value])
        } else {
            Value::from_object([("a".into(), value)])
        };
    }
    value
}

#[test]
fn bounded_stack_operations() {
    const CHILD: &str = "JX_GENERATED_DEPTH_CHILD";
    if let Ok(mode) = std::env::var(CHILD) {
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(move || match mode.as_str() {
                "evaluation" => {
                    for depth in [8192, 16384] {
                        let source = format!(
                            "($f:=function($n,$a){{$n=0?$a:$f($n-1,{{'a':$a}})}};$f({depth},null))"
                        );
                        let expression = jx::compile(&source).unwrap();
                        let error = match expression.evaluate(b"null").and_then(|v| v.single()) {
                            Err(error) => error,
                            Ok(_) => panic!("unbounded generated result"),
                        };
                        assert_eq!(error.kind, ErrorKind::EvaluationLimit);
                        assert_eq!(error.message, "value nesting limit exceeded");
                    }
                }
                "serialization" | "snapshot" | "cleanup" | "debug" => {
                    for shape in ["object", "array", "mixed"] {
                        let value = nested(16384, shape);
                        if mode == "serialization" {
                            let mut output = b"prefix".to_vec();
                            let error = value.write_compact(&mut output).unwrap_err();
                            assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
                            assert_eq!(output, b"prefix");
                        } else if mode == "debug" {
                            let formatted = format!("{value:?}");
                            assert!(formatted.len() < 100);
                            assert!(formatted.contains("nesting: 257"));
                        } else if mode == "snapshot" {
                            assert_eq!(
                                value.to_owned().unwrap_err().kind,
                                ErrorKind::EvaluationLimit
                            );
                        }
                        drop(value);
                        if mode == "cleanup" {
                            let survivor = nested(256, shape);
                            let deep = (0..16384)
                                .fold(survivor.clone(), |v, _| Value::from_array(vec![v]));
                            drop(deep);
                            survivor.write_compact(std::io::sink()).unwrap();
                            survivor.to_owned().unwrap();
                        }
                    }
                }
                "boundary" => {
                    for shape in ["object", "array", "mixed"] {
                        for depth in [0, 1, 255, 256, 257] {
                            let value = nested(depth, shape);
                            let mut output = Vec::new();
                            if depth > 256 {
                                assert!(value.write_compact(&mut output).is_err());
                                assert!(output.is_empty());
                                assert_eq!(
                                    value.to_owned().unwrap_err().kind,
                                    ErrorKind::EvaluationLimit
                                );
                                continue;
                            }
                            let mut expected = "null".to_owned();
                            for level in 0..depth {
                                expected =
                                    if shape == "array" || (shape == "mixed" && level % 2 == 0) {
                                        format!("[{expected}]")
                                    } else {
                                        format!("{{\"a\":{expected}}}")
                                    };
                            }
                            assert!(format!("{value:?}").contains("Null"));
                            value.write_compact(&mut output).unwrap();
                            assert_eq!(output, expected.as_bytes());
                            let owned = value.to_owned().unwrap();
                            drop(value);
                            output.clear();
                            owned.clone().as_value().write_compact(&mut output).unwrap();
                            assert_eq!(output, expected.as_bytes());
                        }
                    }
                }
                "traversals" => {
                    for shape in ["object", "array", "mixed"] {
                        let value = nested(256, shape);
                        for source in ["$=$", "$boolean($)", "$string($)", "$clone($)", "$keys($)"]
                        {
                            let expression = jx::compile(source).unwrap();
                            expression
                                .evaluate_with(
                                    Some(b"null"),
                                    jx::EvaluationOptions {
                                        focus: Some(value.clone()),
                                        ..Default::default()
                                    },
                                )
                                .unwrap()
                                .for_each(|v| {
                                    v.to_owned().unwrap();
                                })
                                .unwrap();
                        }
                    }
                }
                "external" => {
                    let expression = jx::compile("$boolean($)").unwrap();
                    let error = expression
                        .evaluate_with(
                            Some(b"null"),
                            jx::EvaluationOptions {
                                focus: Some(nested(16384, "mixed")),
                                ..Default::default()
                            },
                        )
                        .unwrap_err();
                    assert_eq!(error.kind, ErrorKind::EvaluationLimit);
                    let host = jx::HostFunction::new(0, |_, context| {
                        let callable = jx::HostFunction::new(1, |args, _| Ok(args[0].clone()));
                        let function = callable.value();
                        assert_eq!(
                            context
                                .invoke(&function, &[Some(nested(16384, "object"))])
                                .unwrap_err()
                                .kind,
                            ErrorKind::EvaluationLimit
                        );
                        Ok(Some(nested(16384, "array")))
                    });
                    let expression = jx::CompileOptions::default()
                        .binding("host")
                        .compile("$host()")
                        .unwrap();
                    let error = expression
                        .evaluate_with(
                            Some(b"null"),
                            jx::EvaluationOptions {
                                bindings: vec![("host", host.value())],
                                ..Default::default()
                            },
                        )
                        .unwrap_err();
                    assert_eq!(error.kind, ErrorKind::HostError);
                    assert_eq!(error.cause().unwrap().kind, ErrorKind::EvaluationLimit);
                }
                _ => panic!("unknown child operation"),
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    for mode in [
        "evaluation",
        "serialization",
        "snapshot",
        "cleanup",
        "debug",
        "boundary",
        "traversals",
        "external",
    ] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "bounded_stack_operations", "--nocapture"])
            .env(CHILD, mode)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{mode}: {:?}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn depth_errors_preserve_delivery_controls_and_workspace_ownership() {
    let expression =
        jx::compile("($f:=function($n,$a){$n=0?$a:$f($n-1,{'a':$a})};$f($,null))").unwrap();
    let mut workspace = jx::execution::Workspace::default();
    let retained = expression
        .evaluate_with_workspace(Some(b"256"), Default::default(), &mut workspace)
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    let owned = retained.to_owned().unwrap();
    for options in [
        Default::default(),
        jx::EvaluationOptions {
            limits: Some(jx::Limits::default()),
            ..Default::default()
        },
    ] {
        let error = expression
            .evaluate_with_workspace(Some(b"257"), options, &mut workspace)
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::EvaluationLimit);
        assert_eq!(error.message, "value nesting limit exceeded");
    }
    let token = jx::Cancellation::default();
    token.cancel();
    let error = expression
        .evaluate_with_workspace(
            Some(b"257"),
            jx::EvaluationOptions {
                cancellation: Some(token),
                ..Default::default()
            },
            &mut workspace,
        )
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Cancelled);
    let small = expression
        .evaluate_with_workspace(Some(b"1"), Default::default(), &mut workspace)
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    workspace.clear();
    drop(workspace);
    let mut output = Vec::new();
    small.write_compact(&mut output).unwrap();
    assert_eq!(output, br#"{"a":null}"#);
    output.clear();
    retained.write_compact(&mut output).unwrap();
    let expected = format!("{}null{}", "{\"a\":".repeat(256), "}".repeat(256));
    assert_eq!(output, expected.as_bytes());
    output.clear();
    owned.as_value().write_compact(&mut output).unwrap();
    assert_eq!(output, expected.as_bytes());

    let source = "[1,257].(($f:=function($n,$a){$n=0?$a:$f($n-1,{'a':$a})};$f($,null)))";
    let expression = jx::compile(source).unwrap();
    let program = expression.execution_program();
    let template = jx::execution::InputTemplate::new([program]);
    let mut workspace = jx::execution::Workspace::default();
    let document = template.prepare(b"null", &mut workspace).unwrap();
    let mut output = b"prefix".to_vec();
    assert!(matches!(program.write_json(&document, &mut output),
        Err(jx::ConsumeError::Evaluation(e)) if e.kind == ErrorKind::EvaluationLimit));
    assert_eq!(output, b"prefix");
    let mut seen = 0;
    let result = program.try_for_each(&document, |_| {
        seen += 1;
        Err("stop")
    });
    assert_eq!(result, Err(jx::ConsumeError::Consumer("stop")));
    assert_eq!(seen, 1);
    for (source, kind) in [
        (
            "[1,2,257].($=1?function(){0}:($f:=function($n,$a){$n=0?$a:$f($n-1,{'a':$a})};$f($,null)))",
            ErrorKind::EvaluationLimit,
        ),
        (
            "{'a':$error('first'),'b':($f:=function($n,$a){$n=0?$a:$f($n-1,{'a':$a})};$f(257,null))}",
            ErrorKind::UserError,
        ),
    ] {
        let expression = jx::compile(source).unwrap();
        let program = expression.execution_program();
        let template = jx::execution::InputTemplate::new([program]);
        let document = template.prepare(b"null", &mut workspace).unwrap();
        assert!(matches!(program.write_json(&document, &mut output),
            Err(jx::ConsumeError::Evaluation(e)) if e.kind == kind));
        assert_eq!(output, b"prefix");
    }
    let expression =
        jx::compile("($f:=function($n,$a){$n=0?$a:$f($n-1,{'a':$a})};$f(256,null))").unwrap();
    let program = expression.execution_program();
    let template = jx::execution::InputTemplate::new([program]);
    let document = template.prepare(b"null", &mut workspace).unwrap();
    assert!(
        matches!(program.write_json_with(&document, jx::EvaluationOptions {
        limits: Some(jx::Limits { max_output_bytes: 100, ..Default::default() }),
        ..Default::default()
    }, &mut output), Err(jx::ConsumeError::Evaluation(e))
        if e.kind == ErrorKind::EvaluationLimit && e.phase == jx::Phase::Serialization)
    );
    assert_eq!(output, b"prefix");
}

#[test]
fn depth_bounds_account_for_borrowed_and_snapshot_descendants() {
    let raw = jx::validate(br#"{"x":null}"#).unwrap();
    let snapshot = jx::OwnedValue::from_json(raw.as_bytes()).unwrap();
    for initial in [Value::Raw(raw), snapshot.as_value()] {
        let mut value = initial;
        for _ in 1..256 {
            value = Value::from_array(vec![value]);
        }
        let owned = value.to_owned().unwrap();
        value.write_compact(std::io::sink()).unwrap();
        assert_eq!(
            Value::from_array(vec![value]).to_owned().unwrap_err().kind,
            ErrorKind::EvaluationLimit
        );
        assert_eq!(
            Value::from_object([("a".into(), owned.as_value())])
                .to_owned()
                .unwrap_err()
                .kind,
            ErrorKind::EvaluationLimit
        );
    }
}

#[test]
fn deferred_focus_delivery_can_stop_before_a_later_depth_error() {
    let expression = jx::compile("a.{'x':{'y':{'z':{'w':v}}},'p':%.name}").unwrap();
    let focus = Value::from_object([
        (
            "a".into(),
            Value::from_array(vec![
                Value::from_object([("v".into(), Value::Null)]),
                Value::from_object([("v".into(), nested(253, "object"))]),
            ]),
        ),
        ("name".into(), Value::from_string("root")),
    ]);
    let settings = || jx::EvaluationOptions {
        focus: Some(focus.clone()),
        ..Default::default()
    };
    let evaluation = expression.evaluate_with(Some(b"null"), settings()).unwrap();
    assert_eq!(
        evaluation.try_for_each(|v| {
            let mut bytes = Vec::new();
            v.write_compact(&mut bytes).unwrap();
            assert_eq!(bytes, br#"{"x":{"y":{"z":{"w":null}}},"p":"root"}"#);
            Err("stop")
        }),
        Err(jx::ConsumeError::Consumer("stop"))
    );
    let error = expression
        .evaluate_with(Some(b"null"), settings())
        .unwrap()
        .collect_owned()
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::EvaluationLimit);
}
