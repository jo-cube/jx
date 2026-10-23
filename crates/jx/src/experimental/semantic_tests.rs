use super::*;

fn compare(source: &str, input: &[u8]) {
    let old = crate::compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    let p = Program::compile(&old).unwrap_or_else(|e| panic!("{source}: {e}"));
    let expected = old.evaluate(input).and_then(|v| v.collect_owned());
    let mut acquisition = Acquisition::new([&p]);
    for actual in [
        p.evaluate_bytes(input).and_then(|v| v.collect_owned()),
        p.evaluate_validated(crate::validate(input).unwrap())
            .and_then(|v| v.collect_owned()),
        p.evaluate(&acquisition.prepare(input).unwrap())
            .and_then(|v| v.collect_owned()),
    ] {
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
    }
}

#[test]
fn functions_capture_live_lexical_frames_and_definition_focus() {
    for source in [
        "($f:=function($x){$x+n};$f(3))",
        "($f:=function($x){$x+$v};$v:=n;$f(3))",
        "($v:=n;$f:=function(){ $v };$v:=n+1;$f())",
        "($make:=function($v){function($x){$v+$x}};$f:=$make(n);$f(3))",
        "($twice:=function($f){function($x){$f($f($x))}};$f:=$twice(function($x){$x+n});$f(3))",
        "($f:=function(){n};items.($f()))",
        "($f:=function($a,$b,$c,$d,$e){[$a,$b,$c,$d,$e]};$f(n,2,3,4,5))",
        "($f:=function($x)<n:n>{$x+1};$f(n))",
        "($f:=function($x)<n:n>{$x+1};$f(s))",
        "($f:=function($x)<n-:n>{$x+1};n.$f())",
        "($f:=function($x){$x};[$f(),$f(n,3)])",
        "($f:=function($x){$x};($x:=1);$x)",
        "($f:=function($x){$x};($f:=2);$f(n))",
        "($f:=function($x){$x};[missing,$f(n),null])",
        "($f:=function($x){$x};$f(a) in [a])",
        "($f:=function($x){$x};$f({'x':n}) in [{'x':n}])",
        "($f:=flag?function($x){$x+n}:function($x){$x-n};$f(3))",
        "($f:=function($x){$x+1};flag?($g:=$f;$g(n)):$f(n))",
    ] {
        compare(
            source,
            br#"{"n":4,"s":"x","flag":true,"items":[{"n":8},{"n":9}],"a":{"x":1}}"#,
        );
    }
}

#[test]
fn general_callbacks_and_partial_calls_share_function_values() {
    for source in [
        "($f:=function($x){$x*n};$map(items,$f))",
        "$map(items,$string)",
        "$filter(items,$boolean)",
        "$reduce(items,function($a,$v,$i,$all){$a+$v+$i+$count($all)},n)",
        "$each(a,function($v,$k){$k & $string($v+n)})",
        "$sift(a,function($v){$v>n})",
        "$single(items,function($v){$v=n})",
        "($f:=$substring(?,0,?);$g:=$f(?,n);$g(s))",
        "($f:=function($a,$b){$a+$b};$g:=$f(?,n);$g(3))",
        "($f:=$uppercase~>$trim;$f(s))",
        "s~>$uppercase~>$trim",
        "$zip(items,items,items,items,items)",
    ] {
        compare(
            source,
            br#"{"n":2,"s":"  hello  ","items":[1,2,3],"a":{"x":1,"y":3}}"#,
        );
    }
}

#[test]
fn recursive_functions_use_bounded_tail_execution() {
    compare(
        "($f:=function($x){$x<=1?1:$x*$f($x-1)};$f(n))",
        br#"{"n":6}"#,
    );
    compare(
        "($even:=function($x){$x=0?true:$odd($x-1)};$odd:=function($x){$x=0?false:$even($x-1)};$odd(n))",
        br#"{"n":6555}"#,
    );
    compare(
        "($make:=function($base){($f:=function($x){$x=0?$base:$f($x-1)};$f)};$make(n)(5000))",
        br#"{"n":6}"#,
    );
}

