use super::*;

#[test]
fn temporary_captures_release_their_frames_before_record_completion() {
    for source in [
        "$map(a,function($x){($f:=function(){$x};$f()+1)})",
        "$map(a,function($x){($f:=function(){$x};$f())})",
    ] {
        let expression = crate::compile(source).unwrap();
        let input = crate::validate(b"{\"a\":[1,2,3,4,5,6,7,8]}").unwrap();
        let scope = Scope::with_random(Value::Raw(input), false, None);
        let context = Context {
            value: Value::Raw(input),
            wrapped: true,
            scope: Some(scope.clone()),
        };
        let value = crate::retain::materialize(&expression.root, &context)
            .unwrap()
            .unwrap();
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
    let expression = crate::compile("function(){$x}").unwrap();
    let crate::expression::Kind::Lambda(definition) = &expression.root.kind else {
        panic!("lambda")
    };
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
            scope.bind("saved", crate::Function::lambda(definition, &context));
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
    let expression = crate::compile("function(){$}").unwrap();
    let crate::expression::Kind::Lambda(definition) = &expression.root.kind else {
        panic!("lambda")
    };
    let scope = Scope::with_random(Value::Null, false, None);
    let value = scope
        .retained(|| {
            let child = scope.child(0);
            let local = Context {
                value: Value::Number(9.0),
                wrapped: false,
                scope: Some(child.clone()),
            };
            let captured = crate::Function::lambda(definition, &local);
            let older = Context {
                value: captured,
                wrapped: false,
                scope: Some(scope.clone()),
            };
            let result = crate::Function::lambda(definition, &older);
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
