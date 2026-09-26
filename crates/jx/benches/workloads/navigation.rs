use super::measure_allocations;
use serde_json::{Value, json};
use std::hint::black_box;

fn workload(name: &str, source: &str, input: &str, expected: Value, limit: u64, smoke: bool) {
    let expression = jx::compile(source).unwrap();
    let mut actual = Vec::new();
    expression
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|value| {
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            actual.push(serde_json::from_slice::<Value>(&bytes).unwrap());
        })
        .unwrap();
    assert_eq!(Value::Array(actual), expected, "{name}: {source}");
    measure_allocations(name, input.len(), smoke, Some(limit), || {
        expression
            .evaluate(black_box(input.as_bytes()))
            .unwrap()
            .for_each(|value| {
                black_box(value);
            })
            .unwrap();
    });
}

pub(super) fn run(smoke: bool) {
    let mut fallback = "missing".to_owned();
    for _ in 0..10 {
        fallback = format!("({fallback} ?? missing)");
    }
    // Nesting must not duplicate the compiled left subtree at every level.
    measure_allocations("compile/nested_fallback", 0, smoke, Some(512), || {
        black_box(jx::compile(black_box(&fallback)).unwrap());
    });
    measure_allocations("compile/navigation", 0, smoke, None, || {
        black_box(
            jx::compile(black_box(
                "orders[price>10]^(>price){kind:{\"prices\":price[],\"total\":$sum(price)}}",
            ))
            .unwrap(),
        );
    });
    for size in [100, 500, 1024, 10 * 1024, 64 * 1024, 1024 * 1024] {
        let base = r#"{"id":7,"orders":[{"id":1,"price":5},{"id":2,"price":20}],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, limit) in [
            ("keep", "orders[price>10].id[]", json!([[2]]), 4),
            ("wildcard", "$sum(orders.*)", json!([28]), 16),
            ("descendants", "$sum(**.price)", json!([25]), 32),
            ("ordering", "orders^(>price).id", json!([2, 1]), 8),
            (
                "grouping",
                "orders{\"prices\":price,\"total\":$sum(price)}",
                json!([{"prices":[5,20],"total":25}]),
                16,
            ),
            (
                "membership",
                "orders[price in [5,20]].id",
                json!([1, 2]),
                32,
            ),
            ("fallback", "missing ?? id", json!([7]), 0),
            (
                "mixed",
                "($rows:=orders[price>10]^(>price);$rows{\"ids\":id[],\"total\":$sum(price)})",
                json!([{"ids":[2],"total":20}]),
                32,
            ),
        ] {
            workload(
                &format!("navigation/{name}"),
                source,
                &input,
                expected,
                limit,
                smoke,
            );
        }
    }
    for width in [8, 128, 1024, 16384] {
        let input=json!({"rows":(0..width).map(|i|json!({"id":i,"price":width-i,"kind":if i%2==0{"a"}else{"b"}})).collect::<Vec<_>>()}).to_string();
        workload(
            "navigation/wide_sort",
            "rows^(price)[0].id",
            &input,
            json!([width - 1]),
            64,
            smoke,
        );
        workload(
            "navigation/wide_group",
            "rows{kind:$sum(price)}",
            &input,
            json!([{"a":(width/2)*(width/2+1),"b":(width/2)*(width/2)}]),
            80,
            smoke,
        );
        workload(
            "navigation/wide_descendants",
            "$sum(**.price)",
            &input,
            json!([width * (width + 1) / 2]),
            width * 4 + 64,
            smoke,
        );
        workload(
            "navigation/range",
            "$sum([1..$count(rows)])",
            &input,
            json!([width * (width + 1) / 2]),
            32,
            smoke,
        );
    }
    for width in [8, 128, 1024] {
        let rows = (0..width)
            .map(|i| json!({"key":format!("k{i}"),"value":i}))
            .collect::<Vec<_>>();
        let expected = (0..width)
            .map(|i| (format!("k{i}"), json!(i)))
            .collect::<serde_json::Map<_, _>>();
        workload(
            "navigation/distinct_groups",
            "rows{key:value}",
            &json!({"rows":rows}).to_string(),
            json!([expected]),
            32,
            smoke,
        );
    }
    for depth in [8, 32, 64] {
        let input = format!("{}{{\"id\":7}}{}", "[".repeat(depth), "]".repeat(depth));
        workload(
            "navigation/deep_descendants",
            "$sum(**.id)",
            &input,
            json!([7]),
            8,
            smoke,
        );
    }
}
