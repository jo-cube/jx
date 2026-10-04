#[path = "support/embedding.rs"]
mod common;
use common::{binding, items};
use jx::CompileOptions;
use jx::{ErrorKind, EvaluationOptions, Limits, Value};
#[test]
fn limits_stop_retention_and_consumption_and_validation_still_precedes_effects() {
    for source in [
        "[1..100]",
        "$sum([1..100])",
        "$map([1..100],function($n){$n+1})",
        "($f:=function($n){$f($n+1)};$f(0))",
    ] {
        let expr = jx::compile(source).unwrap();
        let options = EvaluationOptions {
            limits: Some(Limits {
                max_work: 20,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            items(&expr, Some(b"null"), options).unwrap_err().kind,
            ErrorKind::EvaluationLimit,
            "{source}"
        );
    }
    let expr = jx::compile("rows.a").unwrap();
    let input = br#"{"rows":[{"a":1},{"a":2},{"a":3}]}"#;
    let options = EvaluationOptions {
        limits: Some(Limits {
            max_results: 1,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut seen = 0;
    let error = expr
        .evaluate_with(Some(input), options)
        .unwrap()
        .for_each(|_| seen += 1)
        .unwrap_err();
    assert_eq!(seen, 1);
    assert_eq!(error.kind, ErrorKind::EvaluationLimit);
    let token = jx::Cancellation::default();
    let options = EvaluationOptions {
        cancellation: Some(token.clone()),
        ..Default::default()
    };
    let values = expr.evaluate_with(Some(input), options).unwrap();
    values.for_each(|_| token.cancel()).unwrap_err();
    let cancelled = EvaluationOptions {
        cancellation: Some(token.clone()),
        ..Default::default()
    };
    assert_eq!(
        items(&expr, Some(b"[0,]"), cancelled).unwrap_err().kind,
        ErrorKind::InvalidJson
    );
    let expr = jx::compile("$sum(rows.a)").unwrap();
    let options = EvaluationOptions {
        limits: Some(Limits {
            max_items: 1,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        items(&expr, Some(input), options).unwrap_err().kind,
        ErrorKind::EvaluationLimit
    );
    let expr = jx::compile("[1,2,3]").unwrap();
    let options = EvaluationOptions {
        limits: Some(Limits {
            max_output_bytes: 3,
            ..Default::default()
        }),
        ..Default::default()
    };
    let error = items(&expr, Some(b"null"), options).unwrap_err();
    assert_eq!(error.phase, jx::Phase::Serialization);
    let options = EvaluationOptions {
        deadline: Some(std::time::Instant::now()),
        ..Default::default()
    };
    assert_eq!(
        items(&expr, None, options).unwrap_err().kind,
        ErrorKind::EvaluationLimit
    );
}

#[test]
fn tightening_stack_and_tail_limits_is_explicit_and_native_plans_keep_safe_fallback() {
    let expr = jx::compile("($f:=function($n){$n=0?0:1+$f($n-1)};$f(10))").unwrap();
    let options = EvaluationOptions {
        limits: Some(Limits {
            max_calls: 3,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        items(&expr, None, options).unwrap_err().kind,
        ErrorKind::DepthLimit
    );
    let expr = jx::compile("($f:=function($n){$n=0?0:$f($n-1)};$f(10))").unwrap();
    let options = EvaluationOptions {
        limits: Some(Limits {
            max_tail_calls: 3,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        items(&expr, None, options).unwrap_err().kind,
        ErrorKind::EvaluationLimit
    );
    #[cfg(feature = "jit")]
    {
        let host = jx::HostFunction::new(1, |args, _| Ok(args.first().cloned().flatten()));
        let mut expr = CompileOptions::default()
            .binding("host")
            .compile("[$host(2),a*a+1]")
            .unwrap();
        expr.enable_native();
        let options = EvaluationOptions {
            limits: Some(Limits::default()),
            ..binding("host", host.value())
        };
        assert_eq!(
            items(&expr, Some(br#"{"a":3}"#), options).unwrap(),
            vec![b"[2,10]".to_vec()]
        );
    }
}

#[test]
fn controls_check_retained_builtin_arguments_and_parenthesized_focus_paths() {
    let input = format!("{{\"rows\":[{}]}}", vec!["1"; 100].join(","));
    for source in [
        "$sum(rows)",
        "$sort(rows)",
        "$reverse(rows)",
        "$shuffle(rows)",
    ] {
        let expr = jx::compile(source).unwrap();
        let options = EvaluationOptions {
            limits: Some(Limits {
                max_work: 20,
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            items(&expr, Some(input.as_bytes()), options)
                .unwrap_err()
                .kind,
            ErrorKind::EvaluationLimit,
            "{source}"
        );
    }
    let focus = Value::from_json(br#"{"a":3}"#).unwrap();
    let expr = jx::compile("(a)").unwrap();
    let options = EvaluationOptions {
        focus: Some(focus),
        ..Default::default()
    };
    assert_eq!(items(&expr, None, options).unwrap(), vec![b"3".to_vec()]);
}

#[test]
fn limits_do_not_require_json_encodable_results() {
    for source in ["function(){1}", "[1,function(){2}]"] {
        let expression = jx::compile(source).unwrap();
        let options = EvaluationOptions {
            limits: Some(Limits::default()),
            ..Default::default()
        };
        let value = expression
            .evaluate_with(Some(b"null"), options)
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        assert!(matches!(
            value.value_type(),
            jx::ValueType::Function | jx::ValueType::Array
        ));
    }
}

#[test]
fn controls_and_external_bindings_compose_without_native_support() {
    let expression = CompileOptions::default()
        .binding("n")
        .compile("$n+a")
        .unwrap();
    let options = EvaluationOptions {
        limits: Some(Limits::default()),
        ..binding("n", Value::Number(2.))
    };
    assert_eq!(
        items(&expression, Some(br#"{"a":3}"#), options).unwrap(),
        vec![b"5".to_vec()]
    );
}
