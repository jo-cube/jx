use super::{compiler::workload, measure_allocations};
use serde_json::json;
use std::{collections::HashMap, hint::black_box};

const ARITHMETIC: &str = "((x+y)*(x-y)+(x*x+y*y))/(x+1)-y*3";
fn arithmetic(x: f64, y: f64) -> f64 {
    ((x + y) * (x - y) + (x * x + y * y)) / (x + 1.0) - y * 3.0
}
fn fields(input: &[u8]) -> (f64, f64) {
    let raw = jx::validate(input).unwrap().as_str();
    let x = raw[5..].split(',').next().unwrap().parse().unwrap();
    let y = raw
        .split_once("\"y\":")
        .unwrap()
        .1
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    (x, y)
}

pub(super) fn run(smoke: bool) {
    measure_allocations("compile/execution", 0, smoke, None, || {
        black_box(jx::compile(black_box(ARITHMETIC)).unwrap());
    });
    for size in [100, 500, 1024, 10240, 1048576] {
        let base = r#"{"x":7,"y":3,"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        let n = arithmetic(7.0, 3.0);
        let constructor =
            format!(r#"{{"schema":{{"v":1,"units":["n","m"]}},"value":{ARITHMETIC}}}"#);
        for (label, expression, expected, limit) in [
            ("arithmetic", ARITHMETIC.to_owned(), json!([n]), 0),
            (
                "comparison",
                format!("({ARITHMETIC})>0"),
                json!([n > 0.0]),
                0,
            ),
            (
                "conditional",
                format!("x>y ? ({ARITHMETIC}) : 0"),
                json!([n]),
                0,
            ),
            (
                "constructor",
                constructor,
                json!([{"schema":{"v":1,"units":["n","m"]},"value":n}]),
                4,
            ),
        ] {
            workload(
                &format!("execution/{label}"),
                &expression,
                input.as_bytes(),
                expected,
                limit,
                smoke,
            );
        }
        // Controls parse the fixed ASCII fixture once after complete validation.
        assert_eq!(fields(input.as_bytes()), (7.0, 3.0));
        measure_allocations("rust/execution_arithmetic", size, smoke, Some(0), || {
            let (x, y) = fields(black_box(input.as_bytes()));
            black_box(arithmetic(x, y));
        });
        measure_allocations("rust/execution_constructor", size, smoke, Some(0), || {
            let (x, y) = fields(black_box(input.as_bytes()));
            black_box((r#"{"v":1,"units":["n","m"]}"#, arithmetic(x, y)));
        });
    }
    for count in [8, 128, 1024, 16384] {
        let rows = (0..count).map(|n| json!({"v":n%16})).collect::<Vec<_>>();
        let input = serde_json::to_vec(&json!({"rows":rows})).unwrap();
        let expected: f64 = (0..count)
            .map(|n| (n % 16) as f64)
            .filter(|v| (v * 2.0 + v + 1.0) > 12.0)
            .sum();
        workload(
            "execution/filtered_sum",
            "$sum(rows[(v*2+v+1)>12].v)",
            &input,
            json!([expected as u64]),
            0,
            smoke,
        );
        let mapped: f64 = (0..count)
            .map(|n| (n % 16) as f64)
            .map(|v| (v * 2.0 + v * v + 1.0) / (v + 1.0))
            .sum();
        workload(
            "execution/mapped_sum",
            "$sum(rows.((v*2+v*v+1)/(v+1)))",
            &input,
            json!([mapped as u64]),
            0,
            smoke,
        );
        let select = jx::compile("rows").unwrap();
        // The same validated fixture, with a purpose-written numeric member reader.
        measure_allocations(
            "rust/execution_filtered_sum",
            input.len(),
            smoke,
            Some(0),
            || {
                select
                    .evaluate(black_box(&input))
                    .unwrap()
                    .for_each(|value| {
                        let raw = value.as_raw().unwrap().as_str();
                        let sum: f64 = raw
                            .split("\"v\":")
                            .skip(1)
                            .map(|s| s.split('}').next().unwrap().parse::<f64>().unwrap())
                            .filter(|v| (v * 2.0 + v + 1.0) > 12.0)
                            .sum();
                        black_box(sum);
                    })
                    .unwrap();
            },
        );
    }
    let width = 8192;
    let map = (0..width)
        .map(|i| (format!("k{i:05}"), json!(i)))
        .collect::<serde_json::Map<_, _>>();
    let source = format!("$lookup({},key)", serde_json::to_string(&map).unwrap());
    let input = format!(r#"{{"key":"k08191","padding":"{}"}}"#, "x".repeat(472));
    workload(
        "execution/lookup_8192",
        &source,
        input.as_bytes(),
        json!([8191]),
        0,
        smoke,
    );
    let map = (0..width)
        .map(|i| (format!("k{i:05}"), i))
        .collect::<HashMap<_, _>>();
    let select = jx::compile("key").unwrap();
    measure_allocations(
        "rust/execution_lookup_8192",
        input.len(),
        smoke,
        Some(0),
        || {
            select
                .evaluate(black_box(input.as_bytes()))
                .unwrap()
                .for_each(|value| {
                    let raw = value.as_raw().unwrap().as_str();
                    black_box(map.get(&raw[1..raw.len() - 1]));
                })
                .unwrap();
        },
    );
    let expression = jx::compile(ARITHMETIC).unwrap();
    let input = br#"{"x":true,"y":3}"#;
    assert_eq!(
        expression.evaluate(input).unwrap_err().kind,
        jx::ErrorKind::TypeError
    );
    measure_allocations(
        "execution/type_fallback",
        input.len(),
        smoke,
        Some(0),
        || {
            black_box(expression.evaluate(black_box(input)).unwrap_err());
        },
    );
}
