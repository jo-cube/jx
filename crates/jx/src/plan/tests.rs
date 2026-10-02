use super::*;

// The tree is the diagnostic fallback and must agree even on IEEE edge cases.
#[derive(Debug, PartialEq)]
enum Snapshot {
    Number(u64),
    Boolean(bool),
    Array(Vec<Snapshot>),
    Object(Vec<(String, Snapshot)>),
    Json(Vec<u8>),
}
fn snapshot(value: Value<'_, '_>) -> Snapshot {
    match value.atomic() {
        Value::Number(n) => Snapshot::Number(n.to_bits()),
        Value::Boolean(b) => Snapshot::Boolean(b),
        value if value.is_array() => Snapshot::Array(value.elements().map(snapshot).collect()),
        value if value.is_object() => Snapshot::Object(
            value
                .members()
                .map(|(k, v)| (k.to_owned(), snapshot(v)))
                .collect(),
        ),
        value => {
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            Snapshot::Json(bytes)
        }
    }
}
fn result(node: &Node, input: &[u8]) -> Result<Vec<Snapshot>, crate::Error> {
    let mut items = Vec::new();
    crate::evaluate::scalar(node, input, None, node.effects || node.clock)?
        .for_each(|value| items.push(snapshot(value)))?;
    Ok(items)
}

