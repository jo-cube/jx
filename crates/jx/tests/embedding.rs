#[path = "support/embedding.rs"]
mod common;
use common::{binding, items};
use jx::{CompileOptions, ErrorKind, EvaluationOptions, Value};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
#[test]
fn external_bindings_shadow_builtins_without_folding_effects() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let function = jx::HostFunction::new(1, move |args, _| {
        counter.fetch_add(1, Ordering::Relaxed);
        Ok(Some(Value::Number(
            args[0].as_ref().unwrap().as_number().unwrap() + 10.,
        )))
    });
    for source in [
        "$sum(2)",
        "2 ~> $sum()",
        "($f:=$sum; $f(2))",
        "$eval('$sum(2)')",
    ] {
        let expr = CompileOptions::default()
            .binding("sum")
            .compile(source)
            .unwrap();
        assert_eq!(
            items(&expr, Some(b"null"), binding("sum", function.value())).unwrap(),
            vec![b"12".to_vec()]
        );
    }
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    let expr = CompileOptions::default()
        .binding("sum")
        .compile("[$sum(1),$sum(1)]")
        .unwrap();
    assert_eq!(
        items(&expr, Some(b"null"), binding("sum", function.value())).unwrap(),
        vec![b"[11,11]".to_vec()]
    );
    assert_eq!(calls.load(Ordering::Relaxed), 6);
    assert_eq!(
        items(&expr, Some(b"null"), EvaluationOptions::default()).unwrap(),
        vec![b"[1,1]".to_vec()]
    );
    let expr = CompileOptions::default()
        .binding("x")
        .compile("($f:=function(){$x};($x:=2;$f()))")
        .unwrap();
    assert_eq!(
        items(&expr, None, binding("x", Value::Number(7.))).unwrap(),
        vec![b"7".to_vec()]
    );
    let error = jx::compile("$sum(1)")
        .unwrap()
        .evaluate_with(Some(b"null"), binding("sum", function.value()))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::BindingError);
}

#[test]
fn focus_missing_borrowing_and_repeated_bindings() {
    let expr = CompileOptions::default()
        .binding("v")
        .compile("[$v,$v,$,$$]")
        .unwrap();
    let options = EvaluationOptions {
        focus: Some(Value::Number(9.)),
        ..binding("v", Value::from_json(b"[1,2]").unwrap())
    };
    assert_eq!(
        items(&expr, Some(b"3"), options).unwrap(),
        vec![b"[1,2,1,2,9,3]".to_vec()]
    );
    let expr = jx::compile("[$exists($),$exists($$)]").unwrap();
    assert_eq!(
        items(&expr, None, EvaluationOptions::default()).unwrap(),
        vec![b"[false,false]".to_vec()]
    );
    let input = b"{\"a\":7}";
    let expr = CompileOptions::default()
        .binding("v")
        .compile("$v.a")
        .unwrap();
    expr.evaluate_with(None, binding("v", Value::from_json(input).unwrap()))
        .unwrap()
        .for_each(|v| {
            let raw = v.as_raw().unwrap();
            assert_eq!(raw.as_bytes().as_ptr(), input[5..].as_ptr());
        })
        .unwrap();
}

#[test]
fn host_calls_compose_with_callbacks_partials_and_dynamic_code() {
    let identity = jx::HostFunction::new(1, |args, _| Ok(args.first().cloned().flatten()));
    for source in [
        "$map([1,2],$host)",
        "[1,2] ~> $host()",
        "($p:=$host(?);$p([1,2]))",
        "$eval('$host([1,2])')",
    ] {
        let expr = CompileOptions::default()
            .binding("host")
            .compile(source)
            .unwrap();
        assert_eq!(
            items(&expr, None, binding("host", identity.value())).unwrap(),
            if source.starts_with("$map") {
                vec![b"1".to_vec(), b"2".to_vec()]
            } else {
                vec![b"[1,2]".to_vec()]
            }
        );
    }
    let failure = jx::HostFunction::new(0, |_, _| Err(jx::Error::user("nope")));
    let expr = CompileOptions::default()
        .binding("host")
        .compile("$host()")
        .unwrap();
    let error = expr
        .evaluate_with(None, binding("host", failure.value()))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::HostError);
    assert_eq!(error.cause().unwrap().message, "nope");
}

