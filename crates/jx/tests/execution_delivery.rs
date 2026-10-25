#![cfg(feature = "experimental")]
use jx::{CompileOptions, EvaluationOptions, Value};

fn outcome(e: &jx::Expression, input: &[u8]) -> String {
    snapshot(e.evaluate(input))
}
fn snapshot(e: Result<jx::Evaluation<'_, '_>, jx::Error>) -> String {
    format!("{:?}", e.and_then(|r| r.collect_owned()))
}

#[test]
fn prepared_dynamic_code_preserves_scope_focus_effects_and_lazy_failures() {
    let options = CompileOptions::default()
        .constant_binding("source", jx::OwnedValue::from_json(br#""a+b""#).unwrap());
    for source in [
        "$eval('a+b')",
        "($f:=function(){[$eval('{}')][0]};$a:=$f();$b:=$f();[$a in[$a],$a in[$b]])",
        "$eval($source)",
        "false?$eval('broken['):a",
        "$eval('broken[')",
        "($eval:=function($s){$s};$eval('broken['))",
        "($eval:=3;$eval('broken['))",
        "($eval('broken[', $error('argument')))",
        "($x:=2;$eval('$x+a'))",
        "($g:=null;$eval('$g:=function(){a}');$g())",
        "($eval('a', ($eval:=function($s){99};{'a':7})))",
        "($eval('$x:=7');$x)",
        "($f:=$eval('function($x){$x+a}');$f(3))",
        "($f:=$eval('function($x){$x+a}');$g:=$eval('function($x){$x+a}');$f in [$g])",
        "$eval('a', {'a':7})",
        "$eval('$$.a', {'a':7})",
        "$eval('$', [1,2])",
        "$eval('$', null)",
        "$eval('$eval(\"a+1\")')",
        "$eval('$error(\"body\")')",
        "($s:=source;$eval($s))",
        "$eval(source)",
        "$eval('\\uD800')",
        "($f:=function(){$eval('a')};{'a':9}.($f()))",
        "($fs:=$map([1,2,3],function($x){$eval('function(){$x}')});$map($fs,function($f){$f()}))",
    ] {
        let legacy = options.compile(source).unwrap();
        let ir = options.compile_execution(source).unwrap();
        for input in [
            br#"{"a":1,"b":2,"source":"a+3"}"#.as_slice(),
            br#"{"a":[1,2],"b":3,"source":"bad["}"#,
            br#"{}"#,
        ] {
            for _ in 0..3 {
                assert_eq!(outcome(&ir, input), outcome(&legacy, input), "{source}");
            }
        }
    }
}

#[test]
fn completed_fields_keep_borrowing_shape_and_independent_result_lifetimes() {
    for source in ["a", "$.a", "a.b", "$$.a"] {
        let ir = jx::compile_execution(source).unwrap();
        let legacy = jx::compile(source).unwrap();
        let mut w = jx::execution::Workspace::default();
        for input in [
            b" {} ".as_slice(),
            br#"{"a":null}"#,
            br#"{"a":{"b":"value"}}"#,
            br#"{"a":[1,2]}"#,
            br#"[{"a":1},{"a":2}]"#,
            br#"{"a":[{"b":[1,2]},{"b":3}]}"#,
        ] {
            let pending = ir
                .evaluate_with_workspace(Some(input), Default::default(), &mut w)
                .unwrap();
            let _ = ir
                .evaluate_with_workspace(Some(b"{}"), Default::default(), &mut w)
                .unwrap();
            w.clear();
            assert_eq!(snapshot(Ok(pending)), outcome(&legacy, input), "{source}");
        }
    }
    let e = jx::compile_execution("a").unwrap();
    let bytes = br#"{"a":"value"}"#;
    let v = e.evaluate(bytes).unwrap().single().unwrap().unwrap();
    assert_eq!(v.as_raw().unwrap().as_bytes().as_ptr(), bytes[5..].as_ptr());
}

#[test]
fn contextual_settings_keep_streaming_cancellation_and_error_precedence() {
    let input = br#"{"groups":[{"name":"p","items":[{"v":1},{"v":2},{"v":3}]}]}"#;
    for source in [
        "groups.items.{'v':v,'p':%.name}",
        "groups.items@$item#$i.{'v':$item.v,'i':$i}",
    ] {
        let e = jx::compile_execution(source).unwrap();
        let root = jx::validate(input).unwrap();
        let r = e
            .evaluate_with(
                Some(input),
                EvaluationOptions {
                    focus: Some(Value::Raw(root)),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(snapshot(Ok(r)), outcome(&e, input));
        let mut count = 0;
        let r = e
            .evaluate_with(
                Some(input),
                EvaluationOptions {
                    focus: Some(Value::Raw(root)),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(
            r.try_for_each(|_| {
                count += 1;
                Err("stop")
            }),
            Err(jx::ConsumeError::Consumer("stop"))
        );
        assert_eq!(count, 1);
    }
}

#[test]
fn fallible_contextual_settings_still_fail_before_returning_results() {
    let input = br#"{"groups":[{"name":"p","items":[{"v":1},{"v":"bad"}]}]}"#;
    for source in [
        "groups.items.{'v':$number(v),'p':%.name}",
        "groups.items.{'p':%.name,'v':$error('failure')}",
    ] {
        let root = jx::validate(input).unwrap();
        let legacy = jx::compile(source).unwrap();
        let ir = jx::compile_execution(source).unwrap();
        let settings = || EvaluationOptions {
            focus: Some(Value::Raw(root)),
            ..Default::default()
        };
        let expected = legacy.evaluate_with(Some(input), settings()).err();
        assert!(expected.is_some(), "legacy error timing: {source}");
        assert_eq!(
            ir.evaluate_with(Some(input), settings()).err(),
            expected,
            "{source}"
        );
    }
}

#[test]
fn shared_prepared_dynamic_code_never_captures_a_previous_evaluation() {
    let e = std::sync::Arc::new(
        CompileOptions::default()
            .binding("delta")
            .compile_execution("$eval('a+$delta')")
            .unwrap(),
    );
    std::thread::scope(|scope| {
        for worker in 0..4 {
            let e = e.clone();
            scope.spawn(move || {
                for a in 0..50 {
                    let bytes = format!("{{\"a\":{a}}}");
                    let bindings = vec![("delta", Value::Number(worker as f64))];
                    let r = e
                        .evaluate_with(
                            Some(bytes.as_bytes()),
                            EvaluationOptions {
                                bindings,
                                ..Default::default()
                            },
                        )
                        .unwrap()
                        .single()
                        .unwrap()
                        .unwrap();
                    assert_eq!(r.as_number(), Some((a + worker) as f64));
                }
            });
        }
    });
}