#[test]
fn evaluations_defer_paths_own_spans_and_honor_consumer_stop() {
    let p = crate::CompileOptions::default()
        .compile_program("items.(v+1)")
        .unwrap();
    let input = br#"{"items":[{"v":1},{"v":null}]}"#;
    let first = p.evaluate_bytes(input).unwrap();
    assert_eq!(
        first.try_for_each(|_| Err("stop")),
        Err(crate::ConsumeError::Consumer("stop"))
    );
    let evaluation = {
        let mut acquisition = Acquisition::new([&p]);
        let evaluation = p.evaluate(&acquisition.prepare(input).unwrap()).unwrap();
        // Replacement of spans and the acquisition's destruction cannot alter it.
        acquisition.prepare(br#"{"items":[]}"#).unwrap();
        evaluation
    };
    assert_eq!(
        evaluation.try_for_each(|_| Err("stop")),
        Err(crate::ConsumeError::Consumer("stop"))
    );
    assert_eq!(
        p.evaluate_bytes(input)
            .unwrap()
            .collect_owned()
            .unwrap_err()
            .kind,
        crate::ErrorKind::TypeError
    );
    assert!(p.evaluate_bytes(br#"{"items":[]} garbage"#).is_err());
    for source in ["items.(v+1)", "(items.(v+1))", "items[v>0].(v+1)"] {
        let old = crate::compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        let p = Program::compile(&old).unwrap();
        let expected = old
            .evaluate(input)
            .map_err(crate::ConsumeError::Evaluation)
            .and_then(|v| v.try_for_each(|_| Err("stop")));
        let actual = p
            .evaluate_bytes(input)
            .map_err(crate::ConsumeError::Evaluation)
            .and_then(|v| v.try_for_each(|_| Err("stop")));
        assert_eq!(actual, expected, "{source}");
    }
}

#[test]
fn embedding_bindings_focus_constants_and_validation_share_contracts() {
    let data =
        std::sync::Arc::new(crate::OwnedValue::from_json(br#"{"scale":3,"a":{"x":1}}"#).unwrap());
    let options = crate::CompileOptions::default()
        .binding("f")
        .binding("factor")
        .constant_binding("table", data);
    for source in [
        "n*$factor",
        "$f(n)",
        "($g:=function($x){$f($x)+$factor};$g(n))",
        "[n,$$.n,$table.scale]",
        "$table.a in [$table.a]",
        "($f:=function(){$$};$f()).n",
    ] {
        let old = options.compile(source).unwrap();
        // Definitions, constants and declared names survive the original program.
        let p = options.compile_program(source).unwrap().clone();
        let host = crate::HostFunction::new(1, |args, _| Ok(args.first().cloned().flatten()));
        let settings = || crate::EvaluationOptions {
            focus: Some(Value::Raw(crate::validate(br#"{"n":9}"#).unwrap())),
            bindings: vec![("f", host.value()), ("factor", Value::Number(2.0))],
            ..Default::default()
        };
        let input = br#"{"n":4}"#;
        let expected = old
            .evaluate_with(Some(input), settings())
            .and_then(|v| v.collect_owned());
        let actual = p
            .evaluate_with(Some(input), settings())
            .and_then(|v| v.collect_owned());
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
        let actual = p
            .evaluate_validated_with(Some(crate::validate(input).unwrap()), settings())
            .and_then(|v| v.collect_owned());
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
    }
    let p = options.compile_program("$f(n)").unwrap();
    let bad = || crate::EvaluationOptions {
        bindings: vec![("unknown", Value::Number(1.0))],
        ..Default::default()
    };
    assert_eq!(
        p.evaluate_with(Some(b"bad"), bad()).unwrap_err().kind,
        crate::ErrorKind::InvalidJson
    );
    assert_eq!(
        p.evaluate_with(Some(b"{}"), bad()).unwrap_err().kind,
        crate::ErrorKind::BindingError
    );
}

#[test]
fn controlled_calls_and_deferred_results_stop_and_release() {
    let p = crate::CompileOptions::default()
        .compile_program("($f:=function($n){$n=0?0:$f($n-1)};$f(n))")
        .unwrap();
    let token = crate::Cancellation::default();
    token.cancel();
    let settings = crate::EvaluationOptions {
        cancellation: Some(token),
        ..Default::default()
    };
    assert_eq!(
        p.evaluate_with(Some(br#"{"n":1000}"#), settings)
            .unwrap_err()
            .kind,
        crate::ErrorKind::Cancelled
    );
    let settings = crate::EvaluationOptions {
        limits: Some(crate::Limits {
            max_tail_calls: 10,
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(
        p.evaluate_with(Some(br#"{"n":1000}"#), settings)
            .unwrap_err()
            .kind,
        crate::ErrorKind::EvaluationLimit
    );
    let p = crate::CompileOptions::default()
        .compile_program("items.v")
        .unwrap();
    let settings = crate::EvaluationOptions {
        limits: Some(crate::Limits {
            max_results: 1,
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut count = 0;
    let result = p
        .evaluate_with(Some(br#"{"items":[{"v":1},{"v":2},{"v":3}]}"#), settings)
        .unwrap()
        .try_for_each(|_| {
            count += 1;
            Ok::<_, ()>(())
        });
    assert_eq!(count, 1);
    assert!(
        matches!(result,Err(crate::ConsumeError::Evaluation(ref e)) if e.kind==crate::ErrorKind::EvaluationLimit)
    );
}

#[test]
fn closures_keep_missing_focus_and_lexical_root_rebinding() {
    for source in [
        "(function(){$})()",
        "(function(){$$})()",
        "($f:=function(){$};[$f(),1])",
        "($$:=missing;$f:=function(){$$};[$f(),1])",
        "($$:={'n':n+1};$f:=function(){$$.n};$f())",
    ] {
        let e = crate::compile(source).unwrap();
        let p = Program::compile(&e).unwrap();
        let expected = e
            .evaluate_with(None, Default::default())
            .and_then(|v| v.collect_owned());
        let actual = p.evaluate_without_input().and_then(|v| v.collect_owned());
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
        compare(source, br#"{"n":4}"#);
    }
}

#[test]
fn function_results_normalize_at_the_consumers_boundary() {
    for source in [
        "($f:=function($a){$map($a,function($v){$v})};$f(items)[])",
        "($f:=function($a){$map($a,function($v){$v})};($f(items))[])",
        "($f:=$string;$map(items,$f)[])",
        "($f:=function($v){$v};items.($f($))[])",
        "items[]",
        "items.v[]",
        "(items.v)[]",
        "items.(v)[]",
        "$filter(items,function($v){true})[]",
        "($m:=$map(items,function($v){$v});$m[])",
        "($f:=function($a){$map($a,function($v){$v})};$m:=$f(items);$m[])",
    ] {
        for input in [
            b"{}".as_slice(),
            br#"{"items":[]}"#,
            br#"{"items":[1]}"#,
            br#"{"items":[1,2]}"#,
            br#"{"items":[{"v":[[1,2]]}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn enclosing_writes_do_not_require_an_unrelated_function() {
    for source in [
        "($x:=false;items[$x:=v>0];$x)",
        "($x:=0;flag?$x:=n:$x:=m;$x)",
        "($x:=false;flag and ($x:=true);$x)",
    ] {
        for input in [
            br#"{"n":1,"m":2,"flag":true,"items":[{"v":1},{"v":-1}]}"#.as_slice(),
            br#"{"n":1,"m":2,"flag":false,"items":[]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn closure_values_survive_callbacks_and_host_reentry() {
    for source in [
        "($fs:=$map(items,function($v){function(){$v}});$map($fs,function($f){$f()}))",
        "($fs:=$map(items,function($v){function(){{'v':$v}}});$map($fs,function($f){($a:=$f();$a in [$a])}))",
    ] {
        compare(source, br#"{"items":[{"v":1},{"v":2}]}"#);
    }
    let options = crate::CompileOptions::default().binding("apply");
    let old = options.compile("$apply(function($x){$x+n},3)").unwrap();
    let p = options
        .compile_program("$apply(function($x){$x+n},3)")
        .unwrap();
    let host = crate::HostFunction::new(2, |args, context| {
        context.invoke(args[0].as_ref().unwrap(), &args[1..])
    });
    let settings = || crate::EvaluationOptions {
        bindings: vec![("apply", host.value())],
        ..Default::default()
    };
    let input = br#"{"n":4}"#;
    let expected = old
        .evaluate_with(Some(input), settings())
        .unwrap()
        .collect_owned();
    let actual = p
        .evaluate_with(Some(input), settings())
        .unwrap()
        .collect_owned();
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    let mut acquisition = Acquisition::new([&p]);
    let actual = p
        .evaluate_prepared_with(&acquisition.prepare(input).unwrap(), settings())
        .unwrap()
        .collect_owned();
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
}

#[test]
fn retained_arenas_release_on_error_and_external_closures_are_rejected() {
    let options = crate::CompileOptions::default().binding("host");
    let p = options
        .compile_program("($f:=function(){$host()};$f()+null)")
        .unwrap();
    let token = std::sync::Arc::new(());
    let held = token.clone();
    let host = crate::HostFunction::new(0, move |_, _| {
        let _ = &held;
        Ok(Some(Value::Number(1.0)))
    });
    let settings = crate::EvaluationOptions {
        bindings: vec![("host", host.value())],
        ..Default::default()
    };
    assert_eq!(
        p.evaluate_with(Some(b"{}"), settings).unwrap_err().kind,
        crate::ErrorKind::TypeError
    );
    drop(host);
    assert_eq!(std::sync::Arc::strong_count(&token), 1);
    let source = crate::CompileOptions::default()
        .compile_program("function(){1}")
        .unwrap();
    let closure = source
        .evaluate_bytes(b"{}")
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    let p = options.compile_program("$host()").unwrap();
    let settings = crate::EvaluationOptions {
        bindings: vec![("host", closure)],
        ..Default::default()
    };
    assert_eq!(
        p.evaluate_with(Some(b"{}"), settings).unwrap_err().kind,
        crate::ErrorKind::BindingError
    );
}

#[test]
fn ordinary_calls_do_not_spend_the_tail_call_budget() {
    for source in [
        "(function(){1})()",
        "($f:=function($x){$x+1};$f(n))",
        "($f:=function($x){$x??$f(1)};$f(n))",
    ] {
        let old = crate::compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        let p = Program::compile(&old).unwrap();
        let settings = || crate::EvaluationOptions {
            limits: Some(crate::Limits {
                max_tail_calls: 0,
                ..Default::default()
            }),
            ..Default::default()
        };
        let input = br#"{"n":4}"#;
        let expected = old
            .evaluate_with(Some(input), settings())
            .and_then(|v| v.collect_owned());
        let actual = p
            .evaluate_with(Some(input), settings())
            .and_then(|v| v.collect_owned());
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
    }
}

#[test]
fn retained_lookup_keeps_matches_before_path_flattening() {
    for source in [
        "a[]",
        "a.x[]",
        "(a)[]",
        "(a.x)[]",
        "$count(a[])",
        "a[] in [a[]]",
    ] {
        for input in [
            br#"[{"a":[1]}]"#.as_slice(),
            br#"[{"a":[[1,2]]}]"#,
            br#"[{"a":1},{"a":2}]"#,
            br#"{"a":1}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn dynamic_call_abi_uses_programs_and_preserves_host_shadowing() {
    let options = crate::CompileOptions::default().binding("eval");
    let input = br#"{"source":"n","n":4}"#;
    for source in [
        "($g:=$eval;$g(source))",
        "($g:=$eval;$f:=function($s){$g($s)};$f(source))",
        "($g:=$eval;$f:=$g(?);$f(source))",
        "$map([source],$eval)",
    ] {
        let p = options.compile_program(source).unwrap();
        let expected = options
            .compile(source)
            .unwrap()
            .evaluate(input)
            .unwrap()
            .collect_owned()
            .unwrap();
        let result = p.evaluate_bytes(input).unwrap().collect_owned().unwrap();
        assert_eq!(format!("{result:?}"), format!("{expected:?}"), "{source}");
        let mut acquisition = Acquisition::new([&p]);
        let mut output = Vec::new();
        p.write_json(&acquisition.prepare(input).unwrap(), &mut output)
            .unwrap();
        assert_eq!(
            p.evaluate(&acquisition.prepare(input).unwrap())
                .unwrap()
                .collect_owned()
                .map(|v| format!("{v:?}")),
            Ok(format!("{expected:?}"))
        );
    }
    let p = options.compile_program("($g:=$eval;$g(source))").unwrap();
    let host = crate::HostFunction::new(1, |_, _| Ok(Some(Value::Number(7.0))));
    let settings = crate::EvaluationOptions {
        bindings: vec![("eval", host.value())],
        ..Default::default()
    };
    assert_eq!(
        p.evaluate_with(Some(input), settings)
            .unwrap()
            .single()
            .unwrap()
            .unwrap()
            .as_number(),
        Some(7.0)
    );
}

#[test]
fn host_functions_in_external_focus_get_the_shared_call_context() {
    let source = "$each({'a':1,'b':2},$)";
    let old = crate::compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    let p = Program::compile(&old).unwrap();
    let host = crate::HostFunction::new(1, |args, _| Ok(args[0].clone()));
    let settings = || crate::EvaluationOptions {
        focus: Some(host.value()),
        ..Default::default()
    };
    let expected = old
        .evaluate_with(Some(b"{}"), settings())
        .and_then(|v| v.collect_owned());
    let actual = p
        .evaluate_with(Some(b"{}"), settings())
        .and_then(|v| v.collect_owned());
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
}

#[test]
fn call_limits_preserve_signature_and_tail_error_precedence() {
    for source in [
        "($f:=function($x)<n:n>{$x+1};$f(s))",
        "($f:=function($n)<n:n>{$n=0?0:$f($n-1)};$f(s))",
        "($f:=function($n){$n=0?0:$f($n-1)};$f(n))",
        "($f:=function($n){$n=0?0:$f($n-1)+1};$f(n))",
        "($f:=function($x){$x};$map(items,$f))",
        "$map(items,function($x)<n:n>{$x+1})",
        "($f:=function($x)<n:n>{$x+1};$g:=$f(?);$g(s))",
        "($f:=function($x){$x};n~>$f)",
    ] {
        let old = crate::compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        let p = Program::compile(&old).unwrap();
        let mut acquisition = Acquisition::new([&p]);
        for input in [
            br#"{"n":3,"s":"bad","items":[1,2]}"#.as_slice(),
            br#"{"n":0,"s":2,"items":["bad",2]}"#,
        ] {
            for max_calls in [0, 1, 64] {
                for max_tail_calls in [0, 1, 10] {
                    let settings = || crate::EvaluationOptions {
                        limits: Some(crate::Limits {
                            max_calls,
                            max_tail_calls,
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    let expected = old
                        .evaluate_with(Some(input), settings())
                        .and_then(|v| v.collect_owned());
                    let actual = p
                        .evaluate_with(Some(input), settings())
                        .and_then(|v| v.collect_owned());
                    assert_eq!(
                        format!("{actual:?}"),
                        format!("{expected:?}"),
                        "{source}: {max_calls}/{max_tail_calls}"
                    );
                    let actual = p
                        .evaluate_prepared_with(&acquisition.prepare(input).unwrap(), settings())
                        .and_then(|v| v.collect_owned());
                    assert_eq!(
                        format!("{actual:?}"),
                        format!("{expected:?}"),
                        "{source}: {max_calls}/{max_tail_calls}"
                    );
                }
            }
        }
    }
}

#[test]
fn scalar_cse_does_not_elide_function_valued_pattern_effects() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    for source in [
        "($f:=p;{'x':$contains(s,$f),'y':$contains(s,$f)})",
        "($f:=p;{'x':$replace(s,$f,'a'),'y':$replace(s,$f,'a')})",
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let pattern = crate::HostFunction::new(1, move |_, _| {
            count.fetch_add(1, Ordering::Relaxed);
            Ok(None)
        });
        let settings = || crate::EvaluationOptions {
            focus: Some(Value::object(vec![
                (
                    Value::StringLiteral(crate::RawJson("\"s\"")),
                    Value::StringLiteral(crate::RawJson("\"abc\"")),
                ),
                (
                    Value::StringLiteral(crate::RawJson("\"p\"")),
                    pattern.value(),
                ),
            ])),
            ..Default::default()
        };
        let old = crate::compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        let p = Program::compile(&old).unwrap();
        let expected = old
            .evaluate_with(Some(b"{}"), settings())
            .unwrap()
            .collect_owned();
        assert_eq!(calls.swap(0, Ordering::Relaxed), 2, "reference: {source}");
        let actual = p
            .evaluate_with(Some(b"{}"), settings())
            .unwrap()
            .collect_owned();
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        assert_eq!(calls.load(Ordering::Relaxed), 2, "{source}");
    }
}

#[test]
fn fold_fusion_preserves_predicate_writes_and_callback_order() {
    for source in [
        "($x:=false;$sum((items[$x:=v>0]).($number($x))))",
        "($x:=false;$sum(items[$x:=v>0].($number($x))))",
    ] {
        compare(source, br#"{"items":[{"v":1},{"v":-1}]}"#);
    }
    use std::sync::{Arc, Mutex};
    for source in [
        "$sum((items[$tap('p',v)>0]).($number($tap('m',v))))",
        "$sum(items[$tap('p',v)>0].($number($tap('m',v))))",
    ] {
        let options = crate::CompileOptions::default().binding("tap");
        let old = options.compile(source).unwrap();
        let p = options.compile_program(source).unwrap();
        let trace = Arc::new(Mutex::new(Vec::new()));
        let seen = trace.clone();
        let tap = crate::HostFunction::new(2, move |args, _| {
            seen.lock().unwrap().push(format!("{:?}", args[0]));
            Ok(args[1].clone())
        });
        let settings = || crate::EvaluationOptions {
            bindings: vec![("tap", tap.value())],
            ..Default::default()
        };
        let input = br#"{"items":[{"v":1},{"v":2}]}"#;
        let expected = old
            .evaluate_with(Some(input), settings())
            .unwrap()
            .collect_owned();
        let expected_trace = std::mem::take(&mut *trace.lock().unwrap());
        let mut acquisition = Acquisition::new([&p]);
        let actual = p
            .evaluate_prepared_with(&acquisition.prepare(input).unwrap(), settings())
            .unwrap()
            .collect_owned();
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        assert_eq!(*trace.lock().unwrap(), expected_trace, "{source}");
    }
}

#[test]
fn scoped_deferred_delivery_preserves_settings_cancellation() {
    let options = crate::CompileOptions::default().binding("unused");
    let source = "(items.(v+1))";
    let old = options.compile(source).unwrap();
    let p = options.compile_program(source).unwrap();
    let settings = || crate::EvaluationOptions {
        bindings: vec![("unused", Value::Number(1.0))],
        ..Default::default()
    };
    let input = br#"{"items":[{"v":1},{"v":null}]}"#;
    let expected = old
        .evaluate_with(Some(input), settings())
        .map_err(crate::ConsumeError::Evaluation)
        .and_then(|v| v.try_for_each(|_| Err("stop")));
    let actual = p
        .evaluate_with(Some(input), settings())
        .map_err(crate::ConsumeError::Evaluation)
        .and_then(|v| v.try_for_each(|_| Err("stop")));
    assert_eq!(actual, expected);

    let old = crate::compile("($x:=2;items.(v+$x))").unwrap();
    let p = Program::compile(&old).unwrap();
    let mut acquisition = Acquisition::new([&p]);
    let mut output = Vec::new();
    p.write_json(
        &acquisition
            .prepare(br#"{"items":[{"v":1},{"v":2}]}"#)
            .unwrap(),
        &mut output,
    )
    .unwrap();
    assert_eq!(output, b"[3,4]");
}

#[test]
fn complete_delivery_preserves_evaluation_error_precedence() {
    // A complete buffer consumer must finish evaluation before reporting a
    // serialization failure from a function emitted by an earlier row.
    let source = "items.(v=1?function(){$}:v+1)";
    let old = crate::compile(source).unwrap_or_else(|e| panic!("{source}: {e}"));
    let p = Program::compile(&old).unwrap();
    let input = br#"{"items":[{"v":1},{"v":2},{"v":null}]}"#;
    assert_eq!(
        old.evaluate(input).unwrap_err().kind,
        crate::ErrorKind::TypeError
    );
    let mut acquisition = Acquisition::new([&p]);
    let mut output = b"prefix".to_vec();
    let actual = p.write_json(&acquisition.prepare(input).unwrap(), &mut output);
    assert!(
        matches!(actual,Err(crate::ConsumeError::Evaluation(e)) if e.kind==crate::ErrorKind::TypeError)
    );
    assert_eq!(output, b"prefix");
}

#[test]
fn call_ports_and_scoped_ssa_preserve_writes_focus_and_identity() {
    for source in [
        "($f:=function($d){{'x':$d.a,'y':$d.b}};$f($))",
        "($f:=function($a,$b){{'x':$a.x,'y':$b.y}};$f(a,b))",
        "($x:=1;$f:=function(){$x:=3};($f();$z:=5);$z)",
        "($f:=function(){$x};$x:=1;flag?($x:=2):($x:=3);$f())",
        "($f:=function($x){flag?($x:=2):$x};$f(1))",
        "($f:=function($x){(($x:=2);$x)};$f(1))",
        "($f:=function($x){($g:=function(){$x:=2};$g();$x)};$f(1))",
        "($f:=function($x){$x};$m:=a;$f($m) in [$m])",
        "($f:=function($x){$x};$f(a) in [b])",
        "($f:=function($a,$b,$c,$d,$e,$f,$g,$h,$i){$a+$i};$f(1,2,3,4,5,6,7,8,9))",
        "($f:=function($x,$x){$x};$f(1,2))",
    ] {
        for input in [
            br#"{"flag":true,"a":{"x":1},"b":{"y":2}}"#.as_slice(),
            br#"{"flag":false,"a":[{"x":1},{"x":2}],"b":{"y":2}}"#.as_slice(),
            br#"{"flag":true,"a":{"x":0},"a":{"x":1},"b":{"y":2}}"#.as_slice(),
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn resolved_calls_contribute_parameter_and_definition_focus_demands() {
    for source in [
        "($f:=function($d){{'x':$d.a,'y':$d.b}};$f($))",
        "($f:=function(){{'x':a,'y':b}};$f())",
        "($f:=function($d){{'x':$d.a,'y':$d.b}};$g:=$f;$g($))",
    ] {
        let p = crate::CompileOptions::default()
            .compile_program(source)
            .unwrap();
        let mut acquisition = Acquisition::new([&p]);
        assert!(
            acquisition.retained_bytes() >= 3 * std::mem::size_of::<acquisition::Span>(),
            "{source}"
        );
        let d = acquisition.prepare(br#"{"a":1,"b":2}"#).unwrap();
        assert_eq!(p.evaluate(&d).unwrap().collect_owned().unwrap().len(), 1);
        compare(source, br#"{"a":1,"b":2}"#);
    }
}

#[test]
fn candidate_predicates_preserve_positions_duplicates_mixed_values_and_focus() {
    for input in [
        br#"{"items":[{"v":0},{"v":1},{"v":2}]}"#.as_slice(),
        br#"{"items":[{"v":-1},{"v":-2},{"v":-3}]}"#,
        br#"{"items":[[1,2],[3,4]]}"#,
        br#"{"items":[[],[1],null,0,true]}"#,
        br#"{"items":[]}"#,
        br#"{"items":[{"v":1e999},{"v":0}]}"#,
        br#"{}"#,
    ] {
        for source in [
            "items[0]",
            "items[(0)]",
            "items[(-1)]",
            "items[1.8]",
            "items[-1]",
            "items[-2.2]",
            "items[99]",
            "items[v]",
            "items[v>0?true:1]",
            "items[[0,0,-1]]",
            "items[[0,'x']]",
            "items[[]]",
            "items[[true,false]]",
            "items[$number(v)]",
            "items[($index:=v;$index)]",
            "items[0].($)",
            "items[-1].($)",
            "items[[0,-1]].($)",
            "(items[[0]]).($)",
            "(items[[0,-1]]).($)",
            "items[v][true]",
            "items[[0,0,-1]][true]",
            "($f:=function($x){$x};items[$f(v)])",
            "($x:=0;items[$x:=$x+1];$x)",
            "($x:=0;items[$x:=$x+1][$x:=$x+10>0];$x)",
            "($f:=function($a){$a[-1]};$f(items))",
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn candidate_delivery_preserves_prefixes_error_order_and_cancellation() {
    fn prefix(
        e: Result<crate::Evaluation<'_, '_>, crate::Error>,
        limit: usize,
    ) -> (Vec<String>, String) {
        let mut items = Vec::new();
        let result = e.map_err(crate::ConsumeError::Evaluation).and_then(|e| {
            e.try_for_each(|v| {
                items.push(format!("{:?}", v.to_owned().unwrap()));
                if items.len() == limit {
                    Err(())
                } else {
                    Ok(())
                }
            })
        });
        (items, format!("{result:?}"))
    }
    for source in [
        "items[v]",
        "items[[0,0,-1]]",
        "items[-1]",
        "items[0].(v+1)",
        "items[v].(v+1)",
        "items[$number(v)].(v+1)",
        "items[[0,0,-1]][v>0].(v+1)",
        "items[[0,0,-1]].($number(v))",
    ] {
        for input in [
            br#"{"items":[{"v":0},{"v":1},{"v":null}]}"#.as_slice(),
            br#"{"items":[{"v":0},{"v":1},{"v":"bad"}]}"#,
            br#"{"items":[[1],[2]]}"#,
        ] {
            let e = crate::compile(source).unwrap();
            let p = Program::compile(&e).unwrap();
            let mut a = Acquisition::new([&p]);
            for limit in [1, 2, 99] {
                let expected = prefix(e.evaluate(input), limit);
                assert_eq!(
                    prefix(p.evaluate_bytes(input), limit),
                    expected,
                    "raw {source} {limit}"
                );
                assert_eq!(
                    prefix(p.evaluate(&a.prepare(input).unwrap()), limit),
                    expected,
                    "prepared {source} {limit}"
                );
            }
        }
    }
}

#[test]
fn unused_definitions_do_not_acquire_their_body_fields() {
    let p = crate::CompileOptions::default()
        .compile_program("($f:=function(){{'a':a.x,'b':b.y}};1)")
        .unwrap();
    let a = Acquisition::new([&p]);
    assert_eq!(a.retained_bytes(), std::mem::size_of::<acquisition::Span>());
}

#[test]
fn returned_ports_compose_with_navigation_without_acquiring_unused_definitions() {
    for source in [
        "($f:=function($d){$d};$m:=$f($);{'a':$m.a.x,'b':$m.b.y})",
        "($f:=function($d){$d.a};$m:=$f($);{'x':$m.x,'y':$m.y})",
        "($f:=function(){a};$m:=$f();{'x':$m.x,'y':$m.y})",
        "($f:=function($a,$b){$b};$m:=$f(a,b);{'x':$m.x,'y':$m.y})",
        "($f:=function($d){($g:=function($v){$v};$g($d))};$m:=$f($);{'a':$m.a.x,'b':$m.b.y})",
    ] {
        let p = crate::CompileOptions::default()
            .compile_program(source)
            .unwrap();
        let a = Acquisition::new([&p]);
        assert!(
            a.retained_bytes() >= 3 * std::mem::size_of::<acquisition::Span>(),
            "{source}"
        );
        for input in [
            br#"{"a":{"x":1,"y":2},"b":{"x":3,"y":4}}"#.as_slice(),
            br#"{"a":[{"x":1},{"x":2}],"b":{}}"#,
            br#"{}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn read_only_conditionals_keep_parameter_and_return_dependencies() {
    for source in [
        "($f:=function($d){$d.flag?{'a':$d.a.x,'b':$d.b.y}:{'a':$d.b.x,'b':$d.a.y}};$f($))",
        "($f:=function($d){$d.flag?$d:$d};$m:=$f($);{'a':$m.a.x,'b':$m.b.y})",
        "($f:=function($d){($d.flag and $d.a.x>0)?$d:$d};$m:=$f($);{'a':$m.a.x,'b':$m.b.y})",
    ] {
        let p = crate::CompileOptions::default()
            .compile_program(source)
            .unwrap();
        let a = Acquisition::new([&p]);
        assert!(
            a.retained_bytes() >= 6 * std::mem::size_of::<acquisition::Span>(),
            "{source}: {}",
            a.retained_bytes()
        );
        for input in [
            br#"{"flag":true,"a":{"x":1,"y":2},"b":{"x":3,"y":4}}"#.as_slice(),
            br#"{"flag":false,"a":{"x":1,"y":2},"b":{"x":3,"y":4}}"#,
            br#"{}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn repeated_calls_keep_bindings_and_return_ports_in_the_caller() {
    let source = "($f:=function($d){$d};$f(a);$m:=$f(b);{'x':$m.x,'y':$m.y})";
    let p = crate::CompileOptions::default()
        .compile_program(source)
        .unwrap();
    assert!(
        Acquisition::new([&p]).retained_bytes() >= 5 * std::mem::size_of::<acquisition::Span>()
    );
    compare(source, br#"{"a":{},"b":{"x":1,"y":2}}"#);
}

#[test]
fn calls_isolate_caller_bindings_and_late_captures_stay_live() {
    for source in [
        "($x:=n;$f:=function(){$x:=n+1};[$f(),$x])",
        "($x:=n;$f:=function(){(items[$x:=v];$x)};[$f(),$x])",
        "($f:=function(){$x};$f();$x:=n;$f())",
        "($f:=function($d){$d};$f($);$doc:=$f($);{'n':$doc.n,'items':$doc.items})",
        "($f:=function(){$$.n+$$.n};$f())",
    ] {
        compare(source, br#"{"n":7,"items":[{"v":0},{"v":1}]}"#);
    }
}

#[test]
fn opaque_results_and_reads_use_the_common_local_acquisition() {
    for source in [
        "($f:=flag?function($d){$d}:function($d){$d};$m:=$f($);{'a':$m.a,'b':$m.b})",
        "($m:=$;$f:=function(){{'a':$m.a,'b':$m.b}};$f())",
        "($m:=$;$f:=function(){$m.a+$m.b};$f())",
        "($f:=function($d){flag?$d.a:$d.b};$m:=$f($);{'x':$m.x,'y':$m.y})",
    ] {
        for input in [
            br#"{"flag":true,"a":{"x":1,"y":2},"b":{"x":3,"y":4}}"#.as_slice(),
            br#"{"flag":false,"a":2,"b":3}"#,
            br#"{}"#,
        ] {
            compare(source, input);
        }
    }
    let opaque = crate::CompileOptions::default()
        .compile_program(
            "($f:=flag?function($d){$d}:function($d){$d};$m:=$f($);{'a':$m.a,'b':$m.b})",
        )
        .unwrap();
    let direct = crate::CompileOptions::default()
        .compile_program("{'a':a,'b':b}")
        .unwrap();
    let mut a = Acquisition::new([&opaque, &direct]);
    let d = a.prepare(br#"{"a":1,"b":2}"#).unwrap();
    let input = run::Input::Captured(&d);
    // The opaque value is the same prepared object. Reuse spans rather than
    // creating a second index owner when another transform acquired its fields.
    let mut direct_acquisition = Acquisition::new([&direct]);
    let direct_document = direct_acquisition.prepare(br#"{"a":1,"b":2}"#).unwrap();
    assert!(matches!(
        input
            .root()
            .acquire(&input, direct_document.owner.unwrap().0),
        run::Cell::Capture(0)
    ));
    assert_eq!(
        format!("{:?}", opaque.evaluate(&d).unwrap().collect_owned()),
        format!("{:?}", direct.evaluate(&d).unwrap().collect_owned())
    );
    assert!(matches!(
        input.restore(Some(Value::Raw(d.raw()))),
        run::Cell::Capture(0)
    ));
}

#[test]
fn optional_builtin_fallback_keeps_fresh_host_observable_function_values() {
    let options = crate::CompileOptions::default()
        .binding("inspect")
        .binding("uppercase");
    let source = "$inspect($uppercase,$uppercase)";
    let old = options.compile(source).unwrap();
    let p = options.compile_program(source).unwrap();
    let inspect = crate::HostFunction::new(2, |args, _| {
        let same = matches!((&args[0],&args[1]),(Some(Value::Function(a)),Some(Value::Function(b))) if std::rc::Rc::ptr_eq(a,b));
        Ok(Some(Value::Boolean(same)))
    });
    for bound in [false, true] {
        let uppercase = crate::HostFunction::new(1, |args, _| Ok(args[0].clone()));
        let settings = || {
            let mut bindings = vec![("inspect", inspect.value())];
            if bound {
                bindings.push(("uppercase", uppercase.value()));
            }
            crate::EvaluationOptions {
                bindings,
                ..Default::default()
            }
        };
        let actual = p
            .evaluate_with(Some(b"{}"), settings())
            .unwrap()
            .collect_owned();
        let expected = old
            .evaluate_with(Some(b"{}"), settings())
            .unwrap()
            .collect_owned();
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        assert_eq!(
            format!("{actual:?}"),
            format!(
                "{:?}",
                Ok::<_, crate::Error>(vec![
                    crate::OwnedValue::from_json(if bound { b"true" } else { b"false" }).unwrap()
                ])
            )
        );
    }
}

#[test]
fn immediate_callbacks_preserve_call_and_tail_quotas() {
    for source in [
        "$map(items,function($v){$v+1})",
        "$filter(items,function($v){$v>0})",
        "$sum($map(items,function($v){$v*2}))",
        "$map(items,function($v){$string()})",
        "$map(items,function($v){$map([$v],function($v){$v+1})})",
        "$sum($map(items,function($v){$sum($map([$v],function($v){$v+1}))}))",
    ] {
        let old = crate::compile(source).unwrap();
        let p = Program::compile(&old).unwrap();
        for input in [
            br#"{"items":[1,2]}"#.as_slice(),
            br#"{"items":[]}"#,
            br#"{}"#,
        ] {
            for max_calls in [0, 1, 64] {
                for max_tail_calls in [0, 1] {
                    let settings = || crate::EvaluationOptions {
                        limits: Some(crate::Limits {
                            max_calls,
                            max_tail_calls,
                            ..Default::default()
                        }),
                        ..Default::default()
                    };
                    let expected = old
                        .evaluate_with(Some(input), settings())
                        .and_then(|v| v.collect_owned());
                    let actual = p
                        .evaluate_with(Some(input), settings())
                        .and_then(|v| v.collect_owned());
                    assert_eq!(
                        format!("{actual:?}"),
                        format!("{expected:?}"),
                        "{source} {max_calls}/{max_tail_calls}"
                    );
                }
            }
        }
    }
}

#[test]
fn callback_root_bindings_are_visible_to_host_matchers() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    for source in [
        "$map([$],function($$){$contains('a',$$)})",
        "$map([$],function($v){($$:=$v;$contains('a',$v))})",
    ] {
        let options = crate::CompileOptions::default();
        let old = options.compile(source).unwrap();
        let p = options.compile_program(source).unwrap();
        let seen = Arc::new(AtomicBool::new(false));
        let held = seen.clone();
        let host = crate::HostFunction::new(1, move |_, ctx| {
            held.store(matches!(ctx.root(), Value::Function(_)), Ordering::Relaxed);
            Ok(None)
        });
        let settings = || crate::EvaluationOptions {
            focus: Some(host.value()),
            ..Default::default()
        };
        old.evaluate_with(Some(b"{}"), settings())
            .unwrap()
            .collect_owned()
            .unwrap();
        assert!(seen.swap(false, Ordering::Relaxed));
        p.evaluate_with(Some(b"{}"), settings())
            .unwrap()
            .collect_owned()
            .unwrap();
        assert!(seen.swap(false, Ordering::Relaxed), "{source}");
    }
}

#[test]
fn borrowed_row_ports_cross_calls_without_owning_the_row_index() {
    for source in [
        "items.($f:=function(){metrics.v0};$f())",
        "items.($f:=function(){$};$v:=$f();{'x':$v.metrics.v0,'same':$v in [$]})",
        "$map(items,function($d){function($a){$a.metrics.v0+$a.metrics.v1}($d)})",
        "items.($f:=function(){metrics};$m:=$f();{'x':$m.v0,'y':$m.v1})",
        "($fs:=$map(items,function($d){function(){$d}});$map($fs,function($f){$f()}))",
    ] {
        for input in [
            br#"{"items":[{"metrics":{"v0":1,"v1":2}},{"metrics":{"v0":3,"v1":4}}]}"#.as_slice(),
            br#"{"items":[{"metrics":{}},{"metrics":null},{}]}"#,
            br#"{"items":[]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn nested_rows_with_one_schema_keep_distinct_input_views() {
    use input::{Cell, Input};
    let schema = acquisition::Schema {
        nodes: vec![acquisition::Node {
            children: Box::new([]),
            end: 1,
        }],
    };
    let outer = crate::validate(br#"{"v":1}"#).unwrap();
    let inner = crate::validate(br#"{"v":2}"#).unwrap();
    let mut a = vec![acquisition::Span::default(); schema.nodes.len()];
    let mut b = a.clone();
    crate::json::capture_schema_validated(outer, &schema, &mut a);
    crate::json::capture_schema_validated(inner, &schema, &mut b);
    let parent = Input::Raw(None);
    let first = acquisition::Prepared {
        text: outer.as_str(),
        schema: &schema,
        spans: &a,
        bindings: &[],
        owner: None,
    };
    let input = Input::CapturedRow {
        parent: &parent,
        row: &first,
    };
    let retained = input.restore(Some(Value::Raw(outer)));
    let second = acquisition::Prepared {
        text: inner.as_str(),
        schema: &schema,
        spans: &b,
        bindings: &[],
        owner: None,
    };
    let nested = Input::CapturedRow {
        parent: &input,
        row: &second,
    };
    for cell in [retained, nested.restore(Some(Value::Raw(outer)))] {
        assert!(matches!(cell, Cell::RowCapture(..)));
        assert_eq!(
            cell.value(&nested).unwrap().as_raw().unwrap().as_bytes(),
            outer.as_bytes()
        );
    }
    assert_eq!(
        nested
            .restore(Some(Value::Raw(inner)))
            .value(&nested)
            .unwrap()
            .as_raw()
            .unwrap()
            .as_bytes(),
        inner.as_bytes()
    );
}

#[test]
fn acquired_rows_use_their_programs_physical_read_layout() {
    use input::{Cell, Input};
    let p = crate::CompileOptions::default()
        .compile_program("$map(items,function($v){$v.a+$v.b})")
        .unwrap();
    let body = p
        .code
        .iter()
        .find_map(|i| match &i.kind {
            Operation::Each { body, .. } => Some(&**body),
            _ => None,
        })
        .unwrap();
    assert!(body.row_reads.iter().any(Option::is_some));
    let mut acquisition = Acquisition::new([&p]);
    let document = acquisition
        .prepare(br#"{"items":[{"a":1,"b":2}]}"#)
        .unwrap();
    let input = Input::Captured(&document);
    let source = Cell::Value(Value::Raw(crate::validate(br#"[{"a":1,"b":2}]"#).unwrap()));
    input
        .each(&source, &body.row_schema, |row, _, _| {
            let reads = row.reads(body).expect("compiled row reads");
            assert!(
                reads.row.is_some(),
                "row layout must not fall back to root bindings"
            );
            assert!(reads.fields.iter().any(Option::is_some));
            Ok::<_, ()>(())
        })
        .unwrap();
}

#[test]
fn general_routes_and_positions_use_compiled_candidate_contexts() {
    for source in [
        "items.children[v>0].(v*2)",
        "items.(children[v>0]).(v*2)",
        "items.children[0].v",
        "items.children[-1].v",
        "items[true][0].children[0].v",
        "items.children[true][0].v",
        "items.([children]).v",
        "items.(children).([v])",
        "items.children.(v+1).($string($)).($uppercase($))",
        "($f:=function($d){$d.items.children[0].v};$f($))",
        "items.(($x:=children;$x[0])).v",
        "items.*.v",
        "items.**.v",
        "[a..b]",
    ] {
        compare(
            source,
            br#"{"a":1,"b":4,"items":[{"children":[{"v":1},{"v":2}]},{"children":[{"v":3}]}]}"#,
        );
    }
}

#[test]
fn compiled_grouping_and_ordering_share_semantic_kernels() {
    for source in [
        "items{k:v}",
        "items{k:$sum(v)}",
        "items.{k:v}",
        "{key: value}",
        "{'x':a,'x':b}",
        "items{'n':$count($)}",
        "items{k:{'v':v,'n':$count($)}}",
        "items{k:$ in [$$.items[0]]}",
        "items^(v)",
        "items^(>v,k)",
        "items^($lowercase(k)).v",
        "(items^(v))[0].k",
        "($x:=0;items^(($x:=$x+1;v));$x)",
        "items{k:$error('value')}",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"key":"x","value":2,"a":1,"b":2,"items":[]}"#,
            br#"{"items":[{"k":"b","v":3},{"k":"a","v":2},{"k":"b","v":1}]}"#,
            br#"{"items":[{"k":null,"v":1},{"k":"x","v":null}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn general_sequence_delivery_matches_prefixes_errors_and_cancellation() {
    for source in [
        "items.(v).($number($)).($+1)",
        "items.children[true][0].v",
        "items[true][0]",
        "items.children[true][v>0][-1].v",
        "items.*.($number($)).($+1)",
        "items.**.v",
        "[a..b].($+1)",
        "items.([v]).($)",
        "items.(children[0]).v",
        "items.children[v>0].($number(v)).($+1)",
    ] {
        for input in [br#"{}"#.as_slice(), br#"{"a":1,"b":4,"items":[]}"#,
            br#"{"a":1,"b":5,"items":[{"v":"2","children":[{"v":1}]},{"v":"bad","children":[{"v":null}]}]}"#,
            br#"{"items":[[],[{"v":[1,[2]]}],{"v":[3]}]}"#] {
            let old = crate::compile(source).unwrap();
            let p = Program::compile(&old).unwrap();
            for stop in [1, 2, usize::MAX] {
                let mut expected = Vec::new();
                let reference = old.evaluate(input).map_err(crate::ConsumeError::Evaluation).and_then(|r| r.try_for_each(|v| {
                    expected.push(format!("{:?}", v.to_owned()));
                    if expected.len() == stop { Err("stop") } else { Ok(()) }
                }));
                let mut actual = Vec::new();
                let result = p.evaluate_bytes(input).map_err(crate::ConsumeError::Evaluation).and_then(|r| r.try_for_each(|v| {
                    actual.push(format!("{:?}", v.to_owned()));
                    if actual.len() == stop { Err("stop") } else { Ok(()) }
                }));
                assert_eq!((actual, result), (expected, reference), "{source} {input:?} stop={stop}");
            }
            compare(source, input);
        }
    }
}
#[test]
fn array_focus_constructors_preserve_group_identity_and_error_precedence() {
    for source in [
        "items.(children.{'v':v}).v",
        "items{'x':{'v':v,'n':$count($)}}",
        "items{k:($ in [$$.items[0]])}",
        "items{'x':{'self':$,'same':$ in [$$.items]}}",
        "($x:=0;items{($x:=$x+1;$string(v)):($x:= $x+1;$x)};$x)",
        "items{(v=1?$error('key'):k):$error('value')}",
        "items{k:($f:=function(){$};$f())}",
        "items^(v).{'key':k,'value':v}",
        "($x:=0;items^($x:=$x+1);$x)",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"items":[]}"#,
            br#"{"items":[{"k":"a","v":1}]}"#,
            br#"{"items":[{"k":"a","v":2},{"k":"a","v":1}]}"#,
            br#"{"items":[{"k":"0","v":1},{"k":"a","v":2},{"k":"01","v":3}]}"#,
            br#"{"items":[[{"k":"a","v":1}],{"k":"a","v":2}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn object_members_establish_their_own_unwrapped_focus() {
    for source in [
        "{'x':{'self':$}}",
        "{'x':{'a':a}}",
        "{'x':$.(a+1)}",
        "{'x':{'n':$count($),'self':$}}",
    ] {
        for input in [
            br#"[]"#.as_slice(),
            br#"[{"a":1}]"#,
            br#"[{"a":1},{"a":2}]"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn constructor_regions_retain_lexical_bindings_and_definition_focus() {
    for source in [
        "($x:=1;{'v':($x:=$x+1)};$x)",
        "($x:=1;{'a':($x:=$x+1),'b':($x:=$x+1)};$x)",
        "($o:={'f':function(){$}};$o.f())",
        "($o:={'x':{'f':function(){$}}};$o.x.f())",
        "($x:=1;{'o':{'v':($x:=$x+1)}};$x)",
    ] {
        for input in [
            br#"[]"#.as_slice(),
            br#"[{"a":1}]"#,
            br#"[{"a":1},{"a":2}]"#,
            br#"{"a":1}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn nested_constructor_regions_do_not_duplicate_member_programs() {
    fn instructions(p: &Program) -> usize {
        p.code
            .iter()
            .map(|i| {
                let mut n = 1;
                i.kind.children(&mut |p| n += instructions(p));
                n
            })
            .sum()
    }
    let mut previous = None;
    for depth in [1, 2, 4, 8, 16] {
        let mut source = "{'v':v}".to_owned();
        for _ in 0..depth {
            source = format!("{{'child':{source},'v':v}}");
        }
        let p = crate::CompileOptions::default()
            .compile_program(&format!("items.({source})"))
            .unwrap();
        let count = instructions(&p);
        if let Some(previous) = previous {
            assert!(
                count < previous * 3,
                "duplicated member regions at {depth}: {count}"
            );
        }
        previous = Some(count);
    }
}

#[test]
fn non_iterating_regions_share_the_active_row_read_layout() {
    use super::run::{Cell, Input};
    let p = crate::CompileOptions::default()
        .compile_program("$map(items,function($doc){{'a':$doc.a,'b':$doc.b}})")
        .unwrap();
    let body = p
        .code
        .iter()
        .find_map(|i| match &i.kind {
            Operation::Each { body, .. } => Some(&**body),
            _ => None,
        })
        .unwrap();
    let region = body
        .code
        .iter()
        .find_map(|i| match &i.kind {
            Operation::Construct(body) => Some(&**body),
            _ => None,
        })
        .unwrap();
    assert!(
        Arc::ptr_eq(&body.row_schema, &region.row_schema),
        "region discarded its parent's acquired layout"
    );
    let mut acquisition = Acquisition::new([&p]);
    let document = acquisition
        .prepare(br#"{"items":[{"a":1,"b":2}]}"#)
        .unwrap();
    let input = Input::Captured(&document);
    let source = Cell::Value(Value::Raw(crate::validate(br#"[{"a":1,"b":2}]"#).unwrap()));
    input
        .each(&source, &body.row_schema, |row, _, _| {
            let reads = row.reads(region).expect("region row layout");
            assert!(reads.row.is_some());
            assert!(reads.fields.iter().any(Option::is_some));
            Ok::<_, ()>(())
        })
        .unwrap();
}

#[test]
fn immediate_callbacks_keep_argument_ports_in_retained_lexical_scopes() {
    let input = br#"{"n":3,"items":[{"v":1,"w":2},{"v":2,"w":4}]}"#;
    for source in [
        "($unused:=function($v){$v};$map(items,function($v){$v.v+$v.w}))",
        "($unused:=function($v){$v};$map(items,function($v,$i,$all){[$v.v,$i,$count($all)]}))",
        "($f:=function(){3};$n:=4;$map(items,function($v){$v.v+$n}))",
        "($f:=function(){$n};$n:=4;$map(items,function($v){$v.v+$n}))",
        "($f:=function(){$n};$n:=4;$map(items,function($v){($n:=$v.v;$n+1)});$n)",
        "($n:=4;$f:=function(){5};$map(items,function($n){$n.v}))",
        "($f:=function(){5};$map(items,function($v){missing}))",
    ] {
        compare(source, input);
    }
    let p = crate::CompileOptions::default()
        .compile_program(
            "($unused:=function($v){$v};$map(items,function($v){{'a':$v.v,'b':$v.w}}))",
        )
        .unwrap();
    assert!(
        p.code
            .iter()
            .any(|i| matches!(i.kind, Operation::Each { .. }))
    );
}

#[test]
fn acquired_predicates_preserve_shape_cardinality_and_stage_error_order() {
    for input in [
        br#"{}"#.as_slice(),
        br#"{"items":[]}"#,
        br#"{"items":[[]]}"#,
        br#"{"items":[{"v":1,"w":2},{"v":2,"w":3}]}"#,
        br#"{"items":[{"v":1,"w":null},{"v":null,"w":3}]}"#,
        br#"{"items":[{"v":1,"w":null},{"v":"bad","w":3}]}"#,
        br#"[{"items":[{"v":1,"w":2}]},{"items":[]},{}]"#,
    ] {
        for source in [
            "$sum((items[v>0]).(w*2))",
            "$sum((items[$number(v)>0]).(w*2))",
            "(items[v>0])[w>0].(v*w)",
            "(items[v>0])[-1].(w*2)",
            "($n:=0;(items[($n:=$n+1;v>0)]).($n))",
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn conditional_read_only_calls_preserve_ports_and_writing_arms_invalidate_them() {
    for source in [
        "($f:=flag?function($d){$d.a.x}:function($d){$d.b.y};[$f($),$f($)])",
        "($x:=1;$f:=flag?function(){$eval('$x:=2')}:function(){3};[$x,$f(),$x])",
        "($x:=0;$t:=|$eval('$x:=items')|{'v':v+1}|;$f:=flag?function($d){$t($d)}:function($d){$d};$f($);$x)",
    ] {
        for flag in [true, false] {
            compare(
                source,
                format!("{{\"flag\":{flag},\"a\":{{\"x\":4}},\"b\":{{\"y\":2}}}}").as_bytes(),
            );
        }
    }
    let source = "($f:=flag?function($d){$d.a.x}:function($d){$d.b.y};[$f($),$f($)])";
    let p = crate::CompileOptions::default()
        .compile_program(source)
        .unwrap();
    assert!(
        !p.code
            .iter()
            .any(|i| matches!(&i.kind, Operation::Read(n) if &**n=="f"))
    );
    for depth in [8, 48] {
        let source = format!(
            "($f:=function($v){{$v.a.x}};{}$f($))",
            "$f:=flag?$f:$f;".repeat(depth)
        );
        compare(&source, br#"{"flag":true,"a":{"x":4}}"#);
    }
}
