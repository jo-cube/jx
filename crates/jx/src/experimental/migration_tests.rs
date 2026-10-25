use super::*;

fn compare(source: &str, input: &[u8]) {
    compare_with(&crate::CompileOptions::default(), source, input);
}
fn compare_with(options: &crate::CompileOptions, source: &str, input: &[u8]) {
    let old = options
        .compile(source)
        .unwrap_or_else(|e| panic!("{source}: {e:?}"));
    let p = options.compile_program(source).unwrap();
    let expected = old.evaluate(input).and_then(|v| v.collect_owned());
    let raw = p.evaluate_bytes(input).and_then(|v| v.collect_owned());
    assert_eq!(format!("{raw:?}"), format!("{expected:?}"), "{source}");
    let mut a = Acquisition::new([&p]);
    let doc = a.prepare(input).unwrap();
    let mut values = Vec::new();
    let actual = p.try_for_each(&doc, |v| {
        values.push(v.to_owned().unwrap());
        Ok::<_, ()>(())
    });
    match expected {
        Ok(expected) => {
            actual.unwrap();
            assert_eq!(format!("{values:?}"), format!("{expected:?}"), "{source}");
        }
        Err(e) => assert_eq!(actual, Err(crate::ConsumeError::Evaluation(e)), "{source}"),
    }
}

#[test]
fn joins_preserve_both_acquisition_demands() {
    let p = crate::CompileOptions::default()
        .compile_program("($m:=flag?a:b;{'x':$m.x,'y':$m.y})")
        .unwrap();
    let a = Acquisition::new([&p]);
    // Root, flag, and two candidate objects. Joined fields belong to a local scope.
    assert_eq!(
        a.demand_bytes(),
        4 * std::mem::size_of::<acquisition::Span>()
    );
    for input in [
        br#"{"flag":true,"a":{"x":1,"y":2},"b":{"x":3}}"#.as_slice(),
        br#"{"flag":false,"a":{},"b":{"y":4},"b":{"x":5}}"#,
        br#"{"a":null,"b":false}"#,
    ] {
        compare("($m:=flag?a:b;{'x':$m.x,'y':$m.y})", input);
    }
}

#[test]
fn nested_callbacks_retain_parent_focus_and_captures() {
    let input = br#"{"tag":"root","groups":[{"tag":"first","items":[{"x":1,"y":2},{"x":3,"y":4}]},{"tag":"second","items":[{"x":5}]}]}"#;
    for source in [
        "$map(groups,function($g){$map($g.items,function($v){{'parent':$g.tag,'root':tag,'x':$v.x,'y':$v.y}})})",
        "$map(groups,function($g){$map($g.items,function($v){($m:=$v;{'x':$m.x+$m.x,'parent':$g.tag,'root':$$.tag})})})",
        "$sum($map(groups,function($g){$sum($map($g.items,function($v){$v.x*2}))}))",
    ] {
        compare(source, input);
    }
}

