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
    assert_eq!(actual, vec![expected], "{name}");
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
    measure_allocations("compile/lexical", 0, smoke, None, || {
        black_box(
            jx::compile(black_box(
                "($x:=orders.price;$f:=function($v){$sum($v)};$f($x))",
            ))
            .unwrap(),
        );
    });
    for size in [100, 500, 1024, 10 * 1024, 64 * 1024, 1024 * 1024] {
        let base =
            r#"{"id":7,"customer":{"id":42},"orders":[{"price":5},{"price":20}],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected) in [
            ("lookup", "($x:=id;$x)", json!(7)),
            ("repeated_binding", "($x:=customer.id;$x+$x+$x)", json!(126)),
            (
                "retained_sequence",
                "($x:=orders.price;$sum($x)+$count($x))",
                json!(27),
            ),
            (
                "closure_call",
                "($base:=id;$f:=function($x){$x+$base};$f(customer.id))",
                json!(49),
            ),
            (
                "escaped_closure",
                "($make:=function($x){function($y){$x+$y}};$f:=$make(id);$f(customer.id))",
                json!(49),
            ),
            (
                "mixed",
                "($limit:=10;$total:=function($rows){$sum($rows.price)};$rows:=orders[price>$limit];{\"id\":$$.id,\"total\":$total($rows),\"count\":$count($rows)})",
                json!({"id":7,"total":20,"count":1}),
            ),
        ] {
            workload(
                &format!("lexical/{name}"),
                source,
                &input,
                expected,
                32,
                smoke,
            );
        }
    }
    for width in [8, 128, 1024, 16384] {
        let input =
            json!({"rows":(0..width).map(|price| json!({"price":price})).collect::<Vec<_>>()})
                .to_string();
        workload(
            "lexical/mapped_calls",
            "($f:=function($x){$x+1};$sum(rows.$f(price)))",
            &input,
            json!(width * (width + 1) / 2),
            width * 4 + 32,
            smoke,
        );
        workload(
            "lexical/repeated_projection",
            "($x:=rows.price;$sum($x)+$sum($x))",
            &input,
            json!(width * (width - 1)),
            32,
            smoke,
        );
    }
    for width in [16, 256, 16384] {
        let input = format!(
            r#"{{"groups":[{}]}}"#,
            vec![r#"[{"orders":[{"price":1},{"price":2}]},[{"orders":{"price":3}}]]"#; width]
                .join(",")
        );
        workload(
            "lexical/nested_retention",
            "($x:=groups.orders[price>1].price;$sum($x)+$count($x))",
            &input,
            json!(width * 7),
            32,
            smoke,
        );
    }
}