#[test]
fn plan_and_tree_agree_on_values_and_exact_errors() {
    let atoms = [
        "null", "false", "0", "-0", "2", "-3.5", "1e308", "1e999", "1e-320", "[]", "[1]", "[1,2]",
        "{}", "\"x\"",
    ];
    for source in [
        "((x+y)*(x-y)+(x*x+y*y))/(x+1)-y*3",
        "-(x*x+x)-y",
        "(x+x+x)%y",
        "(x+x+x)/y",
        "(x+x+x)<y",
        "(x+x+x)>=y",
        "(x+x+x)/0<y",
        "(x-x+x-x)/0<y",
        "((x<y)<x)<y",
        "missing+missing+missing+x",
        "((x+x)+x)+(y/y+y)",
        "(x-x+x-x)/0",
        "x+x+$.x+$.x",
    ] {
        let expression = crate::compile(source).unwrap();
        let Kind::Plan(plan) = &expression.root.kind else {
            panic!("expected a numeric region: {source}");
        };
        for x in atoms {
            for y in atoms {
                let object = format!(r#"{{"x":{x},"y":{y}}}"#);
                for input in [
                    object.clone(),
                    format!("[{object}]"),
                    format!("[{object},{object}]"),
                    format!(r#"{{"y":{y}}}"#),
                ] {
                    assert_eq!(
                        result(&expression.root, input.as_bytes()),
                        result(&plan.source, input.as_bytes()),
                        "{source}: {input}"
                    );
                }
            }
        }
    }
}

#[test]
fn branches_and_folds_match_the_tree() {
    let atoms = [
        "null", "false", "true", "0", "-0", "2", "-3.5", "1e308", "1e999", "[]", "[1]", "[1,2]",
        "{}", "\"x\"",
    ];
    let expressions = [
        "x>y ? x*x+y*y : x-y",
        "x and y>0 and (x+y<20 or y=0)",
        "(x>0 ? y+y : y*y) + y + y",
        "(x>0 ? (y>0 ? x+y : x-y) : x*y)+y",
        "x>y ? (x+x+x) : (y+y+y)",
        "x+x+x != y",
    ];
    for source in expressions {
        let expression = crate::compile(source).unwrap();
        let Kind::Plan(plan) = &expression.root.kind else {
            panic!("not lowered: {source}")
        };
        for x in atoms {
            for y in atoms {
                let input = format!(r#"{{"x":{x},"y":{y}}}"#);
                assert_eq!(
                    result(&expression.root, input.as_bytes()),
                    result(&plan.source, input.as_bytes()),
                    "{source}: {input}"
                );
            }
        }
    }
    for aggregate in ["sum", "min", "max", "count"] {
        for body in [
            "rows.(x*x+x+1)",
            "rows[x>0].(x*y+1)",
            "rows[x>0 and y>0].(x*y+1)",
            "rows[x>0][y>0].(x*y+1)",
            "rows[x>0].x",
        ] {
            let source = format!("${aggregate}({body})");
            let expression = crate::compile(&source).unwrap();
            let Kind::Plan(plan) = &expression.root.kind else {
                panic!("not lowered: {source}: {:?}", expression.root)
            };
            for x in atoms {
                for y in atoms {
                    let a = format!(r#"{{"x":{x},"y":{y}}}"#);
                    for rows in [
                        a.clone(),
                        format!("[{a}]"),
                        format!("[{a},{a}]"),
                        format!("[[{a}],{a}]"),
                        format!("[{a},null,{{}},{{\"x\":2,\"y\":3}}]"),
                        "[]".into(),
                        "null".into(),
                    ] {
                        let object = format!(r#"{{"rows":{rows}}}"#);
                        for input in [
                            object.clone(),
                            format!("[{object}]"),
                            format!("[{object},{object}]"),
                        ] {
                            assert_eq!(
                                result(&expression.root, input.as_bytes()),
                                result(&plan.source, input.as_bytes()),
                                "{source}: {input}"
                            );
                        }
                    }
                }
            }
            assert_eq!(result(&expression.root, b"{}"), result(&plan.source, b"{}"));
        }
    }
}

#[test]
fn whole_regions_lower() {
    for source in [
        "$sum(payload.orders[active and price>3].(price*qty+1))",
        "active and x > y and (x+y < 20 or y=0)",
    ] {
        let expression = crate::compile(source).unwrap();
        assert!(
            matches!(expression.root.kind, Kind::Plan(_)),
            "{source}: {:#?}",
            expression.root
        );
    }
}

#[test]
fn fixed_objects_lookup_and_capture_match_the_tree() {
    let atoms = [
        "null", "false", "true", "0", "-0", "2", "1e308", "1e999", "[]", "[1]", "{}", "\"x\"",
    ];
    for source in [
        r#"{"sum":x+y,"product":x*y,"schema":{"v":1}}"#,
        r#"{"2":x+x,"1":y+y,"other":x>y ? x+y : x-y}"#,
        r#"{"m":missing+missing,"n":null,"s":"a\nb","value":x*x+x}"#,
        r#"x>0 ? $lookup({"x":3,"y":7,"undefined":9},key)*y+y*y+y : 0"#,
    ] {
        let expression = crate::compile(source).unwrap();
        let Kind::Plan(plan) = &expression.root.kind else {
            panic!("not lowered: {source}")
        };
        for x in atoms {
            for y in atoms {
                for key in ["\"x\"", "\"y\"", "\"absent\"", "null"] {
                    let object = format!(r#"{{"x":{x},"y":{y},"key":{key}}}"#);
                    for input in [
                        object.clone(),
                        format!("[{object}]"),
                        format!("[{object},{object}]"),
                        format!(r#"{{"x":{x},"\u0078":{y},"y":2}}"#),
                    ] {
                        assert_eq!(
                            result(&expression.root, input.as_bytes()),
                            result(&plan.source, input.as_bytes()),
                            "{source}: {input}"
                        );
                    }
                }
            }
        }
    }
    for (source, input) in [
        ("x>0 ? x*x+x : y+y+y", br#"{"x":2,"y":null}"#.as_slice()),
        ("x>0 and (y+y+y>0)", br#"{"x":0,"y":{}}"#.as_slice()),
        ("x>0 or (y+y+y>0)", br#"{"x":2,"y":[1,2]}"#.as_slice()),
    ] {
        let expression = crate::compile(source).unwrap();
        let Kind::Plan(plan) = &expression.root.kind else {
            panic!()
        };
        let context = Context {
            value: Value::Raw(crate::validate(input).unwrap()),
            wrapped: true,
            scope: None,
        };
        assert!(
            plan.run(&context).is_some(),
            "unselected field caused fallback: {source}"
        );
        assert_eq!(result(&expression.root, input), result(&plan.source, input));
    }
}

#[test]
fn nested_demands_preserve_replacement_and_array_fallback() {
    for source in [
        "payload.x*payload.x+payload.y*payload.y",
        "active ? payload.x*payload.y+1 : 0",
        r#"{"n":payload.x+payload.y,"m":payload.x*payload.y}"#,
        r#"$lookup({"a":3,"undefined":9},payload.key)*payload.x+payload.x+1"#,
        "$sum(payload.rows[x>0].(x*y+1))",
    ] {
        let expression = crate::compile(source).unwrap();
        let Kind::Plan(plan) = &expression.root.kind else {
            panic!("{source}");
        };
        for input in [
            r#"{"active":true,"payload":{"x":2,"y":3,"key":"a","rows":[{"x":2,"y":3}]}}"#,
            r#"{"active":false,"payload":{"x":null,"y":[1,2]}}"#,
            r#"{"payload":{"x":2,"y":3},"payload":{"y":4}}"#,
            r#"{"payload":{"x":2,"y":3},"\u0070ayload":null}"#,
            r#"{"payload":[{"x":2,"y":3}]}"#,
            r#"{"payload":[{"x":2,"y":3}],"payload":{"x":4,"y":5}}"#,
            r#"{"payload":{"x":2,"y":3},"payload":[]}"#,
            r#"{"payload":{"rows":[{"x":2,"y":3}]},"payload":{}}"#,
            r#"[{"payload":{"x":2,"y":3}}]"#,
            r#"{"payload":{"x":2,"\u0078":4,"y":3}}"#,
            r#"{}"#,
        ] {
            assert_eq!(
                result(&expression.root, input.as_bytes()),
                result(&plan.source, input.as_bytes()),
                "{source}: {input}"
            );
        }
    }
}