#[test]
fn diagnostics_identify_source_and_nested_cause() {
    let error = jx::compile("a[").unwrap_err();
    assert_eq!(error.phase, jx::Phase::Compilation);
    let error = jx::compile("a").unwrap().evaluate(b"[0,]").unwrap_err();
    assert_eq!(error.phase, jx::Phase::Validation);
    assert_eq!(error.source, jx::Source::Input);
    let error = jx::compile("a+null")
        .unwrap()
        .evaluate(br#"{"a":1}"#)
        .unwrap_err();
    assert_eq!(error.phase, jx::Phase::Evaluation);
    let error = jx::compile("$eval('a[')")
        .unwrap()
        .evaluate(b"null")
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::EvalSyntax);
    assert_eq!(error.cause().unwrap().source, jx::Source::DynamicExpression);
    assert_eq!(error.cause().unwrap().phase, jx::Phase::Compilation);
}

#[test]
fn host_signatures_and_reentrant_jsonata_callbacks_share_the_runtime() {
    let apply = jx::HostFunction::new(2, |args, context| {
        context.invoke(args[0].as_ref().unwrap(), &args[1..])
    })
    .with_signature("<fn:n>")
    .unwrap();
    let expr = CompileOptions::default()
        .binding("apply")
        .compile("($x:=7;$apply(function($v){$v+$x},2))")
        .unwrap();
    assert_eq!(
        items(&expr, None, binding("apply", apply.value())).unwrap(),
        vec![b"9".to_vec()]
    );
    let expr = CompileOptions::default()
        .binding("apply")
        .compile("$apply(1,'bad')")
        .unwrap();
    assert_eq!(
        expr.evaluate_with(None, binding("apply", apply.value()))
            .unwrap_err()
            .kind,
        ErrorKind::TypeError
    );
    for signature in ["", "<", "n", "<q:n>"] {
        assert!(
            jx::HostFunction::new(0, |_, _| Ok(None))
                .with_signature(signature)
                .is_err()
        );
    }
    let focused = jx::HostFunction::new(0, |_, context| {
        Ok(Some(Value::from_array(vec![
            context.focus().clone(),
            context.root(),
        ])))
    });
    let expr = CompileOptions::default()
        .binding("focus")
        .compile("$focus()")
        .unwrap();
    let options = EvaluationOptions {
        focus: Some(Value::Number(9.)),
        ..binding("focus", focused.value())
    };
    assert_eq!(
        items(&expr, Some(b"3"), options).unwrap(),
        vec![b"[9,3]".to_vec()]
    );
    let cancellation = jx::Cancellation::default();
    let token = cancellation.clone();
    let cancel = jx::HostFunction::new(0, move |_, context| {
        token.cancel();
        context.checkpoint()?;
        Ok(None)
    });
    let expr = CompileOptions::default()
        .binding("cancel")
        .compile("$cancel()")
        .unwrap();
    let options = EvaluationOptions {
        cancellation: Some(cancellation),
        ..binding("cancel", cancel.value())
    };
    let error = expr.evaluate_with(None, options).unwrap_err();
    assert_eq!(error.cause().unwrap().kind, ErrorKind::Cancelled);
}

#[test]
fn host_callback_results_keep_escaping_lexical_and_dynamic_closures() {
    let apply = jx::HostFunction::new(2, |args, context| {
        context.invoke(args[0].as_ref().unwrap(), &args[1..])
    })
    .with_signature("<fn:f>")
    .unwrap();
    for source in [
        "($x:=7;$f:=$apply(function($v){function($n){$v+$n+$x}},2);$f(3))",
        "($x:=7;$g:=$eval('function($v){function($n){$v+$n+$x}}');$f:=$apply($g,2);$f(3))",
    ] {
        let expression = CompileOptions::default()
            .binding("apply")
            .compile(source)
            .unwrap();
        assert_eq!(
            items(&expression, None, binding("apply", apply.value())).unwrap(),
            vec![b"12".to_vec()],
            "{source}"
        );
    }
}

#[test]
fn pinned_upstream_external_binding_cases() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/semantics/embedding.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let source = case["expr"].as_str().unwrap();
        let mut compile = CompileOptions::default();
        let stored = case["bindings"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| {
                compile = std::mem::take(&mut compile).binding(k.as_str());
                (k, v.to_string())
            })
            .collect::<Vec<_>>();
        let expression = compile.compile(source).unwrap();
        let input = case["data"].to_string();
        let options = EvaluationOptions {
            bindings: stored
                .iter()
                .map(|(k, v)| (k.as_str(), Value::from_json(v.as_bytes()).unwrap()))
                .collect(),
            ..Default::default()
        };
        let result = items(&expression, Some(input.as_bytes()), options);
        if case.get("error").is_some() {
            assert_eq!(result.unwrap_err().kind, ErrorKind::TypeError, "{source}");
        } else {
            let actual = serde_json::Value::Array(
                result
                    .unwrap()
                    .iter()
                    .map(|v| serde_json::from_slice(v).unwrap())
                    .collect(),
            );
            assert_eq!(actual, case["items"], "{source}");
        }
    }
}
