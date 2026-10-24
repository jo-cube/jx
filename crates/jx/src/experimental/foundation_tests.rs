use super::*;
use crate::ConsumeError;
fn results(source: &str, input: &[u8]) {
    let expression = crate::compile(source).unwrap();
    let p = crate::CompileOptions::default()
        .compile_program(source)
        .unwrap();
    let mut a = Acquisition::new([&p]);
    let mut values = Vec::new();
    let actual = a
        .prepare(input)
        .map_err(ConsumeError::Evaluation)
        .and_then(|d| {
            p.try_for_each(&d, |v| {
                values.push(v.to_owned().unwrap());
                Ok::<_, ()>(())
            })
        });
    let expected = expression.evaluate(input).and_then(|v| v.collect_owned());
    match (actual, expected) {
        (Ok(()), Ok(expected)) => {
            assert_eq!(format!("{values:?}"), format!("{expected:?}"), "{source}")
        }
        (Err(ConsumeError::Evaluation(a)), Err(b)) => assert_eq!(a, b, "{source}"),
        (a, b) => panic!("{source}: {a:?} {b:?}"),
    }
}
#[test]
fn captures_are_scope_specific_unbounded_and_replace_duplicate_subtrees() {
    for source in [
        "{'a':a.x,'b':b.x,'c':a.y}",
        "($m:=a;{'x':$m.x,'y':$m.y})",
        "$$.a.x+a.y",
    ] {
        for input in [
            br#"{"a":{"x":1,"y":2},"b":{"x":3},"x":[1,2]}"#.as_slice(),
            br#"{"a":{"x":1,"y":2},"a":{},"b":{"x":3}}"#,
            br#"{"a":{"x":1},"\u0061":{"y":2}}"#,
        ] {
            results(source, input);
        }
    }
    let source = format!(
        "{{{}}}",
        (0..200)
            .map(|i| format!("'v{i}':a.v{i}+a.v{i}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let input = format!(
        r#"{{"a":{{{}}}}}"#,
        (0..200)
            .map(|i| format!(r#""v{i}":{i}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    results(&source, input.as_bytes());
}
#[test]
fn paths_cancel_but_callbacks_finish_before_output() {
    let input = br#"{"items":[{"v":2},{"v":null}]}"#;
    for source in [
        "items.(v*2)",
        "(items[v>0]).(v*2)",
        "$map(items,function($v){$v.v*2})",
    ] {
        let e = crate::compile(source).unwrap();
        let p = Program::compile(&e).unwrap();
        let mut a = Acquisition::new([&p]);
        let d = a.prepare(input).unwrap();
        let expected = e
            .evaluate(input)
            .map_err(ConsumeError::Evaluation)
            .and_then(|v| v.try_for_each(|_| Err("stop")));
        let actual = p.try_for_each(&d, |_| Err("stop"));
        assert_eq!(actual, expected, "{source}");
    }
}
#[test]
fn shared_schema_later_reads_validation_and_rows() {
    let e = crate::compile("a.x").unwrap();
    let p = Program::compile(&e).unwrap();
    let e2 = crate::compile("b.y").unwrap();
    let p2 = Program::compile(&e2).unwrap();
    let mut a = Acquisition::new([&p]);
    let d = a.prepare(br#"{"a":{"x":1},"b":{"y":2}}"#).unwrap();
    let mut v = None;
    p2.try_for_each(&d, |x| {
        v = x.as_number();
        Ok::<_, ()>(())
    })
    .unwrap();
    assert_eq!(v, Some(2.));
    for input in [br#"{"a":01}"#.as_slice(), b"[1,]", b"true false", b"\xff"] {
        assert_eq!(
            a.prepare(input).unwrap_err(),
            crate::validate(input).unwrap_err()
        );
    }
    for source in [
        "$map(items,function($v){$v.a.x*2})",
        "items.{'x':a.x,'y':b.y}",
        "$sum($map(items,function($v){$v.a.x*2}))",
        "$sum((items[a.x>0]).(a.x*2))",
    ] {
        results(
            source,
            br#"{"items":[{"a":{"x":2},"b":{"y":3}},{"a":{"x":4},"b":{"y":5}}]}"#,
        );
    }
}

#[test]
fn owned_compilation_and_constant_branch_provenance() {
    let options = crate::CompileOptions::default().constant_binding(
        "vars",
        crate::OwnedValue::from_json(br#"{"a":{"k":"A"},"b":{"k":"B"}}"#).unwrap(),
    );
    let source = "($t:=flag ? $vars.a : $vars.b;$lookup($t,id))";
    let p = options.compile_program(source).unwrap();
    let expression = options.compile(source).unwrap();
    for flag in [true, false] {
        let input = format!(r#"{{"flag":{flag},"id":"k"}}"#);
        let mut a = Acquisition::new([&p]);
        let d = a.prepare(input.as_bytes()).unwrap();
        let mut actual = Vec::new();
        p.try_for_each(&d, |v| {
            actual.push(v.to_owned().unwrap());
            Ok::<_, ()>(())
        })
        .unwrap();
        assert_eq!(
            format!("{actual:?}"),
            format!(
                "{:?}",
                expression
                    .evaluate(input.as_bytes())
                    .unwrap()
                    .collect_owned()
                    .unwrap()
            )
        );
    }
    drop(options);
    drop(expression);
    let mut a = Acquisition::new([&p]);
    let d = a.prepare(br#"{"flag":true,"id":"k"}"#).unwrap();
    p.try_for_each(&d, |v| {
        let mut b = Vec::new();
        v.write_compact(&mut b).unwrap();
        assert_eq!(b, br#""A""#);
        Ok::<_, ()>(())
    })
    .unwrap();
}
#[test]
fn filter_and_mapper_errors_keep_their_order() {
    for source in [
        "$sum((items[a>0]).(b*2))",
        "(items[a>0]).(b*2)",
        "$sum(items[a>0].(b*2))",
        "items[a>0].(b*2)",
    ] {
        for input in [
            br#"{"items":[{"a":1,"b":null},{"a":null,"b":2}]}"#.as_slice(),
            br#"{"items":[{"a":1,"b":2},{"a":null,"b":null}]}"#,
        ] {
            results(source, input);
        }
    }
}

#[test]
fn borrowed_encoding_error_order_and_sequence_boundaries() {
    for source in [
        "[a+a,a]",
        "(a+a;a)",
        "a+(1/0+2)",
        "a+(b*2)",
        "$map(items,function($v,$i,$whole){[$v,$i,$count($whole)]})",
        "items.(v*2)",
    ] {
        for input in [
            br#"{"a":1e3,"b":null,"items":[{"v":2},{"v":3}]}"#.as_slice(),
            br#"{"a":null,"b":"bad","items":[]}"#,
            br#"{"a":2,"b":3,"items":[{},null]}"#,
        ] {
            results(source, input);
        }
    }
}

#[test]
fn cancellation_through_branches_bindings_and_nested_paths() {
    for source in [
        "flag ? items.(v*2) : ()",
        "($x:=2;flag ? items.(v*$x) : 0)",
        "($x:=items.(v*2);$x)",
        "items.(children.(v*2))",
    ] {
        let input =
            br#"{"flag":true,"items":[{"v":2,"children":[{"v":2},{"v":null}]},{"v":null}]}"#;
        let e = crate::compile(source).unwrap();
        let p = crate::CompileOptions::default()
            .compile_program(source)
            .unwrap();
        let mut a = Acquisition::new([&p]);
        let d = a.prepare(input).unwrap();
        let expected = e
            .evaluate(input)
            .map_err(crate::ConsumeError::Evaluation)
            .and_then(|v| v.try_for_each(|_| Err("stop")));
        assert_eq!(p.try_for_each(&d, |_| Err("stop")), expected, "{source}");
    }
}

#[test]
fn results_outlive_acquisition_and_compiled_programs_are_shareable() {
    fn shareable<T: Send + Sync>() {}
    shareable::<Program>();
    shareable::<Acquisition>();
    let p = crate::CompileOptions::default()
        .compile_program("items.{'v':v}")
        .unwrap();
    let input = br#"{"items":[{"v":"borrowed"}]}"#;
    let mut retained = None;
    {
        let mut a = Acquisition::new([&p]);
        let d = a.prepare(input).unwrap();
        p.try_for_each(&d, |v| {
            retained = Some(v);
            Ok::<_, ()>(())
        })
        .unwrap();
    }
    let mut bytes = Vec::new();
    retained.unwrap().write_compact(&mut bytes).unwrap();
    assert_eq!(bytes, br#"{"v":"borrowed"}"#);
}

#[test]
fn bound_reads_survive_program_moves_unions_and_unknown_scopes() {
    let options = crate::CompileOptions::default();
    let p = options
        .compile_program("{'x':a.x,'y':b.y,'r':a.x+b.y}")
        .unwrap();
    let q = options.compile_program("{'n':c.n,'x':a.x}").unwrap();
    let mut acquisition = Acquisition::new([&p, &q]);
    let p = Box::new(p);
    for input in [
        br#"{"a":{"x":2},"b":{"y":3},"c":{"n":4}}"#.as_slice(),
        br#"{"a":{"x":2},"a":{},"b":{"y":3},"c":{"n":4}}"#,
    ] {
        let doc = acquisition.prepare(input).unwrap();
        assert!(super::run::Input::Captured(&doc).reads(&p).is_some());
        for (program, source) in [
            (&*p, "{'x':a.x,'y':b.y,'r':a.x+b.y}"),
            (&q, "{'n':c.n,'x':a.x}"),
        ] {
            let mut actual = Vec::new();
            program
                .try_for_each(&doc, |v| {
                    actual.push(v.to_owned().unwrap());
                    Ok::<_, ()>(())
                })
                .unwrap();
            let expected = crate::compile(source)
                .unwrap()
                .evaluate(input)
                .unwrap()
                .collect_owned()
                .unwrap();
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        }
    }
    for source in [
        "($m:=flag?a:b;$m.x+$m.y)",
        "$map(items,function($v){$map($v.children,function($w){$v.a+$w.a})})",
    ] {
        results(source,br#"{"flag":true,"a":{"x":2,"y":3},"b":{"x":7,"y":8},"items":[{"a":2,"children":[{"a":3}]}]}"#);
    }
}
#[test]
fn scalar_allocation_preserves_values_and_reuses_dead_cells() {
    let p = crate::CompileOptions::default()
        .compile_program("metrics.a+metrics.b+metrics.c+metrics.d+metrics.e")
        .unwrap();
    assert!(p.scalar.is_some());
    let layout = &p.storage;
    assert!(layout.count <= 4);
    assert!(layout.count < p.registers);
    for source in [
        "(a+a;a)",
        "($a:=a;$b:=b;$a*$b+$a)",
        "a*a+b*b+a*b",
        "-a+(b*2)",
        "(a*2;b*2;a)",
    ] {
        for input in [
            br#"{"a":1e3,"b":2}"#.as_slice(),
            br#"{"a":null,"b":"bad"}"#,
            b"{}",
        ] {
            results(source, input);
        }
    }
}

#[test]
fn predicate_facts_ignore_grouping_bindings_and_boolean_spelling() {
    for source in [
        "items[(a>0)].{'x':a}",
        "items[($x:=a;$x>0)].{'x':a}",
        "items[$contains(s,'a')].{'x':a}",
        "items[($v:=s;$contains($v,'a'))].{'x':a}",
        "items[a>0 ? $contains(s,'a') : false].{'x':a}",
        "items['yes'].{'x':a}",
        "($ok:=true;items[$ok].{'x':a})",
        "($ok:=a=2;items[$ok].{'x':a})",
    ] {
        for input in [
            br#"{"items":[{"a":2,"s":"Ada"},{"a":-1,"s":"beta"}]}"#.as_slice(),
            br#"{"items":[{"a":2,"s":null},{"a":null}]}"#,
            br#"{"items":[]}"#,
        ] {
            results(source, input);
        }
    }
    for source in ["items[a]", "items[1]", "items[[1,2]]", "items[a>0?true:1]"] {
        results(source, br#"{"items":[{"a":0},{"a":1},{"a":2}]}"#);
    }
}

#[test]
fn predicate_stages_preserve_order_and_acquire_retained_rows() {
    for source in [
        "items[a>0][b>0].{'x':a+b,'s':$uppercase(s)}",
        "(items[a>0])[b>0].{'x':a+b,'s':$uppercase(s)}",
        "$map($filter(items,function($v){$v.a>0}),function($v){{'x':$v.a+$v.b,'s':$uppercase($v.s)}})",
        "$sum(items[a>0][b>0].(a*b))",
    ] {
        for input in [
            br#"{"items":[{"a":1,"b":2,"s":"Ada"},{"a":2,"b":3,"s":"Bee"}]}"#.as_slice(),
            br#"{"items":[{"a":1,"b":null,"s":"Ada"},{"a":null,"b":2}]}"#,
            br#"{"items":[{"a":1,"b":2,"s":"Ada"}]}"#,
            br#"{"items":[]}"#,
        ] {
            results(source, input);
        }
    }
}

#[test]
fn filtered_constructor_cancellation_matches_reference_boundaries() {
    for source in [
        "items[v>0].{'x':b*2}",
        "(items[v>0]).{'x':b*2}",
        "items[(v>0)].{'x':b*2}",
        "items[($v:=v;$v>0)].{'x':b*2}",
        "(items[($v:=v;$v>0)]).{'x':b*2}",
        "items[($v:=v;$v>0)][b>0].{'x':v*2}",
        "items[v>0].($y:=b;{'x':$y*2})",
        "items[v>0][b>0].{'x':v*2}",
        "items[v>0].(b*2)",
        "items[v>0]",
        "($x:=2;items[v>0])",
        "(items.{'x':b*2})",
        "(items[v>0])",
        "(items)[v>0]",
        "items[v>0][b>0]",
        "($x:=items[v>0];$x)",
        "flag ? items[v>0] : ()",
        "$filter(items,function($v){$v.v>0})",
    ] {
        let e = crate::compile(source).unwrap();
        let p = crate::CompileOptions::default()
            .compile_program(source)
            .unwrap();
        let mut acquisition = Acquisition::new([&p]);
        for input in [
            br#"{"flag":true,"items":[{"v":2,"b":2},{"v":null,"b":3}]}"#.as_slice(),
            br#"{"flag":true,"items":[{"v":2,"b":null},{"v":null,"b":3}]}"#,
            br#"{"flag":true,"items":[{"v":2,"b":null},{"v":3,"b":2}]}"#,
            br#"{"flag":true,"items":[{"v":2,"b":2},{"v":3,"b":null},{"v":null,"b":5}]}"#,
        ] {
            for stop in [Some(1), Some(2), None] {
                let mut expected_values = Vec::new();
                let expected = e
                    .evaluate(input)
                    .map_err(crate::ConsumeError::Evaluation)
                    .and_then(|v| {
                        v.try_for_each(|v| {
                            expected_values.push(v.to_owned().unwrap());
                            if Some(expected_values.len()) == stop {
                                Err("stop")
                            } else {
                                Ok(())
                            }
                        })
                    });
                let document = acquisition.prepare(input).unwrap();
                let mut actual_values = Vec::new();
                let actual = p.try_for_each(&document, |v| {
                    actual_values.push(v.to_owned().unwrap());
                    if Some(actual_values.len()) == stop {
                        Err("stop")
                    } else {
                        Ok(())
                    }
                });
                assert_eq!(actual, expected, "{source}, {stop:?}");
                assert_eq!(
                    format!("{actual_values:?}"),
                    format!("{expected_values:?}"),
                    "{source}, {stop:?}"
                );
            }
        }
    }
}

#[test]
fn contextual_ancestors_reuse_demanded_ports_without_record_indexes() {
    let sources = [
        "items.{'parent':%.name,'v':metrics.v0}",
        "groups.items.{'parent':%.tag,'root':%.%.name,'v':v}",
        "groups.items.($f:=function(){%.tag};$f())",
    ];
    let input = br#"{"name":"root","items":[{"metrics":{"v0":1}},{"metrics":{"v0":2}}],"groups":[{"tag":"g","items":[{"v":1},{"v":2}]}]}"#;
    for source in sources {
        results(source, input);
    }
    let p = crate::CompileOptions::default()
        .compile_program(sources[0])
        .unwrap();
    let mut a = Acquisition::new([&p]);
    let d = a.prepare(input).unwrap();
    assert!(d.schema.child(0, "name").is_some());
    let input = run::Input::Captured(&d);
    let root = input.root();
    let schema = d.schema;
    input
        .each_values(
            std::iter::once(Value::Raw(d.raw())),
            schema,
            |_| true,
            |child, focus, _| {
                assert!(matches!(focus, run::Cell::Capture(0)));
                assert!(std::ptr::eq(child, &input));
                assert_eq!(
                    format!("{:?}", root.value(child)),
                    format!("{:?}", focus.value(child))
                );
                Ok::<_, ()>(())
            },
        )
        .unwrap();
}

#[test]
fn captured_lexical_ancestors_preserve_values_across_general_calls() {
    for source in [
        "items.($p:=%;$f:=function(){$p.name};$f())",
        "items.($p:=%;$f:=function($q){$q.name};{\"parent\":$f($p)})",
        "items.($p:=%;$f:=function($q){$q};{\"parent\":$f($p).name})",
        "items.($p:=%;$f:=function(){$p};$f().name)",
        "items.($p:=%;$f:=function(){$p};$g:=function(){$f()};$g().name)",
    ] {
        results(source, br#"{"name":"root","items":[{"v":1},{"v":2}]}"#);
    }
}

#[test]
fn local_acquisition_survives_to_its_consumer() {
    fn owned(p: &Program) -> usize {
        let mut n = 0;
        for i in &p.code {
            n += usize::from(matches!(i.kind, Operation::Acquire(..)));
            i.kind.children(&mut |p| n += owned(p));
        }
        n
    }
    let scope = "(flag?a:b).{'x':x,'y':y}";
    let alias = "($m:=flag?a:b;{'x':$m.x,'y':$m.y})";
    let options = crate::CompileOptions::default();
    assert_eq!(
        owned(&options.compile_program(scope).unwrap()),
        0,
        "navigation exports values before owned acquisition can be consumed"
    );
    assert!(
        owned(&options.compile_program(alias).unwrap()) > 0,
        "an alias retains its acquired owner until field reads finish"
    );
    for source in [scope, alias] {
        results(
            source,
            br#"{"flag":true,"a":{"x":1,"y":2},"b":{"x":3,"y":4}}"#,
        );
        results(
            source,
            br#"{"flag":false,"a":{"x":1,"y":2},"b":{"x":3,"y":4}}"#,
        );
    }
}

#[test]
fn conditional_callees_join_demands_without_expanding_targets() {
    let source =
        "($f:=flag?function($d){$d.a.x+$d.b.y}:function($other){$other.a.x-$other.b.y};$f($))";
    let p = crate::CompileOptions::default()
        .compile_program(source)
        .unwrap();
    let mut a = Acquisition::new([&p]);
    for input in [
        br#"{"flag":true,"a":{"x":4},"b":{"y":2}}"#.as_slice(),
        br#"{"flag":false,"a":{"x":4},"b":{"y":2}}"#,
    ] {
        results(source, input);
        let d = a.prepare(input).unwrap();
        assert!(d.schema.child(0, "a").is_some());
        assert!(d.schema.child(0, "b").is_some());
    }
}

#[test]
fn chained_callee_joins_keep_bounded_demand_sets() {
    let options = crate::CompileOptions::default();
    for n in [1, 8, 48] {
        let source = format!(
            "($f:=function($d){{$d.a.x+$d.b.y}};{}$f($))",
            "$f:=flag?$f:function($v){$v.a.x+$v.b.y};".repeat(n)
        );
        let p = options.compile_program(&source).unwrap();
        let a = Acquisition::new([&p]);
        assert_eq!(
            a.retained_bytes(),
            6 * std::mem::size_of::<acquisition::Span>()
        );
        for flag in [true, false] {
            results(
                &source,
                format!("{{\"flag\":{flag},\"a\":{{\"x\":4}},\"b\":{{\"y\":2}}}}").as_bytes(),
            );
        }
    }
    let source = "($f:=flag?function($d){$d.a.x}:$string;$f($))";
    let p = options.compile_program(source).unwrap();
    let mut a = Acquisition::new([&p]);
    let d = a.prepare(br#"{"flag":false,"a":{"x":4}}"#).unwrap();
    assert!(
        d.schema.child(0, "a").is_none(),
        "unknown targets poison the joined proof"
    );
    results(source, br#"{"flag":false,"a":{"x":4}}"#);
}

#[test]
fn lexical_capture_demands_follow_values_and_resolved_returns() {
    for source in [
        "items.($p:=%;$f:=function(){$p.name};$f())",
        "items.($p:=%;$f:=function($q){$q.name};{\"parent\":$f($p)})",
        "items.($p:=%;$f:=function($q){$q};{\"parent\":$f($p).name})",
        "items.($p:=%;$f:=function(){$p};$f().name)",
        "items.($p:=%;$f:=function(){$p};$g:=$f;$g().name)",
        "items.($p:=%;$f:=function(){$p.name};$map([1,2],function($v){$f()}))",
    ] {
        let p = crate::CompileOptions::default()
            .compile_program(source)
            .unwrap();
        let mut a = Acquisition::new([&p]);
        let d = a
            .prepare(br#"{"name":"root","items":[{"name":"child"},{}]}"#)
            .unwrap();
        assert!(d.schema.child(0, "name").is_some(), "{source}");
        results(source, d.raw().as_bytes());
    }
    // Acquisition is a hint, never substitution of a definition-time capture.
    for source in [
        "($p:=a;$f:=function(){$p.x};$p:=b;$f())",
        "($p:=a;$f:=function(){$p.x};($p:=b;$f()))",
        "($p:=a;$f:=function(){$p};$p:=b;$f().x)",
        "($p:=a;$f:=function(){$p.x};$g:=function($p){$f()};$g(b))",
        "($p:=a;$f:=flag?function(){$p.x}:function(){$p.y};$p:=b;$f())",
    ] {
        for flag in [true, false] {
            results(
                source,
                format!(r#"{{"flag":{flag},"a":{{"x":1,"y":2}},"b":{{"x":3,"y":4}}}}"#).as_bytes(),
            );
        }
    }
}

#[test]
fn borrowed_navigation_metadata_survives_value_boundaries() {
    let options = crate::CompileOptions::default();
    let p = options.compile_program("{'x':x,'y':y,'n':x.n}").unwrap();
    let mut a = Acquisition::new([&p]);
    let bytes = br#"{"x":{"n":1},"y":[2],"extra":3}"#;
    let d = a.prepare(bytes).unwrap();
    let raw = run::Input::Raw(Some(d.raw()));
    let cell = raw.root().acquire(&raw, d.owner.unwrap().0);
    let value = cell.into_value(&raw).unwrap();
    assert!(matches!(value, Value::InputView(..)));
    assert_eq!(value.as_raw().unwrap().as_bytes().as_ptr(), bytes.as_ptr());
    assert_eq!(value.value_type(), crate::ValueType::Object);
    let x = value.field("x").unwrap();
    let x_raw = Value::Raw(d.raw()).field("x").unwrap();
    assert!(
        crate::compare::includes(
            crate::evaluate::Operand::One(x.clone()),
            crate::evaluate::Operand::One(Value::kept(vec![x_raw]))
        )
        .unwrap()
    );
    assert_eq!(x.field("n").unwrap().as_number(), Some(1.));
    assert_eq!(value.field("extra").unwrap().as_number(), Some(3.));
    assert!(value.field("absent").is_none());
    // Metadata owns only its spans and immutable schema, not the preparation
    // workspace or expression. Reusing either does not alter the borrowed value.
    a.prepare(br#"{"x":null,"y":0}"#).unwrap();
    let opts = crate::CompileOptions::default().binding("p");
    for source in [
        "$p.x.n+$p.y[0]",
        "($f:=function(){$p};$f().x.n)",
        "$map([1,2],function($v){$p.x.n})",
        "$eval('$p.x.n')",
        "(function($n)<n:n>{$n*2})($p.x.n)",
        "$p ~> |x|{'n':5}|",
    ] {
        let reference = opts
            .compile(source)
            .unwrap()
            .evaluate_with(
                Some(bytes),
                crate::EvaluationOptions {
                    bindings: vec![("p", Value::Raw(crate::validate(bytes).unwrap()))],
                    ..Default::default()
                },
            )
            .and_then(|v| v.collect_owned());
        let program = opts.compile_program(source).unwrap();
        let actual = program
            .evaluate_with(
                Some(bytes),
                crate::EvaluationOptions {
                    bindings: vec![("p", value.clone())],
                    ..Default::default()
                },
            )
            .and_then(|v| v.collect_owned());
        assert_eq!(format!("{actual:?}"), format!("{reference:?}"), "{source}");
    }
}

#[test]
fn streamed_boundaries_preserve_arrays_missing_errors_and_output_prefixes() {
    let rows = [
        "null",
        "[]",
        "[[]]",
        "[[],[]]",
        "[[1,2]]",
        "[[1,2],[3]]",
        "[{\"v\":[1,2]}]",
        "[{\"v\":2},{\"v\":3}]",
        "[{\"v\":null},{\"v\":2}]",
    ];
    for rows in rows {
        let input = format!(r#"{{"items":{rows}}}"#);
        for expr in [
            "(items[true]).($)",
            "(items[true]).(v)",
            "(items[true]).([v])",
            "(items[true]).(v*2)",
            "(items[true]).(flag?v:missing)",
        ] {
            for agg in ["sum", "count", "average", "min", "max"] {
                results(&format!("${agg}({expr})"), input.as_bytes());
            }
        }
    }
    for source in [
        "$sum((items[$number(p)>0]).(v*2))",
        "$sum(((items[true]).(v)).($number($)))",
    ] {
        for input in [
            br#"{"items":[{"p":1,"v":null},{"p":"bad","v":2}]}"#.as_slice(),
            br#"{"items":[{"p":1,"v":2},{"p":2,"v":"bad"}]}"#,
        ] {
            let p = crate::CompileOptions::default()
                .compile_program(source)
                .unwrap();
            let mut a = Acquisition::new([&p]);
            let d = a.prepare(input).unwrap();
            let mut bytes = b"prefix".to_vec();
            assert!(p.write_json(&d, &mut bytes).is_err());
            assert_eq!(bytes, b"prefix");
            results(source, input);
        }
    }
}

#[test]
fn composed_sequence_cancellation_keeps_eager_error_boundaries() {
    for source in [
        "(items[$number(p)>0]).(v*2)",
        "((items[true]).(v*2)).($)",
        "groups.items.($p:=%;$f:=function(){$p.name};$f())",
    ] {
        for input in [
            br#"{"items":[{"p":1,"v":2},{"p":"bad","v":3}]}"#.as_slice(),
            br#"{"items":[{"p":1,"v":2},{"p":2,"v":null}]}"#,
            br#"{"items":[{"p":1,"v":2},{"p":2,"v":3},{"p":"bad","v":4}]}"#,
            br#"{"groups":[{"name":"a","items":[1,2]},{"name":"b","items":[3]}]}"#,
        ] {
            let old = crate::compile(source).unwrap();
            let p = Program::compile(&old).unwrap();
            let mut a = Acquisition::new([&p]);
            let d = a.prepare(input).unwrap();
            let mut prefix = Vec::new();
            let expected = old
                .evaluate(input)
                .map_err(ConsumeError::Evaluation)
                .and_then(|e| {
                    e.try_for_each(|v| {
                        prefix.push(v.to_owned().unwrap());
                        Err("stop")
                    })
                });
            let expected_prefix = format!("{prefix:?}");
            for mode in 0..3 {
                prefix.clear();
                let e = match mode {
                    0 => p.evaluate_bytes(input),
                    1 => p.evaluate(&d),
                    _ => p.evaluate_prepared_with(&d, Default::default()),
                };
                let actual = e.map_err(ConsumeError::Evaluation).and_then(|e| {
                    e.try_for_each(|v| {
                        prefix.push(v.to_owned().unwrap());
                        Err("stop")
                    })
                });
                assert_eq!(actual, expected, "{source} mode {mode}");
                assert_eq!(
                    format!("{prefix:?}"),
                    expected_prefix,
                    "{source} mode {mode}"
                );
            }
        }
    }
}
