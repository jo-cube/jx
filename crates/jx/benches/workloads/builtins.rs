use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    measure_allocations("compile/builtins", 0, smoke, None, || {
        black_box(
            jx::compile(black_box(
                "$join($map(rows[price>10],function($r){$uppercase($trim($r.name))}),',')",
            ))
            .unwrap(),
        );
    });
    for size in [100, 500, 1024, 10 * 1024, 1024 * 1024] {
        let base = r#"{"text":"  Hello  World  ","n":-3.5,"a":[1,2,3],"object":{"x":1,"y":2},"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, budget) in [
            ("length", "$length(text)", json!([16]), 0),
            (
                "normalize",
                "$uppercase($trim(text))",
                json!(["HELLO WORLD"]),
                32,
            ),
            (
                "split_join",
                "$join($split(text,' '),'-')",
                json!(["--Hello--World--"]),
                80,
            ),
            ("numeric", "$floor($abs(n))", json!([3]), 0),
            ("average", "$average(a)", json!([2]), 0),
            ("type", "$type(object)", json!(["object"]), 0),
            ("keys", "$keys(object)", json!(["x", "y"]), 24),
            ("map_builtin", "$map(a,$abs)", json!([1, 2, 3]), 8),
            (
                "map_closure",
                "($n:=n;$map(a,function($v){$v*$n}))",
                json!([-3.5, -7, -10.5]),
                32,
            ),
            (
                "reduce",
                "$reduce(a,function($a,$b){$a+$b},0)",
                json!([6]),
                32,
            ),
            (
                "sift",
                "$sift(object,function($v){$v>1})",
                json!([{"y":2}]),
                32,
            ),
        ] {
            workload(
                &format!("builtin/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
    }
    for width in [8_u64, 128, 1024] {
        let data = json!({"factor":3,"rows":(0..width).map(|i|json!({"n":i,"name":format!(" p{i} "),"kind":if i%2==0{"a"}else{"b"}})).collect::<Vec<_>>()});
        let input = data.to_string();
        let total = (0..width).filter(|i| i % 2 == 0).sum::<u64>();
        workload(
            "builtin/filter_reduce",
            "$reduce($filter(rows,function($r){$r.n%2=0}),function($a,$r){$a+$r.n},0)",
            &input,
            json!([total]),
            width * 8 + 32,
            smoke,
        );
        workload(
            "builtin/planned_average",
            "$average(rows[n>=0].(n*3+1))",
            &input,
            json!([3.0 * (width - 1) as f64 / 2.0 + 1.0]),
            0,
            smoke,
        );
        workload(
            "builtin/grouped_average",
            "rows{kind:$average(n)}",
            &input,
            json!([{"a":width/2-1,"b":width/2}]),
            128,
            smoke,
        );
        let labels = (0..width)
            .rev()
            .map(|i| format!("P{i}"))
            .collect::<Vec<_>>()
            .join(",");
        workload(
            "builtin/ordered_labels",
            "$join($map(rows^(>n),function($r){$uppercase($trim($r.name))}),',')",
            &input,
            json!([labels]),
            width * 32 + 64,
            smoke,
        );
        workload(
            "builtin/merged_objects",
            "$merge($map(rows,function($r){{$trim($r.name):$r.n}}))",
            &input,
            json!([(0..width)
                .map(|i| (format!("p{i}"), json!(i)))
                .collect::<serde_json::Map<_, _>>()]),
            width * 24 + 64,
            smoke,
        );
        workload(
            "builtin/each_group",
            "$each(rows{kind:$sum(n)},function($v,$k){{'key':$k,'total':$v}})",
            &input,
            json!([{"key":"a","total":total},{"key":"b","total":width*width/4}]),
            128,
            smoke,
        );
        workload(
            "builtin/distinct",
            "$count($distinct(rows.kind))",
            &input,
            json!([2]),
            64,
            smoke,
        );
    }
}
