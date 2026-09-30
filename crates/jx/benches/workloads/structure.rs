use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    for (name, source) in [
        (
            "parent",
            "orders.items[price>%.limit].{\"order\":%.id,\"price\":price}",
        ),
        (
            "transform",
            "$ ~> |orders.items[price>5]|{\"price\":price*1.2},\"sku\"|",
        ),
    ] {
        measure_allocations(&format!("compile/{name}"), 0, smoke, None, || {
            black_box(jx::compile(black_box(source)).unwrap());
        });
    }
    for size in [150, 500, 1024, 10 * 1024, 64 * 1024, 1024 * 1024] {
        let base = r#"{"orders":[{"id":7,"limit":5,"items":[{"price":2,"sku":"a"},{"price":10,"sku":"b"}]}],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        let original: serde_json::Value = serde_json::from_str(&input).unwrap();
        for (name, source, expected, budget) in [
            (
                "parent/filter",
                "orders.items[price>%.limit].price",
                json!([10]),
                128,
            ),
            (
                "parent/nested",
                "orders.items.price.%.%.id",
                json!([7, 7]),
                128,
            ),
            (
                "parent/sort",
                "orders.items.price^(>%.price)",
                json!([10, 2]),
                128,
            ),
            (
                "parent/constructor",
                "orders.items.{\"order\":%.id,\"price\":price}",
                json!([{"order":7,"price":2},{"order":7,"price":10}]),
                128,
            ),
            (
                "parent/closure",
                "orders.items.( $p:=%; $f:=function(){ $p.id }; $f() )",
                json!([7, 7]),
                128,
            ),
            (
                "parent/group",
                "orders.items.%.id{\"total\":$sum($)}",
                json!([{"total":14}]),
                128,
            ),
            (
                "parent/tuple",
                "orders.items@$item.{\"order\":%.id,\"price\":$item.price}",
                json!([{"order":7,"price":2},{"order":7,"price":10}]),
                128,
            ),
            (
                "parent/focus_control",
                "orders@$o.$o.items[price>$o.limit].price",
                json!([10]),
                128,
            ),
            ("transform/clone", "$clone($)", json!([original]), 128),
        ] {
            workload(name, source, &input, expected, budget, smoke);
        }
        for (name, source, change) in [
            (
                "transform/update",
                "$ ~> |orders.items[price>5]|{\"price\":price+1}|",
                true,
            ),
            (
                "transform/no_match",
                "$ ~> |orders.items[price>100]|{}|",
                false,
            ),
        ] {
            let mut expected: serde_json::Value = serde_json::from_str(&input).unwrap();
            if change {
                expected["orders"][0]["items"][1]["price"] = json!(11);
            }
            workload(name, source, &input, json!([expected]), 256, smoke);
        }
        let source = "$ ~> |orders.items[price>5]|{\"price\":price+1}|";
        let expr = jx::compile(source).unwrap();
        let mut sink = Vec::with_capacity(input.len());
        measure_allocations(
            "transform/update_write",
            input.len(),
            smoke,
            Some(256),
            || {
                sink.clear();
                expr.evaluate(black_box(input.as_bytes()))
                    .unwrap()
                    .for_each(|v| v.write_compact(&mut sink).unwrap())
                    .unwrap();
                black_box(&sink);
            },
        );
    }
    for width in [1, 32, 512] {
        let rows: Vec<_> = (0..width)
            .map(|n| json!({"n":n,"items":[{"x":1},{"x":2}],"keep":{"label":"untouched"}}))
            .collect();
        let data = json!({"rows":rows});
        let input = data.to_string();
        for (name, source, expected) in [
            (
                "parent/wide_filter_sum",
                "$sum(rows.items[x>%.n].x)",
                json!([if width == 1 { 3 } else { 5 }]),
            ),
            ("parent/wide_sort", "rows.items.x^(>%.%.n)[0]", json!([1])),
        ] {
            workload(
                name,
                source,
                &input,
                expected,
                128 * width as u64 + 64,
                smoke,
            );
        }
        let mut expected = data.clone();
        for row in expected["rows"].as_array_mut().unwrap() {
            row["n"] = json!(row["n"].as_u64().unwrap() + 1);
        }
        workload(
            "transform/wide_update",
            "$ ~> |rows|{\"n\":n+1}|",
            &input,
            json!([expected]),
            128 * width as u64 + 128,
            smoke,
        );
        let expr = jx::compile("$ ~> |rows|{\"n\":n+1}|").unwrap();
        let mut sink = Vec::with_capacity(input.len());
        measure_allocations(
            "transform/wide_write",
            input.len(),
            smoke,
            Some(128 * width as u64 + 128),
            || {
                sink.clear();
                expr.evaluate(black_box(input.as_bytes()))
                    .unwrap()
                    .for_each(|v| v.write_compact(&mut sink).unwrap())
                    .unwrap();
                black_box(&sink);
            },
        );
    }
}