#[test]
fn array_navigation_keeps_complete_path_cardinality() {
    for source in [
        "a.x",
        "a.x.y",
        "($v:=a;$v.x)",
        "($v:=a;$v.x.y)",
        "{'x':a.x,'y':a.x.y}",
        "$sum(a.x.y)",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"a":[{"x":[{"y":[1,2]}]}]}"#,
            br#"{"a":[{"x":[{"y":1},{"y":2}]},{"x":[{"y":[3]}]}]}"#,
            br#"{"a":[[],[{"x":[1,[2],3]}],{"x":[]}]}"#,
            br#"{"a":[{"x":[]},{"x":[[1,2]]}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn fallbacks_and_membership_keep_values_identity_and_errors() {
    for source in [
        "a ?: b",
        "a ?? b",
        "(a ?? b).x",
        "a in [a]",
        "a in [b]",
        "($x:={'v':a};$x in [$x])",
        "{'x':a ?? b,'y':a ?: b}",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"a":0,"b":2}"#,
            br#"{"a":null,"b":3}"#,
            br#"{"a":[],"b":[1]}"#,
            br#"{"a":{"x":1},"b":{"x":1}}"#,
        ] {
            compare(source, input);
        }
    }
    compare("a ?: $number(b)", br#"{"a":1,"b":false}"#);
    compare("a ?? $number(b)", br#"{"a":null,"b":false}"#);
}

#[test]
fn path_array_results_normalize_once_at_the_last_stage() {
    for source in [
        "items.(a)",
        "items.([a])",
        "items.(flag ? a : b)",
        "items.(a.x)",
        "$sum(items.(a))",
        "$count(items.(a))",
        "$map(items,function($v){$v.a})",
    ] {
        for input in [
            br#"{"items":[]}"#.as_slice(),
            br#"{"items":[{"a":[1,2]}]}"#,
            br#"{"items":[{"a":[]},{"a":[]}]}"#,
            br#"{"items":[{"a":[1,[2]]},{"a":[3]}]}"#,
            br#"{"items":[[{"a":2}],{"a":3}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn pure_collection_and_string_kernels_share_the_value_boundary() {
    for source in [
        "$append(a,b)",
        "$reverse(a)",
        "$distinct(a)",
        "$sort(a)",
        "$keys(a)",
        "$spread(a)",
        "$merge(a)",
        "$zip(a,b)",
        "$split(s,',')",
        "$replace(s,'a','x')",
        "$round(n,2)",
        "$pad(s,8,'_')",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"a":[3,1,1],"b":[4,5],"s":"a,b,a","n":1.235}"#,
            br#"{"a":[{"x":1},{"y":2}],"b":null,"s":false,"n":null}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn fresh_collection_results_are_not_common_subexpressions() {
    for source in [
        "($x:=$merge(items);$y:=$merge(items);$x in [$y])",
        "($x:=$merge(items);$x in [$x])",
        "($x:={'v':a};$y:={'v':a};$x in [$y])",
    ] {
        compare(source, br#"{"items":[{"a":1},{"b":2}],"a":1}"#);
    }
}

#[test]
fn eager_filter_boundaries_preserve_array_cardinality() {
    for source in [
        "items[true]",
        "items[true].($)",
        "(items[true]).($)",
        "items[true][true].($)",
        "(items[true]).($count($))",
        "(items[true]).(a)",
        "(items[true]).([a])",
        "(items[true][true]).($count($))",
    ] {
        for input in [
            br#"{"items":[[1,2]]}"#.as_slice(),
            br#"{"items":[[],[]]}"#,
            br#"{"items":[[{"a":1},{"a":2}],{"a":3}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn missing_filter_sources_preserve_predicate_errors() {
    for source in [
        "items[$number('bad')>0]",
        "items[$number('bad')>0].{'x':1}",
        "items[false][$number('bad')>0]",
        "items[true][$number('bad')>0]",
        "(items[true]).{'x':1}",
        "$sum(items[true].(2))",
        "$sum((items[true]).(2))",
        "$sum((items[$number('bad')>0]).(2))",
    ] {
        for input in [br#"{}"#.as_slice(), br#"{"items":[]}"#] {
            compare(source, input);
        }
    }
}

#[test]
fn array_result_prefixes_and_eager_errors_match_the_reference() {
    use crate::ConsumeError;
    for source in [
        "items.(a)",
        "items.(flag?a:$number(s))",
        "items[true].(a)",
        "(items[true]).(a)",
        "(items[true]).($number(s))",
        "items[flag=true].([a])",
        "a.x.y",
    ] {
        for input in [
            br#"{"items":[{"a":[1,2],"flag":true},{"s":"bad"}]}"#.as_slice(),
            br#"{"items":[{"a":[],"flag":true},{"a":[[1]],"flag":true}]}"#,
            br#"{"items":[[{"a":1}],{"a":3}]}"#,
            br#"{"a":[{"x":[{"y":[1,2]}]},{"x":[{"y":3}]}]}"#,
        ] {
            for stop in [0, 1, 2] {
                let e = crate::compile(source).unwrap();
                let p = crate::CompileOptions::default()
                    .compile_program(source)
                    .unwrap();
                let mut a = Acquisition::new([&p]);
                let doc = a.prepare(input).unwrap();
                let mut left = Vec::new();
                let mut right = Vec::new();
                let expected = e
                    .evaluate(input)
                    .map_err(ConsumeError::Evaluation)
                    .and_then(|v| {
                        v.try_for_each(|v| {
                            left.push(v.to_owned().unwrap());
                            if left.len() > stop {
                                Err("stop")
                            } else {
                                Ok(())
                            }
                        })
                    });
                let actual = p.try_for_each(&doc, |v| {
                    right.push(v.to_owned().unwrap());
                    if right.len() > stop {
                        Err("stop")
                    } else {
                        Ok(())
                    }
                });
                assert_eq!(actual, expected, "{source} stop {stop}");
                assert_eq!(
                    format!("{right:?}"),
                    format!("{left:?}"),
                    "{source} stop {stop}"
                );
            }
        }
    }
}

#[test]
fn selected_fragments_keep_gaps_stages_and_prefixes() {
    let input=br#"{"items":[{"v":1,"keep":true},{"v":2,"keep":false},{"v":3,"keep":true},{"v":4,"keep":false},{"v":5,"keep":true}]}"#;
    for source in [
        "(items[keep=true]).(v)",
        "(items[keep=true][v>1]).(v)",
        "(items[keep=true]).([v])",
        "($x:=$map(items,function($v){$v});($x[keep=true]).(v))",
        "([{'v':1},{'v':2},{'v':3}][v!=2]).(v)",
    ] {
        compare(source, input);
    }
}

#[test]
fn fragment_density_and_repeated_filters_preserve_results() {
    for length in [0, 1, 2, 63, 64, 65, 128, 257] {
        let input = format!(
            "{{\"items\":[{}]}}",
            (0..length)
                .map(|i| format!("{{\"v\":{i},\"yes\":{}}}", i % 3 != 1))
                .collect::<Vec<_>>()
                .join(", \n")
        );
        for source in [
            "(items[yes=true]).(v)",
            "(items[yes=true][v>2]).(v)",
            "(items[yes=true][v>2][v!=9]).([v])",
            "($x:=$map(items,function($v){$v});($x[yes=true][v>2]).(v))",
        ] {
            compare(source, input.as_bytes());
        }
    }
}

#[test]
fn fragment_selection_preserves_raw_and_constructed_identity() {
    let input = br#"{"items":[{"v":1},{"v":2},{"v":3}]}"#;
    for source in [
        "(items[v>1]).{'same':$ in $$.items}",
        "($rows:=$map(items,function($v){{'v':$v.v}});($rows[v>1]).{'same':$ in $rows})",
    ] {
        compare(source, input);
    }
    compare(
        "(items[v>0]).(v)",
        br#"{"items":[{"v":1,"v":-1},{"v":-1,"\u0076":2}]}"#,
    );
}

#[test]
fn infallible_dead_reads_disappear_but_checks_remain_ordered() {
    for source in [
        "($unused:=items.name;1)",
        "($unused:=items.(name);1)",
        "($unused:=$map(items,function($v){$v.name});1)",
        "($unused:=a.x;{'id':id})",
        "($unused:=a*2;1)",
        "($unused:=$number(a);1)",
    ] {
        for input in [
            br#"{"a":null,"items":[{"name":"a"},{"name":"b"}]}"#.as_slice(),
            br#"{"a":2,"id":"row"}"#,
            br#"{"a":1e999,"items":[{"name":1e999}]}"#,
        ] {
            compare(source, input);
        }
    }
    let p = crate::CompileOptions::default()
        .compile_program("($unused:=items.name;1)")
        .unwrap();
    assert_eq!(p.instruction_count(), 1);
    assert_eq!(p.register_count(), 1);
    let mut a = Acquisition::new([&p]);
    for input in [b"{\"ignored\":01}".as_slice(), b"[1,]"] {
        assert_eq!(
            a.prepare(input).unwrap_err(),
            crate::validate(input).unwrap_err()
        );
    }
}
#[test]
fn complete_folds_cross_only_single_use_eager_boundaries() {
    for source in [
        "$sum(items[($v:=v;$v>0)].(b*2))",
        "$sum((items[($v:=v;$v>0)]).(b*2))",
        "($x:=items.(b*2);$sum($x))",
        "($x:=items.(b*2);[$sum($x),$count($x)])",
    ] {
        for input in [
            br#"{"items":[{"v":2,"b":2},{"v":null,"b":3}]}"#.as_slice(),
            br#"{"items":[{"v":2,"b":null},{"v":null,"b":3}]}"#,
            br#"{"items":[{"v":2,"b":2},{"v":3,"b":4}]}"#,
            br#"{}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn root_rebinding_is_lexical_and_survives_callback_capture() {
    for source in [
        "($$:={'x':1};$$.x)",
        "($$:=other;$map(items,function($v){$$.x}))",
        "(($$:={'x':1};$$.x);$$.x)",
        "($$:=1;$)",
        "items.($$:=$;{'x':$$.x})",
    ] {
        compare(
            source,
            br#"{"x":2,"other":{"x":3},"items":[{"x":4},{"x":5}]}"#,
        );
    }
}

#[test]
fn caller_scope_writes_use_the_arena_and_positions_remain_explicit() {
    compare(
        "($x:=false;items[$x:=v>0];$x)",
        br#"{"items":[{"v":2},{"v":-1}]}"#,
    );
    compare(
        "($x:=0;items[$x:=$x+1];$x)",
        br#"{"items":[{"v":2},{"v":-1}]}"#,
    );
    compare("($x:=false;items[($x:=v>0)];$x)", br#"{"items":[{"v":2}]}"#);
}

#[test]
fn conditional_acquisition_scales_without_branch_products() {
    for depth in [1, 4, 12, 32, 64] {
        let mut source = String::from("($m:=$;");
        for _ in 0..depth {
            source.push_str("$m:=flag?$m.a:$m.b;");
        }
        source.push_str("$m.x)");
        let p = crate::CompileOptions::default()
            .compile_program(&source)
            .unwrap();
        let a = Acquisition::new([&p]);
        assert_eq!(
            a.demand_bytes(),
            4 * std::mem::size_of::<acquisition::Span>()
        );
        assert!(p.instruction_count() < depth * 8 + 10);
    }
    for source in [
        "($m:=flag?a:b;$n:=flag?$m.x:$m.y;{'x':$n.v,'same':$n in [$n]})",
        "($m:=flag?a:b;$map(items,function($v){{'x':$m.x,'y':$m.y,'item':$v}}))",
        "($m:=flag?{'x':a}:b;{'x':$m.x,'same':$m in [$m]})",
    ] {
        for input in [
            br#"{"flag":true,"a":{"x":{"v":2},"y":4},"b":{},"items":[1,2]}"#.as_slice(),
            br#"{"flag":false,"b":[{"x":1},{"y":2}],"items":[1]}"#,
            br#"{}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn complete_json_delivery_matches_retained_values_and_rolls_back_errors() {
    for source in [
        "items[($v:=v;$v>0)].{'v':v*2}",
        "(items[true]).(a)",
        "($x:=items.(a);$x)",
        "items.(flag?a:$number(s))",
        "items.([a])",
        "a.x.y",
        "flag ? items.(v*2) : ()",
        "((items[true])[0]).(a)",
        "((items[true])[-1]).(a)",
        "((items[true])[[0,-1]]).(a)",
        "$sum((items[true]).(a))",
        "$sum((items[true]).($number(s)))",
        "$sum((items[true]).($error('downstream')))",
        "$sum((items.($number(s))[true]).($error('downstream')))",
        "($x:=0;(items[true]).($x:=$x+1);$x)",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"items":[{"v":1,"a":[1,2]},{"v":2,"a":[3]}]}"#,
            br#"{"items":[{"v":1},{"v":null}]}"#,
            br#"{"items":[{"a":[1,2],"flag":true},{"s":"bad"}]}"#,
            br#"{"items":[[{"a":1}],{"a":3}],"a":[{"x":[{"y":[1,2]}]}]}"#,
        ] {
            let p = crate::CompileOptions::default()
                .compile_program(source)
                .unwrap();
            let mut a = Acquisition::new([&p]);
            let doc = a.prepare(input).unwrap();
            let expected = crate::compile(source)
                .unwrap()
                .evaluate(input)
                .and_then(|v| v.collect_owned());
            let mut output = b"prefix".to_vec();
            let result = p.write_json(&doc, &mut output);
            match expected {
                Ok(values) => {
                    assert_eq!(result.unwrap(), !values.is_empty(), "{source}");
                    let mut bytes = Vec::new();
                    if values.len() > 1 {
                        bytes.push(b'[');
                    }
                    for (i, v) in values.iter().enumerate() {
                        if i > 0 {
                            bytes.push(b',');
                        }
                        v.as_value().write_compact(&mut bytes).unwrap();
                    }
                    if values.len() > 1 {
                        bytes.push(b']');
                    }
                    assert_eq!(&output[6..], bytes, "{source}");
                }
                Err(error) => {
                    assert!(
                        matches!(result, Err(crate::ConsumeError::Evaluation(actual)) if actual == error),
                        "{source}"
                    );
                    assert_eq!(output, b"prefix");
                }
            }
        }
    }
}

#[test]
fn acquisition_clones_share_metadata_and_isolate_worker_spans() {
    let p = crate::CompileOptions::default()
        .compile_program("{'x':a.x,'y':b.y}")
        .unwrap();
    let mut a = Acquisition::new([&p]);
    let mut b = a.clone();
    let left = a.prepare(br#"{"a":{"x":1}}"#).unwrap();
    let right = b.prepare(br#"{"b":{"y":2}}"#).unwrap();
    let mut x = Vec::new();
    let mut y = Vec::new();
    p.write_json(&left, &mut x).unwrap();
    p.write_json(&right, &mut y).unwrap();
    assert_eq!(x, br#"{"x":1}"#);
    assert_eq!(y, br#"{"y":2}"#);
}

#[test]
fn alias_diamonds_have_bounded_delivery_and_acquisition_analysis() {
    for depth in [4, 16, 64] {
        let source = format!("($m:=$;{}$m.x)", "$m:=flag?$m:$m;".repeat(depth));
        let p = crate::CompileOptions::default()
            .compile_program(&source)
            .unwrap();
        let mut a = Acquisition::new([&p]);
        assert_eq!(
            a.demand_bytes(),
            3 * std::mem::size_of::<acquisition::Span>()
        );
        let doc = a.prepare(br#"{"flag":true,"x":7}"#).unwrap();
        let mut output = Vec::new();
        assert!(p.write_json(&doc, &mut output).unwrap());
        assert_eq!(output, b"7");
    }
}

#[test]
fn copy_joins_keep_condition_checks_and_container_identity() {
    for source in [
        "($m:=a;$m:=flag?$m:$m;{'x':$m.x,'same':$m in [a]})",
        "($m:={'x':1};$m:=flag?$m:$m;$m in [$m])",
        "($m:=a;$m:=$number(flag)?$m:$m;$m.x)",
        "($m:=flag?a:b;$m:=flag?$m.x:$m.y;$m.v)",
    ] {
        for input in [
            br#"{"flag":true,"a":{"x":{"v":1},"y":{"v":2}}}"#.as_slice(),
            br#"{"flag":false,"b":[{"x":1},{"y":2}]}"#,
            br#"{"flag":null}"#,
            br#"{"flag":"bad"}"#,
            br#"{}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn shared_fold_stages_keep_checks_cardinality_and_error_priority() {
    let atoms = ["1", "0", "-2", "null", "true", "\"2\"", "[1]", "1e999"];
    for aggregate in ["sum", "count", "average", "min", "max"] {
        for predicate in ["v>0", "($m:=$;$m.v>0)"] {
            let source = format!("${aggregate}((items[{predicate}]).(v*b+v))");
            for left in atoms {
                for right in atoms {
                    // Mapper error on an early row must not hide a later
                    // predicate failure, including different error offsets.
                    let input = format!(
                        "{{\"items\":[{{\"v\":1,\"b\":{left}}},{{\"v\":{right},\"b\":2}}]}}"
                    );
                    compare(&source, input.as_bytes());
                }
            }
            for input in [
                br#"{}"#.as_slice(),
                br#"{"items":[]}"#,
                br#"{"items":{"v":2,"b":3}}"#,
            ] {
                compare(&source, input);
            }
        }
    }
}

#[test]
fn atomic_lowering_preserves_observable_tokens_and_checked_operands() {
    for source in [
        "v",
        "{'raw':v,'n':v+v}",
        "[v,v*v]",
        "v>0",
        "v+v",
        "v*v+v",
        "-v",
    ] {
        for input in [
            br#"{"v":1e0}"#.as_slice(),
            br#"{"v":-0}"#,
            br#"{"v":1e999}"#,
            br#"{"v":"text"}"#,
            br#"{"v":null}"#,
            br#"{}"#,
        ] {
            compare(source, input);
            let options = crate::CompileOptions::default();
            let old = options
                .compile(source)
                .unwrap_or_else(|e| panic!("{source}: {e:?}"));
            let p = options.compile_program(source).unwrap();
            let mut a = Acquisition::new([&p]);
            let doc = a.prepare(input).unwrap();
            let mut actual = Vec::new();
            let mut expected = Vec::new();
            let x = p.write_json(&doc, &mut actual);
            let y = old
                .evaluate(input)
                .and_then(|v| v.for_each(|v| v.write_compact(&mut expected).unwrap()));
            if y.is_ok() {
                assert!(x.is_ok());
                assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
            }
        }
    }
}

#[test]
fn cross_stage_common_arithmetic_keeps_first_check_offsets() {
    for source in [
        "$sum((items[a+b>0]).((a+b)*2))",
        "$sum((items[($n:=a+b;$n>0)]).((a+b)*2))",
        "$sum((items[a/b>0]).((a/b)+a))",
        "$count((items[a+b>0]).((a+b)*2))",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"items":[]}"#,
            br#"{"items":[{"a":1,"b":2},{"a":3,"b":4}]}"#,
            br#"{"items":[{"a":1,"b":null},{"a":null,"b":4}]}"#,
            br#"{"items":[{"a":1,"b":0},{"a":1e999,"b":2}]}"#,
            br#"{"items":[{"a":1},{"a":"x","b":"y"}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn composed_fields_preserve_arrays_identity_and_lexical_focus() {
    for source in [
        "($a:=a;$b:=$a.x;$b.y)",
        "($a:=a.x;$b:=$a.y;{'v':$b,'same':$b in [$b]})",
        "($a:=a;{'v':$a.x.y,'base':$a})",
        "$map(items,function($v){($m:=$v.metrics;$m.v0+$m.v1)})",
        "$sum((items[($m:=metrics;$m.v0>0)]).(metrics.v0*metrics.v1+metrics.v0))",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"a":{"x":{"y":[1,2]}}}"#,
            br#"{"a":[{"x":[{"y":[1,2]}]},{"x":[{"y":[3]}]}]}"#,
            br#"{"a":[[],[{"x":[{"y":[]},[{"y":4}]]}]]}"#,
            br#"{"a":{"x":[{"y":1}]},"a":{"x":{"y":2}}}"#,
            br#"{"items":[{"metrics":{"v0":1,"v1":2}},{"metrics":[{"v0":2,"v1":3}]}]}"#,
            br#"{"items":[{"metrics":{"v0":1,"v1":null}},{"metrics":{"v0":null,"v1":3}}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn callback_input_demands_keep_indices_whole_values_and_nested_captures() {
    for source in [
        "$map(items,function($v){1})",
        "$map(items,function($v,$i){$i})",
        "$map(items,function($v,$i,$all){$all})",
        "$map(items,function($v){$map([1,2],function($n){$v.name})})",
        "$sum($map(items,function($v,$i){$i+1}))",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"items":[]}"#,
            br#"{"items":[{"name":"a"},{"name":"b"}]}"#,
            br#"{"items":{"name":"one"}}"#,
            br#"{"items":[1,[2],3]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn complete_sequence_consumers_preserve_cardinality_aliases_and_errors() {
    for aggregate in ["sum", "count", "average", "min", "max"] {
        for argument in [
            "items.metrics.v0",
            "items.(metrics.v0)",
            "(items[true][true]).(metrics.v0)",
            "($m:=items.metrics;$m.v0)",
        ] {
            let source = format!("${aggregate}({argument})");
            for input in [
                br#"{}"#.as_slice(),
                br#"{"items":[]}"#,
                br#"{"items":[{},{}]}"#,
                br#"{"items":[{"metrics":{"v0":3}}]}"#,
                br#"{"items":[{"metrics":{"v0":[]}}]}"#,
                br#"{"items":[{"metrics":{"v0":[1]}}]}"#,
                br#"{"items":[{"metrics":{"v0":[1,2]}},{"metrics":{"v0":[3]}}]}"#,
                br#"{"items":[{"metrics":{"v0":[[1,2]]}}]}"#,
                br#"{"items":[{"metrics":{"v0":null}},{"metrics":{"v0":"bad"}}]}"#,
                br#"{"items":[{"metrics":{"v0":1e999}},{"metrics":{"v0":-0}}]}"#,
            ] {
                compare(&source, input);
            }
        }
    }
    for source in [
        "($m:=items.metrics;[$sum($m.v0),$count($m)])",
        "$sum(items[true][true].(v>0 ? a : b))",
        "$sum(items[true][v>0].(a))",
        "$sum(items.(v>0 ? a : $number('bad')))",
        "[$count(items.(metrics.v0)),$sum(items.(v*2))]",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"items":[{"metrics":{"v0":2},"v":1,"a":null},{"v":null}]}"#,
            br#"{"items":[{"v":1,"a":null},{"v":0}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn primitive_results_keep_checked_order_and_value_consumers() {
    for source in [
        "(a*2)+(b/0)",
        "(a/0)+(b*2)",
        "(a-a)/(b-b)",
        "(a*2)=(b*2)",
        "($n:=$map(items,function($v){$v.a});$n*2)",
        "($n:=$map(items,function($v){$v.a});{'n':$n*2,'same':$n=$n})",
        "(a*2)<(b*2)",
        "$lookup($,a*2)",
        "{'a':a*2,'b':-(b+1),'c':a=b}",
        "$sum((items[v>0]).(v*b+v))",
        "$sum($map(items,function($v){-($v.a*2)+$v.b}))",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"a":2,"b":3,"items":[{"v":1,"a":2,"b":3}]}"#,
            br#"{"a":null,"b":1e999,"items":[{"v":1,"b":null},{"v":null}]}"#,
            br#"{"a":1e999,"b":null,"items":[{"v":1,"a":1e999,"b":1}]}"#,
            br#"{"a":[2],"b":-0,"items":[{"v":2,"a":[1],"b":3}]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn immutable_numeric_lookup_results_keep_types_and_callback_ownership() {
    let options = crate::CompileOptions::default().constant_binding("vars",
        crate::OwnedValue::from_json(br#"{"labels":{"n":3,"z":-0,"large":1000000000000,"s":"3","b":true,"nil":null,"a":[1],"o":{}}}"#).unwrap());
    for source in [
        "$lookup($vars.labels,id)*2",
        "($n:=$lookup($vars.labels,id);$n*2+$n)",
        "$sum($map(items,function($v){$lookup($vars.labels,$v.id)*2}))",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"id":"n","items":[{"id":"n"},{"id":"z"}]}"#,
            br#"{"id":"s","items":[{"id":"n"},{"id":"s"}]}"#,
            br#"{"id":"large","items":[{"id":"large"},{"id":"n"}]}"#,
            br#"{"id":"b","items":[{"id":"b"}]}"#,
            br#"{"id":"nil","items":[{"id":"nil"}]}"#,
            br#"{"id":"a","items":[{"id":"a"}]}"#,
            br#"{"id":"o","items":[{"id":"o"}]}"#,
            br#"{"id":"missing","items":[{},{}]}"#,
        ] {
            compare_with(&options, source, input);
        }
    }
}

#[test]
fn owned_results_survive_frame_reuse_and_capture_release() {
    let input = br#"{"items":[{"n":1,"s":"Ad\u0061","name":"Ada","metrics":{"x":[1,2]}},{"n":2,"s":"\u00e9","name":"B","metrics":{"x":3}},{"n":3,"s":"z","name":"C","metrics":{}}]}"#;
    for source in [
        "$map(items,function($v){($m:={'n':$v.n,'s':$uppercase($v.s)};{'one':$m,'two':$m,'same':$m in [$m]})})",
        "items.($m:=metrics;{'m':$m,'x':$m.x,'s':$uppercase(name)})",
    ] {
        let expected = crate::compile(source)
            .unwrap()
            .evaluate(input)
            .unwrap()
            .collect_owned()
            .unwrap();
        let program = crate::CompileOptions::default()
            .compile_program(source)
            .unwrap();
        let mut acquisition = Acquisition::new([&program]);
        let mut held = Vec::new();
        {
            let doc = acquisition.prepare(input).unwrap();
            program
                .try_for_each(&doc, |value| {
                    held.push(value);
                    Ok::<_, ()>(())
                })
                .unwrap();
        }
        drop(acquisition);
        let actual: Vec<_> = held.iter().map(|value| value.to_owned().unwrap()).collect();
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
    }
}

#[test]
fn absent_input_is_distinct_from_json_null() {
    for source in [
        "$",
        "$$",
        "$type($)",
        "$exists($$)",
        "$count($)",
        "$boolean($)",
        "$.x",
        "$ ?? 7",
        "$ ?: 7",
        "{'v':$,'exists':$exists($$)}",
        "$map([1,2],function($v){[$v,$$]})",
        "($x:=$;$exists($x))",
        "$number($)",
        "$number('bad')",
    ] {
        let options = crate::CompileOptions::default();
        let expression = options
            .compile(source)
            .unwrap_or_else(|e| panic!("{source}: {e:?}"));
        let program = options.compile_program(source).unwrap();
        let expected = expression
            .evaluate_with(None, crate::EvaluationOptions::default())
            .and_then(|e| e.collect_owned());
        let actual = program
            .evaluate_without_input()
            .and_then(|e| e.collect_owned());
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
    }
    let p = crate::CompileOptions::default()
        .compile_program("$type($)")
        .unwrap();
    assert!(
        p.evaluate_without_input()
            .unwrap()
            .single()
            .unwrap()
            .is_none()
    );
    assert_eq!(
        p.evaluate_bytes(b"null")
            .unwrap()
            .single()
            .unwrap()
            .unwrap()
            .as_str()
            .unwrap()
            .unwrap(),
        "null"
    );
}

#[test]
fn program_clones_keep_acquisition_bindings_and_independent_execution() {
    for source in [
        "{'x':a.x,'y':a.y,'sum':a.x+a.y}",
        "$sum(items[v>0].(v*2))",
        "($m:=flag?a:b;{'x':$m.x,'y':$m.y})",
        "$map(items,function($v){$map($v.children,function($c){$c.v+$v.v})})",
    ] {
        let p = crate::CompileOptions::default()
            .compile_program(source)
            .unwrap();
        let mut acquisition = Acquisition::new([&p]);
        let copy = p.clone();
        assert!(std::sync::Arc::ptr_eq(&p.key, &copy.key));
        drop(p);
        for input in [
            br#"{"a":{"x":2,"y":3},"flag":true,"items":[{"v":1,"children":[{"v":4}]},{"v":-2}]}"#
                .as_slice(),
            br#"{"a":{},"b":{"x":9},"flag":false,"items":[]}"#,
        ] {
            let doc = acquisition.prepare(input).unwrap();
            assert!(super::run::Input::Captured(&doc).reads(&copy).is_some());
            let actual = copy.evaluate(&doc).unwrap().collect_owned().unwrap();
            let expected = crate::compile(source)
                .unwrap()
                .evaluate(input)
                .unwrap()
                .collect_owned()
                .unwrap();
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"), "{source}");
        }
    }
}

#[test]
fn validated_acquisition_matches_validation_and_replaces_previous_spans() {
    let options = crate::CompileOptions::default();
    let programs = [
        options.compile_program("{'x':a.x,'y':a.y}").unwrap(),
        options.compile_program("$sum(items.(a.x))").unwrap(),
    ];
    let mut a = Acquisition::new(&programs);
    for input in [
        br#" {"a":{"x":2,"y":3},"items":[{"a":{"x":4}}]} "#.as_slice(),
        br#"{"a":{"x":2,"y":3},"\\u0061":{},"items":[{"a":{"x":[1,2]}}]}"#,
        br#"{}"#,
        br#"[{"a":{"x":5}},{"a":{"y":6}}]"#,
        br#"null"#,
    ] {
        let raw = crate::validate(input).unwrap();
        let validated = a.prepare_validated(raw);
        assert_eq!(validated.raw().as_bytes(), raw.as_bytes());
        let expected = programs
            .iter()
            .map(|p| p.evaluate(&validated).and_then(|e| e.collect_owned()))
            .collect::<Vec<_>>();
        let checked = a.prepare(input).unwrap();
        for (p, expected) in programs.iter().zip(expected) {
            let actual = p.evaluate(&checked).and_then(|e| e.collect_owned());
            assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        }
    }
}

#[test]
fn matcher_values_compose_with_compiled_callbacks_and_closures() {
    for source in [
        "$match(text, /([a-z]+)([0-9]+)/)",
        "$replace(text, /([a-z]+)([0-9]+)/, function($m){$uppercase($m.groups[0])})",
        "($p:=/[0-9]+/;$split(text,$p))",
        "($f:=function($p){function($s){$match($s,$p)}};$f(/[0-9]+/)(text))",
        "($p:=/[0-9]+/;$m:=$p(text);[$m.match,$m.next().match])",
        "$contains(text,/[0-9]+/)",
    ] {
        for input in [
            br#"{"text":"a12 b34"}"#.as_slice(),
            br#"{"text":"none"}"#,
            br#"{}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn formatting_uses_compiled_arguments_and_shared_picture_validation() {
    for source in [
        "$formatNumber(n,'#,##0.00')",
        "$formatNumber(n,picture)",
        "$formatInteger(n,'000')",
        "$parseInteger(text,'000')",
        "$fromMillis(n,'[Y0001]-[M01]-[D01]')",
        "$toMillis(text)",
        "$formatBase(n,16)",
        "($f:=$formatNumber;$f(n,'0.0'))",
        "$map([n,n+1],function($v){$formatNumber($v,'0.00')})",
        "$formatNumber(n,'0.0.0')",
    ] {
        for input in [
            br#"{"n":1234,"text":"123","picture":"0.0"}"#.as_slice(),
            br#"{}"#,
            br#"{"n":null}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn seeded_effects_are_honored_by_all_input_apis() {
    for source in [
        "[$random(),$random()]",
        "$shuffle(items)",
        "($f:=$shuffle;$f(items))",
    ] {
        let options = crate::CompileOptions::default();
        let old = options
            .compile(source)
            .unwrap_or_else(|e| panic!("{source}: {e:?}"));
        let p = options.compile_program(source).unwrap();
        let input = br#"{"items":[1,2,3,4]}"#;
        let settings = || crate::EvaluationOptions {
            random: Some(crate::Random::seeded(42)),
            ..Default::default()
        };
        let expected = old
            .evaluate_with(Some(input), settings())
            .unwrap()
            .collect_owned()
            .unwrap();
        let mut acquisition = Acquisition::new([&p]);
        let document = acquisition.prepare(input).unwrap();
        for actual in [
            p.evaluate_with(Some(input), settings()),
            p.evaluate_validated_with(Some(crate::validate(input).unwrap()), settings()),
            p.evaluate_prepared_with(&document, settings()),
        ] {
            assert_eq!(
                format!("{:?}", actual.unwrap().collect_owned().unwrap()),
                format!("{expected:?}"),
                "{source}"
            );
        }
    }
}

#[test]
fn dynamic_programs_retain_closures_scopes_focus_and_owned_definitions() {
    for source in [
        "$eval(source)",
        "$eval(source,items)",
        "($x:=n;$eval('$x+1'))",
        "($x:=n;$eval('$x:=4');$x)",
        "($make:=$eval('function($x){function($y){$x+$y+n}}');$make(2)(3))",
        "($f:=$eval('function($x){$x=0?0:$f($x-1)+1}');$f(6))",
        "($f:=$eval('function($x){$x+n}');$eval('$f(3)'))",
        "($m:=$eval('/[0-9]+/');$m(text).match)",
        "$eval('$error(\"inner\")')",
        "$eval('1+')",
        "($f:=$eval;$f(source))",
    ] {
        for input in [
            br#"{"n":2,"source":"n+1","items":[1,2],"text":"a12"}"#.as_slice(),
            br#"{}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn dynamic_calls_invalidate_caller_ports_and_nested_region_summaries() {
    for source in [
        "($x:=n;$f:=$eval;$f('$x:=4');$x)",
        "($x:=n;$f:=flag?$eval:$eval;$f('$x:=4');$x)",
        "($x:=n;$f:=$eval;{'a':$x,'b':$f('$x:=4'),'c':$x};$x)",
        "($x:=n;$f:=$eval;items[$f('$x:=4')];$x)",
        "($x:=n;$f:=$eval;'$x:=4' ~> $f;$x)",
        "($x:=n;$f:=$eval;$g:=$f(?);$g('$x:=4');$x)",
    ] {
        compare(source, br#"{"n":2,"flag":true,"items":[1,2]}"#);
    }
}

#[test]
fn transforms_use_compiled_bodies_and_preserve_copy_identity() {
    for source in [
        "$ ~> |items|{'v':v+1}|",
        "$ ~> |items|{'v':v+1},'remove'|",
        "($delta:=2;$t:=|items|{'v':v+$delta}|;$delta:=3;$t($))",
        "($t:=|items|{'v':v+1}|;[$t($),$t($)])",
        "($x:=n;$t:=|items|{'v':($x:=$x+1)}|;$t($);$x)",
        "($make:=function($d){|items|{'v':v+$d}|};$make(2)($))",
        "($t:=$eval(\"|items|{'v':v+1}|\");$t($))",
        "($t:=|items|{'v':v+1}|;$eval('$t($)'))",
        "($t:=|items|{'v':v}|;$eval('$t') in [$t])",
        "$ ~> |items|5|",
        "$ ~> |items|{'v':v},42|",
        "($a:=$clone(items);$b:=$clone(items);$a in [$b])",
    ] {
        for input in [
            br#"{"n":2,"items":[{"v":1,"remove":true},{"v":2}]}"#.as_slice(),
            br#"{}"#,
            br#"{"items":null}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn callback_kernels_propagate_possible_caller_writes() {
    for source in [
        "($x:=n;$map(['$x:=4'],$eval);$x)",
        "($x:=n;{'a':$x,'b':$map(['$x:=4'],$eval),'c':$x};$x)",
        "($x:=n;$f:=$eval;$map(['$x:=4'],$f);$x)",
        "($x:=n;$map(['$x:=4'],function($s){$eval($s)});$x)",
    ] {
        compare(source, br#"{"n":2}"#);
    }
}

#[test]
fn known_functions_propagate_writes_through_captured_calls() {
    for source in [
        "($x:=0;$t:=|$eval('$x:=items')|{'v':v+1}|;$f:=function($d){$t($d)};$f($);$x)",
        "($x:=0;$t:=|$eval('$x:=items')|{'v':v+1}|;$f:=function($d){$t($d)};$g:=function($d){$f($d)};$g($);$x)",
        "($x:=0;$t:=|$eval('$x:=items')|{'v':v+1}|;$f:=function($d){$t($d)};$map([$],$f);$x)",
    ] {
        for input in [br#"{"items":[{"v":1}]}"#.as_slice(), br#"{}"#] {
            compare(source, input);
        }
    }
}

#[test]
fn correlated_rows_cross_sequence_and_lexical_boundaries() {
    let input =
        br#"{"groups":[{"name":"a","items":[{"v":1},{"v":3}]},{"name":"b","items":[{"v":2}]}]}"#;
    for query in [
        "groups#$g.items#$i.{'g':$g,'i':$i,'parent':%.name,'v':v}",
        "groups@$g.items@$i[$i.v>1].{'name':$g.name,'v':$i.v}",
        "groups.items[%.name='a'].v",
        "(groups.items)[%.name='a'].v",
        "groups.items@$it^($it.v).{'v':$it.v}",
        "groups#$g.items{'key':[$g,v]}",
        "groups.items.($p:=%;function(){$p.name}) ~> $map(function($f){$f()})",
        "groups.items.( $p := %; {'name':$p.name,'v':v})",
    ] {
        compare(query, input);
    }
}

#[test]
fn folds_accept_general_sequence_shapes() {
    for query in [
        "$sum(items.(a))",
        "$sum(items[true].(2))",
        "$sum((items[true]).(2))",
        "$count(items.(a))",
        "$sum((items[true][true]).(a))",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"items":[[]]}"#,
            br#"{"items":[[],[]]}"#,
            br#"{"items":[{"a":[1]}]}"#,
            br#"{"items":[{"a":[1,2]},{"a":[3]}]}"#,
            br#"{"items":[[{"a":1},{"a":2}],{"a":3}]}"#,
        ] {
            compare(query, input);
        }
    }
}

#[test]
fn route_source_context_survives_root_arrays_and_nested_focus_changes() {
    for source in [
        "$sum((items[true]).(2))",
        "$sum(items[true].(2))",
        "(items[true]).(2)",
        "((items[true].(2))).($)",
        "$map($,function($x){(items[true]).(2)})",
        "{'v':(items[true]).(2)}",
    ] {
        for input in [
            br#"[1,2]"#.as_slice(),
            br#"[]"#,
            br#"[[1,2],3]"#,
            br#"[{"items":[1,2]},{"items":[]},{}]"#,
            br#"{"items":[[]]}"#,
            br#"{"items":[[],[]]}"#,
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn native_candidate_positions_become_missing_when_read_by_callbacks() {
    for source in [
        "$map(items[[0,-1]],function($v){$v})",
        "$map(items[[0,-1]],function($v){[$v]})",
        "$map(items[[0,-1]],function($v){{'v':$v}})",
        "items[[0,-1]].($)",
        "(items[[0,-1]]).([$])",
    ] {
        for input in [
            br#"{}"#.as_slice(),
            br#"{"items":[]}"#,
            br#"{"items":[1,2]}"#,
            br#"{"items":[[1,2],[]]}"#,
        ] {
            compare(source, input);
        }
    }
}
