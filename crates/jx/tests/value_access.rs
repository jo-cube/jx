#[path = "support/embedding.rs"]
mod common;
use common::{binding, items};
use jx::{CompileOptions, ErrorKind, Value};
use std::borrow::Cow;
#[test]
fn decoded_access_works_across_storage_and_owned_snapshots() {
    let input = br#"{"n":2,"b":true,"s":"hi","escaped":"h\u0069","nil":null,"items":[1,2],"surrogate":"\ud800"}"#;
    let value = Value::from_json(input).unwrap();
    assert_eq!(value.value_type(), jx::ValueType::Object);
    assert_eq!(value.get("n").unwrap().as_number(), Some(2.0));
    assert_eq!(value.get("b").unwrap().as_bool(), Some(true));
    assert!(value.get("nil").unwrap().is_null());
    assert!(matches!(
        value.get("s").unwrap().as_str().unwrap(),
        Some(Cow::Borrowed("hi"))
    ));
    assert!(
        matches!(value.get("escaped").unwrap().as_str().unwrap(),Some(Cow::Owned(s)) if s=="hi")
    );
    let surrogate = value.get("surrogate").unwrap();
    assert_eq!(
        surrogate.as_str().unwrap_err().kind,
        ErrorKind::EncodingError
    );
    assert_eq!(
        surrogate.string_units().unwrap().collect::<Vec<_>>(),
        vec![0xd800]
    );
    assert_eq!(
        value
            .get("items")
            .unwrap()
            .array_items()
            .unwrap()
            .map(|v| v.as_number().unwrap())
            .collect::<Vec<_>>(),
        vec![1., 2.]
    );
    assert_eq!(value.object_entries().unwrap().count(), 7);
    for source in ["$", "{'n':n,'s':s}", "$ ~> |$|{'extra':3}|"] {
        let owned = {
            let expr = jx::compile(source).unwrap();
            let mut owned = None;
            expr.evaluate(input)
                .unwrap()
                .for_each(|v| owned = Some(v.to_owned().unwrap()))
                .unwrap();
            owned.unwrap()
        };
        assert_eq!(owned.as_value().get("n").unwrap().as_number(), Some(2.));
        assert_eq!(
            owned
                .as_value()
                .get("s")
                .unwrap()
                .as_str()
                .unwrap()
                .unwrap(),
            "hi"
        );
    }
    assert!(Value::Undefined.to_owned().unwrap().as_value().is_missing());
    assert!(
        Value::Number(f64::INFINITY)
            .to_owned()
            .unwrap()
            .as_value()
            .as_number()
            .unwrap()
            .is_infinite()
    );
    let expr = jx::compile("[function(){1}]").unwrap();
    expr.evaluate(b"null")
        .unwrap()
        .for_each(|v| assert_eq!(v.to_owned().unwrap_err().kind, ErrorKind::TypeError))
        .unwrap();
    fn shared<T: Send + Sync>() {}
    shared::<jx::Expression>();
    shared::<jx::OwnedValue>();
    shared::<jx::HostFunction>();
}

#[test]
fn owned_results_keep_sequence_shape_and_duplicate_keys_and_reject_cross_arena_closures() {
    let expr = jx::compile("rows.a").unwrap();
    assert_eq!(
        expr.evaluate(b"{}").unwrap().single().unwrap().map(|_| ()),
        None
    );
    assert_eq!(
        expr.evaluate(br#"{"rows":[{"a":1},{"a":2}]}"#)
            .unwrap()
            .single()
            .unwrap_err()
            .kind,
        ErrorKind::CardinalityError
    );
    let owned = expr
        .evaluate(br#"{"rows":[{"a":1},{"a":2}]}"#)
        .unwrap()
        .collect_owned()
        .unwrap();
    assert_eq!(owned.len(), 2);
    assert_eq!(owned[1].as_value().as_number(), Some(2.));
    let bound_expr = CompileOptions::default()
        .binding("v")
        .compile("($v:=rows.a; $v)")
        .unwrap();
    let owned = bound_expr
        .evaluate(br#"{"rows":[{"a":1},{"a":2}]}"#)
        .unwrap()
        .collect_owned()
        .unwrap();
    assert_eq!(owned.len(), 2);
    let retained_expr = jx::compile("{'v':rows.a}").unwrap();
    let owned = retained_expr
        .evaluate(br#"{"rows":[{"a":1},{"a":2}]}"#)
        .unwrap()
        .single()
        .unwrap()
        .unwrap()
        .get("v")
        .unwrap()
        .to_owned()
        .unwrap();
    let expr = CompileOptions::default()
        .binding("v")
        .compile("$v")
        .unwrap();
    assert_eq!(
        items(&expr, None, binding("v", owned.as_value())).unwrap(),
        vec![b"1".to_vec(), b"2".to_vec()]
    );
    let owned = jx::OwnedValue::from_json(br#"{"a":1,"\u0061":2}"#).unwrap();
    assert_eq!(owned.as_value().get("a").unwrap().as_number(), Some(2.));
    let closure_expr = jx::compile("function(){3}").unwrap();
    let closure = closure_expr
        .evaluate(b"null")
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    assert_eq!(
        expr.evaluate_with(None, binding("v", closure))
            .unwrap_err()
            .kind,
        ErrorKind::BindingError
    );
}
