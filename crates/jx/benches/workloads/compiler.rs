use super::measure_allocations;
use serde_json::{Value, json};
use std::{collections::HashMap, hint::black_box};

pub(super) fn workload(
    name: &str,
    source: &str,
    input: &[u8],
    expected: Value,
    limit: u64,
    smoke: bool,
) {
    let expression = jx::compile(source).unwrap();
    let mut actual = Vec::new();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| {
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            actual.push(serde_json::from_slice::<Value>(&bytes).unwrap());
        })
        .unwrap();
    assert_eq!(json!(actual), expected, "{name}");
    measure_allocations(name, input.len(), smoke, Some(limit), || {
        expression
            .evaluate(black_box(input))
            .unwrap()
            .for_each(|v| {
                black_box(v);
            })
            .unwrap();
    });
}

pub(super) fn run(smoke: bool) {
    for width in [8, 128, 1024, 4096] {
        let map = (0..width)
            .map(|i| (format!("k{i:05}"), json!(i)))
            .collect::<serde_json::Map<_, _>>();
        let literal = serde_json::to_string(&map).unwrap();
        let source = format!("$lookup({literal},key)");
        let rust_map = (0..width)
            .map(|i| (format!("k{i:05}"), i))
            .collect::<HashMap<_, _>>();
        for hit in [true, false] {
            let key = if hit {
                format!("k{:05}", width - 1)
            } else {
                "absent".into()
            };
            let input = format!(r#"{{"key":"{key}","padding":"{}"}}"#, "x".repeat(472));
            let expected = if hit { json!([width - 1]) } else { json!([]) };
            let label = if hit { "hit" } else { "miss" };
            workload(
                &format!("compiler/lookup_{width}_{label}"),
                &source,
                input.as_bytes(),
                expected,
                0,
                smoke,
            );
            // Same validated ASCII-key fixture; field extraction uses the engine's
            // validating path selector. Only the immutable map lookup is handwritten.
            let select = jx::compile("key").unwrap();
            measure_allocations(
                &format!("rust/lookup_{width}_{label}"),
                input.len(),
                smoke,
                Some(0),
                || {
                    select
                        .evaluate(black_box(input.as_bytes()))
                        .unwrap()
                        .for_each(|v| {
                            let raw = v.as_raw().unwrap().as_str();
                            black_box(rust_map.get(&raw[1..raw.len() - 1]));
                        })
                        .unwrap();
                },
            );
        }
        let records = (0..256)
            .map(|i| {
                format!(
                    r#"{{"key":"k{:05}","padding":"{}"}}"#,
                    (i * 4051) % width,
                    "x".repeat(472)
                )
                .into_bytes()
            })
            .collect::<Vec<_>>();
        let expression = jx::compile(&source).unwrap();
        for (i, record) in records.iter().enumerate() {
            let mut values = Vec::new();
            expression
                .evaluate(record)
                .unwrap()
                .for_each(|v| {
                    let mut bytes = Vec::new();
                    v.write_compact(&mut bytes).unwrap();
                    values.push(serde_json::from_slice::<Value>(&bytes).unwrap());
                })
                .unwrap();
            assert_eq!(json!(values), json!([(i * 4051) % width]));
        }
        let mut at = 0;
        measure_allocations(
            &format!("compiler/varying_lookup_{width}"),
            records[0].len(),
            smoke,
            Some(0),
            || {
                expression
                    .evaluate(black_box(&records[at]))
                    .unwrap()
                    .for_each(|v| {
                        black_box(v);
                    })
                    .unwrap();
                at = (at + 1) % records.len();
            },
        );
        let select = jx::compile("key").unwrap();
        let mut at = 0;
        measure_allocations(
            &format!("rust/varying_lookup_{width}"),
            records[0].len(),
            smoke,
            Some(0),
            || {
                select
                    .evaluate(black_box(&records[at]))
                    .unwrap()
                    .for_each(|v| {
                        let raw = v.as_raw().unwrap().as_str();
                        black_box(rust_map.get(&raw[1..raw.len() - 1]));
                    })
                    .unwrap();
                at = (at + 1) % records.len();
            },
        );
        workload(
            &format!("compiler/bound_lookup_{width}"),
            &format!("($table:={literal};$lookup($table,key))"),
            br#"{"key":"k00000"}"#,
            json!([0]),
            6,
            smoke,
        );
        measure_allocations(&format!("compile/lookup_{width}"), 0, smoke, None, || {
            black_box(jx::compile(black_box(&source)).unwrap());
        });
    }
    for size in [100, 500, 1024, 10240, 1048576] {
        let base = r#"{"x":7,"y":3,"rows":[{"v":1},{"v":5}],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, limit) in [
            ("constant_scalar", "(3*7+5)*(8-2)+(100/4)", json!([181]), 0),
            ("repeated_fields", "x+x+x+x+x+x+x+x+y+y+y+y", json!([68]), 0),
            (
                "dynamic_leaves",
                r#"{"schema":{"a":[1,2,3],"b":{"c":4}},"value":x+2*3}"#,
                json!([{"schema":{"a":[1,2,3],"b":{"c":4}},"value":13}]),
                4,
            ),
            ("filtered_sum", "$sum(rows[v>2].v)", json!([5]), 0),
            (
                "invariant_aggregate",
                "x+$sum([1,2,3,4,5,6,7,8])",
                json!([43]),
                0,
            ),
            ("builtin_reference", "$sum=$sum", json!([true]), 0),
            (
                "static_array",
                r#"[1,2,3,[4,5],{"n":6}]"#,
                json!([[1,2,3,[4,5],{"n":6}]]),
                1,
            ),
            (
                "constant_membership",
                "x in [1,2,3,4,5,6,7,8]",
                json!([true]),
                1,
            ),
        ] {
            workload(
                &format!("compiler/{name}"),
                source,
                input.as_bytes(),
                expected,
                limit,
                smoke,
            );
        }
        let table = (0..128)
            .map(|i| (format!("k{i:05}"), json!(i)))
            .collect::<serde_json::Map<_, _>>();
        let base = r#"{"key":"k00127","padding":""}"#;
        let record = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        workload(
            "compiler/lookup_record",
            &format!("$lookup({},key)", serde_json::to_string(&table).unwrap()),
            record.as_bytes(),
            json!([127]),
            0,
            smoke,
        );
        let expression = jx::compile(&format!(
            "$lookup({0},key) ?? $lookup({0},fallback)",
            serde_json::to_string(&table).unwrap(),
        ))
        .unwrap();
        let plan = jx::InputPlan::new([&expression]);
        for (name, key, fallback, expected) in [
            ("first", "k00127", "absent", Some(127.)),
            ("second", "absent", "k00127", Some(127.)),
            ("missing", "absent", "absent", None),
        ] {
            let base = format!(r#"{{"key":"{key}","fallback":"{fallback}","padding":""}}"#);
            let record = base.replace(
                "\"padding\":\"\"",
                &format!(
                    "\"padding\":\"{}\"",
                    "x".repeat(size.saturating_sub(base.len()))
                ),
            );
            measure_allocations(
                &format!("compiler/prepared_fallback_{name}"),
                record.len(),
                smoke,
                Some(0),
                || {
                    let value = plan
                        .prepare(black_box(record.as_bytes()))
                        .unwrap()
                        .evaluate(0)
                        .unwrap()
                        .single()
                        .unwrap();
                    assert_eq!(value.and_then(|v| v.as_number()), expected);
                },
            );
        }
        // Purpose-written control for this fixed ASCII object layout. It still
        // validates the complete record and parses both demanded numeric fields.
        measure_allocations("rust/repeated_fields", input.len(), smoke, Some(0), || {
            let raw = jx::validate(black_box(input.as_bytes())).unwrap().as_str();
            let x = raw[5..].split(',').next().unwrap().parse::<f64>().unwrap();
            let y = raw
                .split_once("\"y\":")
                .unwrap()
                .1
                .split(',')
                .next()
                .unwrap()
                .parse::<f64>()
                .unwrap();
            black_box(x + x + x + x + x + x + x + x + y + y + y + y);
        });
    }
}
