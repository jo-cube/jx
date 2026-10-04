use super::{measure, measure_allocations};
use std::hint::black_box;

fn workload(name: &str, source: &str, input: &str, expected: Option<f64>, smoke: bool) {
    let expression = jx::compile(source).unwrap();
    let mut actual = None;
    expression
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|value| {
            assert!(actual.is_none(), "{name}: multiple results");
            let jx::Value::Number(number) = value else {
                panic!("{name}: {value:?}")
            };
            actual = Some(number);
        })
        .unwrap();
    assert_eq!(actual, expected, "{name}: {source}");
    measure(name, input.len(), smoke, || {
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
    measure_allocations("compile/aggregate", 0, smoke, None, || {
        black_box(jx::compile(black_box("$sum(orders[price > 10].price)")).unwrap());
    });
    for size in [100, 500, 1024, 10 * 1024, 64 * 1024, 1024 * 1024] {
        let base = r#"{"orders":[{"price":5},{"price":20}],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        assert_eq!(input.len(), size);
        for (name, source, expected) in [
            ("count", "$count(orders.price)", Some(2.0)),
            ("sum", "$sum(orders.price)", Some(25.0)),
            ("min", "$min(orders.price)", Some(5.0)),
            ("max", "$max(orders.price)", Some(20.0)),
            (
                "filtered_count",
                "$count(orders[price > 10].price)",
                Some(1.0),
            ),
            ("filtered_sum", "$sum(orders[price > 10].price)", Some(20.0)),
            ("empty_sum", "$sum(orders[price < 0].price)", None),
        ] {
            workload(
                &format!("aggregate/{name}"),
                source,
                &input,
                expected,
                smoke,
            );
        }
    }
    for width in [8, 128, 1024, 16384] {
        let values = (0..width)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let raw = format!(r#"{{"a":[{values}]}}"#);
        let total = (width * (width - 1) / 2) as f64;
        for (name, source, expected) in [
            ("array_count", "$count(a)", width as f64),
            ("array_sum", "$sum(a)", total),
            ("array_min", "$min(a)", 0.0),
            ("array_max", "$max(a)", (width - 1) as f64),
            ("sequence_sum", "$sum(a[true])", total),
            ("computed_sum", "$sum(a.($ * 2))", total * 2.0),
            (
                "wide_filtered_sum",
                "$sum(a[$ % 2 = 0])",
                ((width / 2) * (width / 2 - 1)) as f64,
            ),
        ] {
            workload(
                &format!("aggregate/{name}"),
                source,
                &raw,
                Some(expected),
                smoke,
            );
        }
    }
    for width in [16, 256, 16384] {
        let input = format!(
            r#"{{"groups":[{}]}}"#,
            vec![r#"[{"orders":[{"price":1},{"price":2}]},[{"orders":{"price":3}}]]"#; width]
                .join(",")
        );
        for (name, source, expected) in [
            (
                "nested_sum",
                "$sum(groups.orders.price)",
                width as f64 * 6.0,
            ),
            (
                "nested_filtered_sum",
                "$sum(groups.orders[price > 1].price)",
                width as f64 * 5.0,
            ),
            (
                "nested_filtered_count",
                "$count(groups.orders[price > 1].price)",
                width as f64 * 2.0,
            ),
        ] {
            workload(
                &format!("aggregate/{name}"),
                source,
                &input,
                Some(expected),
                smoke,
            );
        }
    }
    let deep = format!("{}{{\"id\":7}}{}", "[".repeat(64), "]".repeat(64));
    workload("aggregate/deep_sum", "$sum(id)", &deep, Some(7.0), smoke);
    workload(
        "aggregate/deep_filtered_sum",
        "$sum(id[$ > 0])",
        &deep,
        Some(7.0),
        smoke,
    );
}
