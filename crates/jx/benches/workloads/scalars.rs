use super::{measure, measure_allocations};
use std::hint::black_box;

fn workload(
    name: &str,
    source: &str,
    input: &[u8],
    expected: &[u8],
    smoke: bool,
    allocations: u64,
) {
    let expression = jx::compile(source).unwrap();
    let mut output = Vec::new();
    expression
        .evaluate(input)
        .unwrap()
        .for_each(|value| value.write_compact(&mut output).unwrap());
    assert_eq!(output, expected, "{name}: {source}");
    measure_allocations(name, input.len(), smoke, Some(allocations), || {
        expression
            .evaluate(black_box(input))
            .unwrap()
            .for_each(|value| {
                black_box(value);
            });
    });
}

pub(super) fn run(smoke: bool) {
    measure_allocations("compile/scalar", 0, smoke, None, || {
        black_box(jx::compile(black_box("(a * b + customer.id) >= 50 and active")).unwrap());
    });
    for size in [100, 500, 1024, 10 * 1024, 64 * 1024, 1024 * 1024] {
        let base =
            r#"{"a":7,"b":2.5,"active":true,"customer":{"id":42},"name":"Ada","padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        assert_eq!(input.len(), size);
        for (name, source, expected) in [
            ("literal", "42", "42"),
            ("arithmetic", "a * b + 1", "18.5"),
            ("nested", "customer.id / b", "16.8"),
            ("comparison", "a > b", "true"),
            ("strings", "name = 'Ada'", "true"),
            ("boolean", "active and a > 3", "true"),
            ("missing", "unknown + a", ""),
            ("short_circuit", "true or (a + null)", "true"),
            ("mixed", "(a * b + customer.id) >= 50 and active", "true"),
        ] {
            workload(
                &format!("scalar/{name}"),
                source,
                input.as_bytes(),
                expected.as_bytes(),
                smoke,
                0,
            );
        }
        let expression = jx::compile("a * b + 1").unwrap();
        let mut output = Vec::with_capacity(32);
        measure("scalar/write", input.len(), smoke, || {
            output.clear();
            expression
                .evaluate(black_box(input.as_bytes()))
                .unwrap()
                .for_each(|value| value.write_compact(&mut output).unwrap());
            black_box(&output);
        });
    }
    let rows = (0..16)
        .map(|i| format!(r#"{{"x":{i}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let input = format!(
        r#"{{"a":[{rows}],"b":[{rows}],"values":[{}]}}"#,
        (0..16).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    );
    for (name, source, limit) in [
        ("sequence_boolean", "a.x or false", 0),
        ("sequence_array_equality", "a.x = values", 0),
        ("sequence_equality", "a.x = b.x", 3),
        ("array_equality", "a = b", 16),
    ] {
        workload(
            &format!("scalar/{name}"),
            source,
            input.as_bytes(),
            b"true",
            smoke,
            limit,
        );
    }
    workload(
        "scalar/singleton",
        "a.x + 1",
        br#"{"a":[{"x":2}]}"#,
        b"3",
        smoke,
        0,
    );
    let expression = jx::compile("a.x + 1").unwrap();
    measure("scalar/sequence_type_error", input.len(), smoke, || {
        let error = expression
            .evaluate(black_box(input.as_bytes()))
            .unwrap_err();
        assert_eq!(error.kind, jx::ErrorKind::TypeError);
        black_box(error);
    });
    for width in [8, 128] {
        let fields = (0..width)
            .map(|i| format!(r#""k{i}":{i}"#))
            .collect::<Vec<_>>()
            .join(",");
        let input = format!(r#"{{"a":{{{fields}}},"b":{{{fields}}}}}"#);
        workload(
            "scalar/object_equality",
            "a = b",
            input.as_bytes(),
            b"true",
            smoke,
            8,
        );
    }
    workload(
        "scalar/escaped_strings",
        r#"a = "é😀""#,
        br#"{"a":"\u00e9\ud83d\ude00"}"#,
        b"true",
        smoke,
        0,
    );
}
