use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;

fn padded(base: &str, size: usize) -> String {
    base.replace(
        "\"padding\":\"\"",
        &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
    )
}

pub(super) fn run(smoke: bool) {
    for size in [100, 500, 1024, 10240, 1048576] {
        let input = padded(
            r#"{"a":2,"b":3,"c":4,"nested":{"a":5,"b":6,"c":7},"padding":""}"#,
            size,
        );
        for (name, source, expected) in [
            ("call_one", "function($a){$a+1}(a)", 3),
            ("call_three", "function($a,$b,$c){$a*$b+$c}(a,b,c)", 10),
            (
                "call_six",
                "function($a,$b,$c,$d,$e,$f){$a*$b+$c+$d*$e+$f}(a,b,c,nested.a,nested.b,nested.c)",
                47,
            ),
            (
                "call_nested",
                "function($a,$b,$c){$a*$b+$c}(nested.a,nested.b,nested.c)",
                37,
            ),
            (
                "call_repeated",
                "function($a,$b,$c){$a*$b+$c}(nested.a,nested.a,nested.a)",
                30,
            ),
        ] {
            workload(
                &format!("acquisition/{name}"),
                source,
                &input,
                json!([expected]),
                12,
                smoke,
            );
        }
    }
    for width in [1, 4, 32, 128, 4096, 8192, 32768] {
        for (kind, prefix) in [
            ("short", "k"),
            ("ascii", "key-shared-prefix-"),
            ("unicode", "é中😀-"),
        ] {
            let map = (0..width)
                .map(|i| (format!("{prefix}{i:05}"), json!(i)))
                .collect::<serde_json::Map<_, _>>();
            let source = format!("$lookup({},key)", serde_json::to_string(&map).unwrap());
            for (encoding, escaped) in [("raw", false), ("escaped", true)] {
                let key = format!("{prefix}{:05}", width - 1);
                let key = if escaped {
                    format!(
                        "\"{}\"",
                        key.encode_utf16()
                            .map(|u| format!("\\u{u:04x}"))
                            .collect::<String>()
                    )
                } else {
                    serde_json::to_string(&key).unwrap()
                };
                let input = padded(&format!(r#"{{"key":{key},"padding":""}}"#), 500);
                workload(
                    &format!("acquisition/lookup_{width}_{kind}_{encoding}"),
                    &source,
                    &input,
                    json!([width - 1]),
                    0,
                    smoke,
                );
            }
            // Alternate hits, misses and keys; no per-record key cache can win this case.
            let expression = jx::compile(&source).unwrap();
            let records = [0, width / 2, width - 1, width]
                .map(|i| padded(&format!(r#"{{"key":"{prefix}{i:05}","padding":""}}"#), 500));
            for (i, input) in records.iter().enumerate() {
                let mut output = Vec::new();
                expression
                    .evaluate(input.as_bytes())
                    .unwrap()
                    .for_each(|v| v.write_compact(&mut output).unwrap())
                    .unwrap();
                assert_eq!(
                    output,
                    if i == 3 {
                        Vec::new()
                    } else {
                        (if i == 0 {
                            0
                        } else if i == 1 {
                            width / 2
                        } else {
                            width - 1
                        })
                        .to_string()
                        .into_bytes()
                    }
                );
            }
            let mut at = 0;
            measure_allocations(
                &format!("acquisition/lookup_{width}_{kind}_varying"),
                500,
                smoke,
                Some(0),
                || {
                    expression
                        .evaluate(black_box(records[at].as_bytes()))
                        .unwrap()
                        .for_each(|v| {
                            black_box(v);
                        })
                        .unwrap();
                    at = (at + 1) % records.len();
                },
            );
            if kind == "ascii" && width == 4096 {
                let lookup = format!("$lookup({},key)", serde_json::to_string(&map).unwrap());
                let input = padded(
                    r#"{"key":"key-shared-prefix-04095","a":2,"b":3,"padding":""}"#,
                    500,
                );
                workload(
                    "acquisition/lookup_repeated",
                    &format!("{lookup}+{lookup}+{lookup}"),
                    &input,
                    json!([12285]),
                    0,
                    smoke,
                );
                workload(
                    "acquisition/lookup_mixed",
                    &format!("{{'value':({lookup})*a+b,'active':({lookup})>a}}"),
                    &input,
                    json!([{"value":8193,"active":true}]),
                    4,
                    smoke,
                );
            }
        }
    }
    for size in [100, 500, 10240] {
        let input = padded(
            r#"{"a":"Ada","b":"Lovelace","unicode":"é😀","escaped":"\u00e9\ud83d\ude00","padding":""}"#,
            size,
        );
        for (name, source, expected, limit) in [
            ("string_borrowed", "a", json!(["Ada"]), 0),
            ("string_compare", "a='Ada'", json!([true]), 0),
            (
                "string_unicode_compare",
                "unicode=escaped",
                json!([true]),
                0,
            ),
            ("string_concat", "a&b", json!(["AdaLovelace"]), 2),
            (
                "string_owned_compare",
                "(a&b)='AdaLovelace'",
                json!([true]),
                2,
            ),
            (
                "string_construct",
                "{'name':a&b,'label':a&'-'&b}",
                json!([{"name":"AdaLovelace","label":"Ada-Lovelace"}]),
                12,
            ),
            (
                "string_callback",
                "$map([a,b],function($s){$s&'!'})",
                json!(["Ada!", "Lovelace!"]),
                24,
            ),
        ] {
            workload(
                &format!("acquisition/{name}"),
                source,
                &input,
                expected,
                limit,
                smoke,
            );
        }
    }
}
