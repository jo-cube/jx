#![cfg(feature = "experimental")]
use jx::{CompileOptions, EvaluationOptions, Value};

fn snapshot(result: Result<jx::Evaluation<'_, '_>, jx::Error>) -> String {
    format!("{:?}", result.and_then(|v| v.collect_owned()))
}

#[test]
fn invocation_input_domains_preserve_focus_root_scope_and_identity() {
    for source in [
        "$eval(\"{'a':a,'b':b,'r':$$.b}\")",
        "$eval(\"{'a':a,'b':b,'r':$$.b}\",other)",
        "$eval(\"{'a':a,'b':b,'r':$$.b}\",missing)",
        "$eval(\"{'a':a,'b':b,'r':$$.b}\",a)",
        "($eval:=5;$eval(\"{'a':a,'b':b}\",other))",
        "($v:=other;$eval(\"{'a':$v.a,'b':b}\",($v:=$$;$v)))",
        "($eval(\"$x:=other\");$eval(\"{'v':$x.a,'r':$$.a}\",missing))",
        "($v:=other;$eval(\"{'a':a,'b':b,'r':$$.b}\",$v))",
        "($$:=other;$eval(\"{'a':a,'b':b,'r':$$.b}\"))",
        "($v:=other;$eval(\"{'a':$v.a,'b':$v.b}\"))",
        "items.($eval(\"{'a':a,'b':b,'r':$$.b}\"))",
        "($f:=$eval(\"function(){{'a':a,'b':b,'r':$$.b}}\",other);$f())",
        "($eval:=function($s,$v){$v};$eval(\"{'a':a,'b':b}\",other))",
        "$eval(\"{'same':a in [a],'v':a}\")",
        "$eval(\"$eval(\\\"{'a':a,'b':b}\\\")\")",
        "($eval(\"$x:=other\");$eval(\"{'a':$x.a,'b':$x.b}\"))",
    ] {
        let legacy = jx::compile(source).unwrap();
        let ir = jx::compile_execution(source).unwrap();
        let mut w = jx::execution::Workspace::default();
        let programs = [ir.clone(), jx::compile_execution("other.a").unwrap()];
        let plan = jx::InputPlan::new(&programs);
        for bytes in [
            br#" {"a":1,"b":2,"other":{"a":3,"b":4},"items":[{"a":5,"b":6},{"a":7}]} "#.as_slice(),
            br#"{"a":{"v":1},"a":{},"b":[1,2],"other":[{"a":3},{"b":4}]}"#,
            br#"[{"a":1,"b":2},{"a":3}]"#,
            b"{}",
        ] {
            let expected = snapshot(legacy.evaluate(bytes));
            assert_eq!(snapshot(ir.evaluate(bytes)), expected, "{source}");
            let pending = ir.evaluate_with_workspace(Some(bytes), Default::default(), &mut w);
            w.clear();
            assert_eq!(snapshot(pending), expected, "workspace: {source}");
            let prepared = plan.prepare(bytes).unwrap();
            assert_eq!(snapshot(prepared.evaluate(0)), expected, "shared: {source}");
        }
    }
}

#[test]
fn external_focus_and_root_are_independent_of_acquisition_hints() {
    let input = br#" {"a":1,"b":2,"other":{"a":3,"b":4}} "#;
    let focus_bytes = br#"{"a":7,"b":8}"#;
    for source in [
        "$eval(\"{'a':a,'b':b,'r':$$.b}\")",
        "$eval(\"{'a':a,'b':b,'r':$$.b}\",other)",
    ] {
        let legacy = jx::compile(source).unwrap();
        let ir = jx::compile_execution(source).unwrap();
        let options = || EvaluationOptions {
            focus: Some(Value::Raw(jx::validate(focus_bytes).unwrap())),
            ..Default::default()
        };
        assert_eq!(
            snapshot(ir.evaluate_with(Some(input), options())),
            snapshot(legacy.evaluate_with(Some(input), options())),
            "{source}"
        );
    }
}

#[test]
fn prepared_errors_remain_lazy_and_shadowable_with_shared_validation() {
    fn legacy_validation_error() -> jx::Error {
        jx::compile("1")
            .unwrap()
            .evaluate(b"{invalid")
            .err()
            .unwrap()
    }
    let options = CompileOptions::default().binding("eval");
    let ir = options.compile_execution("$eval('broken[')").unwrap();
    let error = ir.evaluate(b"{invalid").err().unwrap();
    assert_eq!(error, legacy_validation_error());
    // Invalid immutable code must not be inspected before call resolution.
    let legacy = options.compile("$eval('broken[')").unwrap();
    assert_eq!(
        snapshot(ir.evaluate(b"{}")),
        snapshot(legacy.evaluate(b"{}"))
    );
}
