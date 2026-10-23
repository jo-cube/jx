use super::*;
fn compare(source: &str, input: &[u8]) {
    let expression = crate::compile(source).unwrap();
    let program = Program::compile(&expression)
        .unwrap_or_else(|e| panic!("{source}: {e}: {:?}", expression.root));
    let original = expression.evaluate(input).and_then(|v| v.collect_owned());
    let raw = program
        .evaluate_bytes(input)
        .and_then(|v| v.collect_owned());
    assert_eq!(format!("{original:?}"), format!("{raw:?}"), "raw {source}");
    let mut acquisition = Acquisition::new([&program]);
    let mut values = Vec::new();
    let actual = acquisition
        .prepare(input)
        .map_err(crate::ConsumeError::Evaluation)
        .and_then(|doc| {
            program.try_for_each(&doc, |v| {
                values.push(v.to_owned().unwrap());
                Ok::<_, ()>(())
            })
        });
    let actual = match actual {
        Ok(()) => Ok(values),
        Err(crate::ConsumeError::Evaluation(e)) => Err(e),
        _ => unreachable!(),
    };
    assert_eq!(
        format!("{original:?}"),
        format!("{actual:?}"),
        "acquired {source}"
    );
}
#[test]
fn projections_bindings_strings_and_errors() {
    let sources = [
        "{'x':a.x,'y':b,'z':a.x+a.x}",
        "($x:=a.x;$y:=$x+1;{'x':$x*$y,'s':$uppercase(s)})",
        "a.x>1 ? $number(b) : $length(s)",
        "a.x=2 or $number(b)>0",
        "a.x=0 and $number(b)>0",
        "{'v':$substring(s,1,2),'x':$contains(s,'a')}",
        "[a.x,b,[1,2]]",
    ];
    for source in sources {
        for input in [
            r#"{}"#,
            r#"{"a":{"x":2},"b":"3","s":"ada"}"#,
            r#"{"a":{"x":null},"b":false,"s":"é😀\ud800"}"#,
        ] {
            compare(source, input.as_bytes());
        }
    }
}
#[test]
fn callbacks_lexical_focus_folds_and_duplicates() {
    for source in [
        "$map(items,function($v){ {'x':$v.a.x,'s':$uppercase($v.s)} })",
        "($n:=2;$map(items,function($v){$v.a.x*$n}))",
        "$filter(items,function($v){$v.a.x>1})",
        "$sum($map(items,function($v){$v.a.x*2}))",
        "$sum((items[a.x>1]).(a.x*2))",
        "items.{'n':a.x}",
        "items.(a.x*2)",
    ] {
        for input in [
            r#"{"items":[]}"#,
            r#"{"items":[{"a":{"x":2},"s":"abc"}]}"#,
            r#"{"items":[{"a":{"x":2},"s":"abc"},{"a":{"x":3},"s":"def"}]}"#,
            r#"{"items":[{"a":{"x":2}},{"a":{"x":null}}]}"#,
        ] {
            compare(source, input.as_bytes());
        }
    }
    compare(
        "{'x':a.x,'s':s}",
        br#"{"a":{"x":1},"a":{},"s":"a","\u0073":"b"}"#,
    );
}
#[test]
fn scalar_distribution_and_fusion_error_order() {
    let values = [
        "null", "false", "0", "2", "-3.5", "1e308", "\"3\"", "\"x\"", "{}",
    ];
    for a in values {
        for b in values {
            let input = format!("{{\"a\":{a},\"b\":{b},\"s\":\"é😀\"}}");
            for source in [
                "a*a+b*b+a*b",
                "($x:=a;$y:=b;($x*$x+$y*$y+$x*$y))",
                "a>1 ? $number(b) : $length(s)",
                "a=2 or b=3",
                "a>0 and b>0",
                "{'a':$string(a),'b':$number(b)}",
            ] {
                compare(source, input.as_bytes());
            }
        }
    }
    for input in [
        r#"{"items":[{"v":"x"},{"v":null}]}"#,
        r#"{"items":[{"v":1},{"v":"x"}]}"#,
        r#"{"items":[{"v":[1,2]}]}"#,
    ] {
        for source in [
            "$sum($map(items,function($v){$v.v}))",
            "$sum($map(items,function($v){$number($v.v)}))",
        ] {
            compare(source, input.as_bytes());
        }
    }
    compare("($x:=2;($x:=3;$x)+$x)", b"{}");
    compare("($x:=2;a=1 ? ($x:=3;$x) : $x)", br#"{"a":1}"#);
}
#[test]
fn scalar_cse_preserves_dominance_focus_and_failures() {
    for source in [
        "{'a':$uppercase(s),'b':$uppercase(s),'c':$contains(s,'a') and $contains(s,'a')}",
        "a>1?$number(s):$number(s)",
        "{'a':a>1?$uppercase(s):s,'b':$uppercase(s)}",
        "$number(s)*$number(s)+$number(s)",
        "($v:=$number(s);$v*$v+$v)",
    ] {
        for input in [
            br#"{"a":2,"s":"3"}"#.as_slice(),
            br#"{"a":0,"s":null}"#,
            br#"{"a":0,"s":"x"}"#,
            b"{}",
        ] {
            compare(source, input);
        }
    }
    {
        let source = "a>0 ? $x:=2 : $x:=3";
        for input in [br#"{"a":1}"#.as_slice(), br#"{"a":0}"#] {
            compare(source, input);
        }
    }
}

#[test]
fn callbacks_retain_captured_wrapped_focus() {
    for input in [b"[]".as_slice(), br#"[{"a":1}]"#, br#"[{"a":1},{"a":2}]"#] {
        for source in [
            "$map($,function($v){{'n':$v.a}})",
            "$map($,function($v){$})",
            "$map($,function($v){$count($)})",
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn nested_native_sequences_survive_callback_returns() {
    for input in [
        br#"{"items":[{"children":[]},{"children":[]}]}"#.as_slice(),
        br#"{"items":[{"children":[{"a":1}]},{"children":[{"a":1}]}]}"#,
        br#"{"items":[{"children":[{"a":1},{"a":2}]},{"children":[{"a":1},{"a":2}]}]}"#,
    ] {
        for source in [
            "$map(items,function($v){$map($v.children,function($w){$w.a})})",
            "$map(items,function($v){{'values':$map($v.children,function($w){$w.a})}})",
            "$map(items,function($v){[$v.children]})",
        ] {
            compare(source, input);
        }
    }
}

#[test]
fn grouped_pipeline_cardinality_errors_and_shapes() {
    for source in [
        "$sum((items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))",
        "(items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0)",
        "(items[metrics.v0>0]).{'v':metrics.v0}",
        "(items[metrics.v0>0]).($)",
        "$count((items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))",
        "(items[metrics.v0>0]).(function($v){$v.metrics.v0}($))",
        "$sum($map($filter(items,function($v){$v.metrics.v0>0}),function($v){$v.metrics.v0*$v.metrics.v1+$v.metrics.v0}))",
    ] {
        for item in [
            "null",
            "false",
            "3",
            "[]",
            "[1]",
            "{}",
            "{\"metrics\":{\"v0\":3,\"v1\":4}}",
            "{\"metrics\":{\"v0\":3,\"v1\":null}}",
            "{\"metrics\":{\"v0\":null,\"v1\":2}}",
        ] {
            for rows in [
                item.to_owned(),
                format!("[{item}]"),
                format!("[[{item}]]"),
                format!("[{item},{item}]"),
                format!("[[{item}],{item}]"),
                format!("[{item},{{\"metrics\":{{\"v0\":1,\"v1\":null}}}}]"),
            ] {
                for input in [
                    format!("{{\"items\":{rows}}}"),
                    format!("[{{\"items\":{rows}}}]"),
                ] {
                    compare(source, input.as_bytes());
                }
            }
        }
    }
}

#[test]
fn sequence_boundaries_keep_empty_candidate_parent() {
    compare(
        "(items[true]).{'value':v,'parent':%.name}",
        br#"{"name":"root","items":[[],[{"v":2,"w":3}]]}"#,
    );
}
