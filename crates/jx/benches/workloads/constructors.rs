use super::measure_allocations;
use serde_json::{Value, json};
use std::hint::black_box;

fn workload(
    name: &str,
    source: &str,
    input: &str,
    expected: &[Value],
    budget: u64,
    write: bool,
    smoke: bool,
) {
    let expression = jx::compile(source).unwrap();
    let mut actual = Vec::new();
    let mut output = Vec::new();
    expression
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|value| {
            output.clear();
            value.write_compact(&mut output).unwrap();
            actual.push(serde_json::from_slice::<Value>(&output).unwrap());
        })
        .unwrap();
    assert_eq!(actual, expected, "{name}: {source}");
    // A reused sink includes output copying, excluding buffer growth and physical I/O.
    output.reserve(input.len() * 2 + 1024);
    measure_allocations(name, input.len(), smoke, Some(budget), || {
        output.clear();
        expression
            .evaluate(black_box(input.as_bytes()))
            .unwrap()
            .for_each(|value| {
                if write {
                    value.write_compact(&mut output).unwrap();
                }
                black_box(value);
            })
            .unwrap();
        black_box(&output);
    });
}

pub(super) fn run(smoke: bool) {
    measure_allocations("compile/constructor", 0, smoke, None, || {
        black_box(
            jx::compile(black_box(
                r#"{"total":$sum(orders[price>10].price),"ids":[orders[price>10].id]}"#,
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
        assert_eq!(input.len(), size);
        let raw: Value = serde_json::from_str(&input).unwrap();
        for (name, source, expected, write) in [
            (
                "raw_member",
                r#"{"record":$}"#,
                vec![json!({"record":raw})],
                false,
            ),
            (
                "raw_member_write",
                r#"{"record":$}"#,
                vec![json!({"record":raw})],
                true,
            ),
            (
                "scalars",
                r#"{"id":id,"price":2+3,"missing":absent,"null":null}"#,
                vec![json!({"id":7,"price":5,"null":null})],
                false,
            ),
            (
                "filtered_summary",
                r#"{"total":$sum(orders[price>10].price),"ids":[orders[price>10].id]}"#,
                vec![json!({"total":20,"ids":[2]})],
                false,
            ),
            (
                "nested",
                r#"{"id":id,"summary":{"total":$sum(orders.price)},"rows":[orders.{"id":id,"double":price*2}]}"#,
                vec![
                    json!({"id":7,"summary":{"total":25},"rows":[{"id":1,"double":10},{"id":2,"double":40}]}),
                ],
                false,
            ),
            (
                "navigate",
                r#"{"total":$sum(orders.price)}.total + 1"#,
                vec![json!(26)],
                false,
            ),
        ] {
            workload(
                &format!("construct/{name}"),
                source,
                &input,
                &expected,
                24,
                write,
                smoke,
            );
        }
    }
    for width in [8, 128, 1024, 16384] {
        let rows = (0..width)
            .map(|i| json!({"id":i,"price":i,"keep":i%2==0}))
            .collect::<Vec<_>>();
        let input = json!({"rows":rows}).to_string();
        let expected = rows
            .iter()
            .filter(|r| r["keep"] == true)
            .map(|r| json!({"id":r["id"],"double":r["price"].as_u64().unwrap()*2}))
            .collect::<Vec<_>>();
        workload(
            "construct/mapped_objects",
            r#"rows[keep].{"id":id,"double":price*2}"#,
            &input,
            &expected,
            width as u64 * 3,
            false,
            smoke,
        );
        workload(
            "construct/collected_objects",
            r#"[rows[keep].{"id":id,"double":price*2}]"#,
            &input,
            &[json!(expected)],
            width as u64 * 3 + 20,
            false,
            smoke,
        );
        workload(
            "construct/collected_write",
            r#"[rows[keep].{"id":id,"double":price*2}]"#,
            &input,
            &[json!(expected)],
            width as u64 * 3 + 20,
            true,
            smoke,
        );
        workload(
            "construct/filtered_aggregate",
            r#"{"total":$sum(rows[keep].price),"count":$count(rows[keep])}"#,
            &input,
            &[json!({"total":(width/2)*(width/2-1),"count":width/2})],
            3,
            false,
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
            "construct/nested_summary",
            r#"{"sum":$sum(groups.orders[price>1].price),"count":$count(groups.orders[price>1])}"#,
            &input,
            &[json!({"sum":width*5,"count":width*2})],
            3,
            false,
            smoke,
        );
    }
}
