use super::*;

fn outcome(result: Result<Evaluation<'_, '_>, Error>) -> Result<Vec<Vec<u8>>, Error> {
    let mut items = Vec::new();
    result?.for_each(|value| {
        let mut bytes = Vec::new();
        match value {
            Value::Number(n) => bytes.extend(n.to_bits().to_le_bytes()),
            value => value.write_compact(&mut bytes).unwrap(),
        }
        items.push(bytes);
    })?;
    Ok(items)
}
#[test]
fn acquisition_preserves_values_cardinality_and_exact_errors() {
    let atoms = [
        "null",
        "true",
        "0",
        "-0",
        "2",
        "1e999",
        "1e-320",
        "[]",
        "[1]",
        "[[1],[2]]",
        "{}",
        "\"x\"",
        "\"\\ud800\"",
    ];
    for source in [
        "a&':'&$string(b)&':'&a",
        "a&$number(b)&a",
        "a?($string(a)&b&b):($string(b)&a&a)",
        "{'name':a&':'&a,'n':$string(b+b),'raw':b,'nested':{'v':a}}",
        "{'2':a,'1':b,'x':a&b,'absent':missing}",
        "{'x':a&b,'\\u0078':a}",
        "[a,b,a,[a,b]]",
        "a.x&$string(b.x)&a.x",
        "{'s':a&b,'compare':a=b,'in':a in b,'order':a<b}",
        "a&($lookup({'x':1},b))&a",
        "(a ?? b)&a",
        "(a ?: b)&a",
        "$exists(a) ? (a ?? b) : (b ?? a)",
        "a.x ?? (b.x ?? a.x)",
    ] {
        let expression = crate::compile(source).unwrap();
        assert!(expression.region.is_some(), "{source}");
        for a in atoms {
            for b in atoms {
                let object = format!(r#"{{"a":{a},"b":{b}}}"#);
                for input in [
                    object.clone(),
                    format!("[{object}]"),
                    format!("[{object},{object}]"),
                    format!(r#"{{"a":{a},"b":{b},"\u0061":null}}"#),
                    format!(r#"{{"b":{b}}}"#),
                ] {
                    assert_eq!(
                        outcome(expression.evaluate(input.as_bytes())),
                        outcome(crate::evaluate::scalar(
                            &expression.root,
                            input.as_bytes(),
                            None,
                            false
                        )),
                        "{source}: {input}"
                    );
                }
            }
        }
    }
}
#[test]
fn validation_still_precedes_expression_errors_and_output() {
    let expression = crate::compile("a&$number(b)&a").unwrap();
    for input in [
        r#"{"a":"x","b":"invalid","unused":[1,]}"#,
        r#"{"a":"x","b":"invalid"} trailing"#,
    ] {
        assert_eq!(
            outcome(expression.evaluate(input.as_bytes())),
            outcome(crate::evaluate::scalar(
                &expression.root,
                input.as_bytes(),
                None,
                false
            ))
        );
        assert_eq!(
            expression.evaluate(input.as_bytes()).unwrap_err().kind,
            crate::ErrorKind::InvalidJson
        );
    }
}
#[test]
fn captures_keep_borrowing_and_do_not_escape_into_results() {
    let expression = crate::compile("{'a':a,'b':b,'c':a}").unwrap();
    assert!(expression.region.is_some());
    let input = br#"{"a":{"untouched":1},"b":"hello"}"#;
    let value = expression
        .evaluate(input)
        .unwrap()
        .single()
        .unwrap()
        .unwrap();
    let members = value.members().collect::<Vec<_>>();
    assert_eq!(members.len(), 3);
    for (_, value) in members {
        let raw = value.as_raw().unwrap();
        let start = raw.as_bytes().as_ptr() as usize - input.as_ptr() as usize;
        assert_eq!(&input[start..start + raw.as_bytes().len()], raw.as_bytes());
    }
    assert!(value.to_owned().is_ok());
}
#[test]
fn dynamic_contexts_effects_and_unbounded_demands_use_normal_execution() {
    for source in [
        "a",
        "$string(a)",
        "a+a",
        "rows.{'a':a,'b':b,'c':a}",
        "$map(rows,function($r){$r.a&$r.b&$r.a})",
        "rows^(a&b&a)",
        "rows{a&b&a:c}",
        "a&$random()&a",
        "a&$eval(b)&a",
        "a&$error('stop')&a",
        "a&$assert(b)&a",
        "($a:=a;$a&b&$a)",
        "$$.a&b&$$.a",
    ] {
        assert!(crate::compile(source).unwrap().region.is_none(), "{source}");
    }
    let source = (0..33)
        .map(|i| format!("field{i}"))
        .collect::<Vec<_>>()
        .join("&");
    assert!(crate::compile(&source).unwrap().region.is_none());
}
#[test]
fn focus_bindings_and_controls_keep_their_existing_evaluation_path() {
    let expression = crate::compile("a&$string(b)&a").unwrap();
    assert!(expression.region.is_some());
    let input = br#"{"a":"x","b":2}"#;
    let cancellation = crate::Cancellation::default();
    cancellation.cancel();
    assert_eq!(
        expression
            .evaluate_with(
                Some(input),
                crate::EvaluationOptions {
                    cancellation: Some(cancellation),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .kind,
        crate::ErrorKind::Cancelled
    );
    let focus = Value::Raw(crate::validate(br#"{"a":"y","b":3}"#).unwrap());
    assert_eq!(
        outcome(expression.evaluate_with(
            Some(input),
            crate::EvaluationOptions {
                focus: Some(focus),
                ..Default::default()
            }
        ))
        .unwrap(),
        vec![b"\"y3y\"".to_vec()]
    );
}

#[cfg(feature = "jit")]
#[test]
fn captured_numeric_subplans_keep_native_and_tree_guards() {
    let source = "{'s':n&n&n,'v':(a+b)*(a-b)}";
    let mut expression = crate::compile(source).unwrap();
    assert!(expression.region.is_some());
    assert!(expression.enable_native().kernels > 0);
    for input in [
        r#"{"n":"x","a":2,"b":3}"#,
        r#"{"n":"x","a":2,"b":null}"#,
        r#"{"n":"x","b":3}"#,
        r#"{"n":"x","a":[],"b":3}"#,
        r#"[{"n":"x","a":2,"b":3}]"#,
    ] {
        let original = crate::compile(source).unwrap();
        assert_eq!(
            outcome(expression.evaluate(input.as_bytes())),
            outcome(original.evaluate(input.as_bytes())),
            "{input}"
        );
    }
}

#[test]
fn pure_missing_and_fallback_demands_are_bounded_static_paths() {
    for source in [
        "a ?? b",
        "a ?: b",
        "$exists(a) ? a : b",
        "$lookup({'x':1},a&'-'&b) ?? $lookup({'x':1},c)",
        "flag ? (a ?? b) : (b ?? c)",
    ] {
        let expression = crate::compile(source).unwrap();
        assert!(paths(&expression.root, 1).is_some(), "{source}");
    }
    for source in [
        "a ?? $random()",
        "a ?? $now()",
        "a ?? $error('untaken')",
        "a ?? $eval(b)",
        "($exists:=function($v){false}; a ?? b)",
        "flag ? (a ?? b) : $lowercase(c)",
    ] {
        let expression = crate::compile(source).unwrap();
        assert!(paths(&expression.root, 1).is_none(), "{source}");
    }
    let source = (0..33)
        .map(|i| format!("field{i}"))
        .collect::<Vec<_>>()
        .join(" ?? ");
    assert!(paths(&crate::compile(&source).unwrap().root, 1).is_none());
}
