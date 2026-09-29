use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    measure_allocations("compile/scoped_path", 0, smoke, None, || {
        black_box(
            jx::compile(black_box(
                "rows@$r.bands[$r.kind=kind]{kind:$sum($r.price)}",
            ))
            .unwrap(),
        );
    });
    for size in [100, 500, 1024, 10 * 1024, 1024 * 1024] {
        let base = r#"{"rows":[{"id":1,"price":5},{"id":2,"price":20}],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, budget) in [
            ("index", "rows#$i.$i", json!([0, 1]), 24),
            (
                "filter_sum",
                "$sum(rows#$i[price>10].(price+$i))",
                json!([21]),
                32,
            ),
            (
                "plan_leaves",
                "rows#$i.{\"index\":$i,\"amount\":price*price+id}",
                json!([{"index":0,"amount":26},{"index":1,"amount":402}]),
                40,
            ),
            ("kept", "rows#$i[price>10].$i[]", json!([[1]]), 32),
            (
                "ordered",
                "rows#$i^(>price).{\"index\":$i,\"price\":price}",
                json!([{"index":1,"price":20},{"index":0,"price":5}]),
                64,
            ),
        ] {
            workload(
                &format!("tuple/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
    }
    for width in [8_u64, 128, 1024] {
        let rows: Vec<_> = (0..width)
            .map(|i| json!({"id":i,"price":i,"kind":if i%2==0{"x"}else{"y"},"children":[i,i+1]}))
            .collect();
        let input =
            json!({"rows":rows,"bands":[{"kind":"x","rate":2},{"kind":"y","rate":3}]}).to_string();
        workload(
            "tuple/wide_sum",
            "$sum(rows#$i[$i%2=0].(price+$i))",
            &input,
            json!([width * (width / 2 - 1)]),
            width * 12 + 32,
            smoke,
        );
        workload(
            "tuple/nested_sum",
            "$sum(rows#$i.children#$j.($+$i+$j))",
            &input,
            json!([2 * width * width]),
            width * 28 + 32,
            smoke,
        );
        let total: u64 = (0..width).map(|i| i * if i % 2 == 0 { 2 } else { 3 }).sum();
        workload(
            "tuple/join_sum",
            "$sum(rows@$r.bands[kind=$r.kind].(rate*$r.price))",
            &input,
            json!([total]),
            width * 32 + 32,
            smoke,
        );
        workload(
            "tuple/join_bound",
            "($bands:=bands; $sum(rows@$r.$bands[kind=$r.kind].(rate*$r.price)))",
            &input,
            json!([total]),
            width * 32 + 64,
            smoke,
        );
        workload(
            "tuple/group",
            "rows#$i{kind:{\"indices\":$i,\"total\":$sum(price)}}",
            &input,
            json!([{"x":{"indices":(0..width).step_by(2).collect::<Vec<_>>(),"total":(width/2)*(width/2-1)},"y":{"indices":(1..width).step_by(2).collect::<Vec<_>>(),"total":width*width/4}}]),
            width * 16 + 64,
            smoke,
        );
        workload(
            "tuple/negative_index",
            "rows#$i[-1].$i",
            &input,
            json!([width - 1]),
            width * 8 + 64,
            smoke,
        );
    }
}
