use super::*;

#[test]
fn temporary_captures_release_their_frames_before_record_completion() {
    for source in [
        "$map(a,function($x){($f:=function(){$x};$f()+1)})",
        "$map(a,function($x){($f:=function(){$x};$f())})",
    ] {
        let expression = crate::CompileOptions::default()
            .compile_program(source)
            .unwrap();
        let input = crate::validate(b"{\"a\":[1,2,3,4,5,6,7,8]}").unwrap();
        let scope = Scope::with_random(Value::Raw(input), false, None);
        let context = Context {
            value: Value::Raw(input),
            wrapped: true,
            scope: Some(scope.clone()),
        };
        let value = expression.evaluate_region(&context).unwrap().unwrap();
        assert_eq!(value.elements().count(), 8);
        let frames = scope.runtime.frames.borrow();
        assert!(
            frames.len() <= 2,
            "temporary captures retained {} frames",
            frames.len()
        );
        assert!(frames[1..].iter().all(|f| f.vacant));
    }
}

#[test]
fn an_older_frame_write_keeps_its_new_capture_alive() {
    let expression = crate::CompileOptions::default()
        .compile_program("function(){$x}")
        .unwrap();
    let scope = Scope::with_random(Value::Null, false, None);
    scope
        .retained(|| {
            let child = scope.child(0);
            child.bind("x", Value::Number(7.0));
            let context = Context {
                value: Value::Null,
                wrapped: false,
                scope: Some(child.clone()),
            };
            scope.bind(
                "saved",
                expression.evaluate_region(&context).unwrap().unwrap(),
            );
            child.release();
            Ok(None)
        })
        .unwrap();
    let Value::Function(saved) = scope.lookup("saved").unwrap() else {
        panic!("function")
    };
    let context = Context {
        value: Value::Null,
        wrapped: false,
        scope: Some(scope.clone()),
    };
    assert!(matches!(
        crate::function::invoke(&saved, &[], &context, 0).unwrap(),
        crate::evaluate::Operand::One(Value::Number(7.0))
    ));
}

#[test]
fn closure_focus_is_an_escape_root_even_with_an_older_frame() {
    let expression = crate::CompileOptions::default()
        .compile_program("function(){$}")
        .unwrap();
    let scope = Scope::with_random(Value::Null, false, None);
    let value = scope
        .retained(|| {
            let child = scope.child(0);
            let local = Context {
                value: Value::Number(9.0),
                wrapped: false,
                scope: Some(child.clone()),
            };
            let captured = expression.evaluate_region(&local).unwrap().unwrap();
            let older = Context {
                value: captured,
                wrapped: false,
                scope: Some(scope.clone()),
            };
            let result = expression.evaluate_region(&older).unwrap().unwrap();
            child.release();
            Ok(Some(result))
        })
        .unwrap()
        .unwrap();
    let Value::Function(first) = value else {
        panic!("first")
    };
    let context = Context {
        value: Value::Null,
        wrapped: false,
        scope: Some(scope.clone()),
    };
    let crate::evaluate::Operand::One(Value::Function(second)) =
        crate::function::invoke(&first, &[], &context, 0).unwrap()
    else {
        panic!("second")
    };
    assert!(matches!(
        crate::function::invoke(&second, &[], &context, 0).unwrap(),
        crate::evaluate::Operand::One(Value::Number(9.0))
    ));
}

#[test]
fn compiled_transform_regions_reclaim_discarded_closure_environments() {
    let program = crate::CompileOptions::default()
        .compile_program("($f:=function(){1};$f();1)")
        .unwrap();
    let scope = Scope::with_random(Value::Null, false, None);
    let context = Context {
        value: Value::Null,
        wrapped: false,
        scope: Some(scope.clone()),
    };
    for _ in 0..128 {
        let value = crate::transform::Body::evaluate(&program, &context)
            .unwrap()
            .unwrap();
        assert_eq!(value.as_number(), Some(1.0));
        assert!(
            scope.runtime.frames.borrow().len() <= 2,
            "discarded region captures retained frames"
        );
    }
}

#[test]
fn dynamic_program_regions_reclaim_discarded_closure_environments() {
    let scope = Scope::with_random(Value::Null, false, None);
    let context = Context {
        value: Value::Null,
        wrapped: false,
        scope: Some(scope.clone()),
    };
    let source = Value::from_string("($f:=function(){1};$f();1)");
    for _ in 0..128 {
        crate::dynamic::Builtin::Eval
            .values(&[Some(source.clone())], &context, 0)
            .unwrap();
        assert!(
            scope.runtime.frames.borrow().len() <= 2,
            "discarded dynamic captures retained frames"
        );
    }
}

#[test]
fn correlated_regions_reclaim_per_row_captures() {
    let source = "items#$i.($f:=function(){$i};$f())";
    let program = crate::CompileOptions::default()
        .compile_program(source)
        .unwrap();
    let input = format!("{{\"items\":[{}]}}", vec!["0"; 128].join(","));
    let raw = crate::validate(input.as_bytes()).unwrap();
    let scope = Scope::with_random(Value::Raw(raw), false, None);
    let context = Context {
        value: Value::Raw(raw),
        wrapped: true,
        scope: Some(scope.clone()),
    };
    for _ in 0..8 {
        let value = program.evaluate_region(&context).unwrap().unwrap();
        assert_eq!(value.elements().count(), 128);
        let frames = scope.runtime.frames.borrow();
        assert!(frames.len() <= 4, "{} frames", frames.len());
        assert!(frames[1..].iter().all(|f| f.vacant));
    }
}
