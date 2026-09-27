use super::{compiler::workload, measure_allocations};
use serde_json::json;
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    for size in [100, 500, 1024, 10240, 1048576] {
        let base = r#"{"x":7,"y":3,"active":true,"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, allocations) in [
            ("branch", "x > y ? x*x + y*y : x-y", json!([58]), 0),
            (
                "boolean",
                "active and x > y and (x+y < 20 or y=0)",
                json!([true]),
                0,
            ),
            (
                "object",
                r#"{"sum":x+y,"product":x*y,"large":x>y,"schema":{"v":1}}"#,
                json!([{"sum":10,"product":21,"large":true,"schema":{"v":1}}]),
                4,
            ),
        ] {
            workload(
                &format!("plan/{name}"),
                source,
                input.as_bytes(),
                expected,
                allocations,
                smoke,
            );
        }
    }
    let table = (0..4096)
        .map(|i| (format!("k{i:04}"), json!(i)))
        .collect::<serde_json::Map<_, _>>();
    let lookup = format!("$lookup({},key)", serde_json::to_string(&table).unwrap());
    let source = format!("active ? ({lookup})*qty + fee : 0");
    let input = format!(
        r#"{{"key":"k4095","active":true,"qty":2,"fee":3,"padding":"{}"}}"#,
        "x".repeat(432)
    );
    workload(
        "plan/lookup_branch",
        &source,
        input.as_bytes(),
        json!([8193]),
        0,
        smoke,
    );
    let quote = format!("active ? ({lookup})*qty + qty*fee + qty : 0");
    workload(
        "plan/lookup_quote",
        &quote,
        input.as_bytes(),
        json!([8198]),
        0,
        smoke,
    );
    for count in [1, 8, 128, 1024, 16384] {
        let rows = (0..count)
            .map(|n| json!({"price":n%16,"qty":2,"active":n%2==0}))
            .collect::<Vec<_>>();
        let input = serde_json::to_vec(&json!({"payload":{"orders":rows}})).unwrap();
        let total: u64 = (0..count)
            .filter(|n| n % 2 == 0 && n % 16 > 3)
            .map(|n| (n % 16 * 2 + 1) as u64)
            .sum();
        let sum = "$sum(payload.orders[active and price>3].(price*qty+1))";
        let expected = if total == 0 {
            json!([])
        } else {
            json!([total])
        };
        workload("plan/invoice_sum", sum, &input, expected, 0, smoke);
        let select = jx::compile("payload.orders").unwrap();
        let rust_sum = || {
            let mut total = None;
            select
                .evaluate(black_box(&input))
                .unwrap()
                .for_each(|value| {
                    let raw = value.as_raw().unwrap().as_str();
                    for row in raw[1..raw.len() - 1].split("},") {
                        if row.is_empty() {
                            continue;
                        }
                        let active = row.contains("\"active\":true");
                        let price: f64 = row
                            .split_once("\"price\":")
                            .unwrap()
                            .1
                            .split(',')
                            .next()
                            .unwrap()
                            .parse()
                            .unwrap();
                        let qty: f64 = row
                            .split_once("\"qty\":")
                            .unwrap()
                            .1
                            .trim_end_matches('}')
                            .parse()
                            .unwrap();
                        if active && price > 3.0 {
                            total = Some(total.unwrap_or(0.0) + price * qty + 1.0);
                        }
                    }
                })
                .unwrap();
            total
        };
        assert_eq!(rust_sum(), (total > 0).then_some(total as f64));
        measure_allocations("rust/plan_invoice", input.len(), smoke, Some(0), || {
            black_box(rust_sum());
        });
        let expected = if total == 0 {
            json!([{"version":1}])
        } else {
            json!([{"version":1,"total":total}])
        };
        workload(
            "plan/invoice_object",
            &format!(r#"{{"version":1,"total":{sum}}}"#),
            &input,
            expected,
            3,
            smoke,
        );
        let expected = rows.iter().map(|r| json!({"gross": r["price"].as_u64().unwrap()*2,"large":r["price"].as_u64().unwrap()>3})).collect::<Vec<_>>();
        workload(
            "plan/mapped_objects",
            r#"payload.orders.{"gross":price*qty,"large":price>3}"#,
            &input,
            json!(expected),
            3 * count as u64,
            smoke,
        );
    }
    let wide = (0..512)
        .map(|i| (format!("field{i:03}"), json!(i)))
        .chain([("x".to_owned(), json!(7)), ("y".to_owned(), json!(3))])
        .collect::<serde_json::Map<_, _>>();
    let wide = serde_json::to_vec(&wide).unwrap();
    workload(
        "plan/wide_fields",
        "x>y ? x*x+y*y : x-y",
        &wide,
        json!([58]),
        0,
        smoke,
    );
    let input = format!(
        r#"{{"active":false,"x":7,"y":3,"padding":"{}"}}"#,
        "x".repeat(455)
    );
    workload(
        "plan/untaken_branch",
        "active ? (x*x+y*y)/(x+y+1) : 0",
        input.as_bytes(),
        json!([0]),
        0,
        smoke,
    );
    measure_allocations("compile/plan_branch", 0, smoke, None, || {
        black_box(jx::compile(black_box("x > y ? x*x + y*y : x-y")).unwrap());
    });
}
