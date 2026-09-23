use super::{array_workload, measure_allocations};
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    measure_allocations("compile/filter", 0, smoke, None, || {
        black_box(jx::compile(black_box("orders[price > 10 and active].id")).unwrap());
    });
    for size in [100, 500, 1024, 10 * 1024, 64 * 1024, 1024 * 1024] {
        let base = r#"{"orders":[{"id":1,"price":5},{"id":2,"price":20}],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        assert_eq!(input.len(), size);
        for (name, source, count) in [
            ("predicate", "orders[price > 10].id", 1),
            ("none", "orders[price < 0].id", 0),
            ("first", "orders[0].id", 1),
            ("last", "orders[-1].id", 1),
            ("computed_last", "orders[-(1+0)].id", 1),
            ("chained", "orders[price > 0][price < 10].id", 1),
        ] {
            array_workload(&format!("filter/{name}"), source, &input, count, smoke);
        }
    }
    for width in [8, 128, 1024] {
        let rows = (0..width)
            .map(|i| format!(r#"{{"id":{i},"price":{i}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let input = format!(r#"{{"orders":[{rows}]}}"#);
        array_workload(
            "filter/wide_predicate",
            "orders[price % 2 = 0].id",
            &input,
            width / 2,
            smoke,
        );
        array_workload("filter/wide_last", "orders[-1].id", &input, 1, smoke);
        array_workload(
            "filter/sequence_last",
            "(orders.id)[-(1+0)]",
            &input,
            1,
            smoke,
        );
    }
    let input = format!(
        r#"{{"groups":[{}]}}"#,
        vec![r#"[{"orders":[{"id":1},{"id":2}]},[{"orders":{"id":3}}]]"#; 16].join(",")
    );
    array_workload(
        "filter/nested",
        "groups.orders[id > 1].id",
        &input,
        32,
        smoke,
    );
    array_workload("filter/per_group", "groups.orders[0].id", &input, 16, smoke);
    array_workload("filter/global", "(groups.orders)[0].id", &input, 1, smoke);
    let deep = format!("{}{{\"id\":7}}{}", "[".repeat(64), "]".repeat(64));
    array_workload("filter/deep", "id[$ > 0]", &deep, 1, smoke);
    // Repeated computed negative positions exercise length reuse across replays.
    let chain = format!("orders{}", "[-(1+0)]".repeat(16));
    array_workload(
        "filter/negative_chain",
        &chain,
        r#"{"orders":[1,2,3,4]}"#,
        1,
        smoke,
    );
}
