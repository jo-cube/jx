use super::navigation::workload;
use serde_json::json;

pub(super) fn run(smoke: bool) {
    for width in [1_u64, 8, 128, 1024] {
        let rows = (0..width)
            .map(|i| json!({"a":i%7,"b":width-i,"detail":{"a":i%7,"b":width-i}}))
            .collect::<Vec<_>>();
        let input = json!({"rows":rows}).to_string();
        let values = (0..width)
            .map(|i| 2 * (i % 7) + width - i)
            .collect::<Vec<_>>();
        for (name, source, expected, budget) in [
            (
                "map",
                "$map(rows,function($r){$r.a+$r.b+$r.a})",
                json!(values),
                16,
            ),
            (
                "nested_map",
                "$map(rows,function($r){$r.detail.a+$r.detail.b+$r.detail.a})",
                json!(values),
                16,
            ),
            (
                "indexed",
                "$map(rows,function($r,$i){$r.a+$r.b+$i})",
                json!((0..width).map(|i| width + i % 7).collect::<Vec<_>>()),
                16,
            ),
            (
                "reduce",
                "$reduce(rows,function($acc,$r){$acc+$r.a+$r.b},0)",
                json!([(0..width).map(|i| i % 7 + width - i).sum::<u64>()]),
                4,
            ),
            (
                "filtered_sum",
                "$sum($map($filter(rows,function($r){$r.a>2 and $r.b>0}),function($r){$r.a*$r.b+$r.a}))",
                if width < 4 {
                    json!([])
                } else {
                    json!([(0..width)
                        .filter(|i| i % 7 > 2)
                        .map(|i| (i % 7) * (width - i + 1))
                        .sum::<u64>()])
                },
                0,
            ),
            (
                "object",
                r#"$map(rows,function($r){ {"sum":$r.a+$r.b,"square":($r.a+$r.b)*($r.a+$r.b),"valid":$r.a>0 and $r.b>0} })"#,
                json!((0..width).map(|i|json!({"sum":i%7+width-i,"square":(i%7+width-i).pow(2),"valid":i%7>0})).collect::<Vec<_>>()),
                width * 2 + 16,
            ),
            (
                "sort",
                "rows^(a*a+a+b*b+b).b",
                json!(({
                    let mut ids = (0..width).collect::<Vec<_>>();
                    ids.sort_by_key(|i| {
                        let a = i % 7;
                        let b = width - i;
                        a * a + a + b * b + b
                    });
                    ids.into_iter().map(|i| width - i).collect::<Vec<_>>()
                })),
                64,
            ),
            (
                "groups",
                r#"rows{a&"":$sum($.(b*b+b))}"#,
                json!([({
                    let mut groups = serde_json::Map::new();
                    for key in 0..7 {
                        let total = (0..width)
                            .filter(|i| i % 7 == key)
                            .map(|i| {
                                let b = width - i;
                                b * b + b
                            })
                            .sum::<u64>();
                        if total > 0 {
                            groups.insert(key.to_string(), json!(total));
                        }
                    }
                    groups
                })]),
                width * 4 + 64,
            ),
        ] {
            workload(
                &format!("regions/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
    }
    for size in [100, 500, 1024, 10 * 1024, 1024 * 1024] {
        let mut record = json!({"a":7,"b":11,"detail":{"a":7,"b":11},"padding":""});
        let base = record.to_string().len();
        record["padding"] = json!("x".repeat(size - base));
        let input = record.to_string();
        workload(
            "regions/sparse_callback",
            "function($r){$r.detail.a+$r.detail.b+$r.detail.a}($)",
            &input,
            json!([25]),
            3,
            smoke,
        );
        let source = format!(
            "{{{}}}",
            (0..8)
                .map(|i| format!("\"v{i}\":(a+b)*(a+b)+{i}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let expected = (0..8)
            .map(|i| (format!("v{i}"), json!(324 + i)))
            .collect::<serde_json::Map<_, _>>();
        workload(
            "regions/wide_members",
            &source,
            &input,
            json!([expected]),
            2,
            smoke,
        );
        workload(
            "regions/reusable_members",
            r#"{"x":(a+b)*(a+b),"y":(a+b)*(a+b)+1}"#,
            &input,
            json!([{"x":324,"y":325}]),
            2,
            smoke,
        );
    }
}
