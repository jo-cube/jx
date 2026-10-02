use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;
pub(super) fn run(smoke: bool) {
    for (name, source) in [
        ("static", "$eval('n+v')"),
        ("dynamic", "$eval(code)"),
        ("nested", "$eval('$eval(\"n+v\")')"),
    ] {
        measure_allocations(&format!("compile/eval_{name}"), 0, smoke, None, || {
            black_box(jx::compile(black_box(source)).unwrap());
        });
    }
    for size in [100, 500, 1024, 10 * 1024, 1024 * 1024] {
        let base = r#"{"n":7,"v":42,"obj":{"n":3,"v":4},"code":"n+v","a":[1,2,3],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, limit) in [
            ("static_scalar", "$eval('n+v')", json!([49]), 0),
            ("dynamic_scalar", "$eval(code)", json!([49]), 100),
            (
                "static_literal",
                "$eval('{\"n\":7,\"a\":[1,2]}')",
                json!([{"n":7,"a":[1,2]}]),
                1,
            ),
            ("context", "$eval('n+v',obj)", json!([7]), 0),
            ("nested", "$eval('$eval(\"n+v\")')", json!([49]), 0),
            ("inherited", "($x:=n;$eval('$x+v'))", json!([49]), 12),
            (
                "static_closure",
                "($f:=$eval('function($x){$x+n}');$f(v))",
                json!([49]),
                12,
            ),
            (
                "folded_literal",
                "$eval(\"{\\\"n\\\":7}\" & \"\")",
                json!([{"n":7}]),
                1,
            ),
            (
                "mixed",
                "{'total':$eval('$sum(a)'), 'n':obj.n, 'random':$random()>=0}",
                json!([{"total":6,"n":3,"random":true}]),
                20,
            ),
        ] {
            workload(
                &format!("eval/{name}"),
                source,
                &input,
                expected,
                limit,
                smoke,
            );
        }
        for (name, source) in [
            ("random", "$random()"),
            ("random_injected", "$random()"),
            ("shuffle", "$shuffle(a)"),
            ("shuffle_empty", "$shuffle([])"),
            ("shuffle_single", "$shuffle([1])"),
        ] {
            let expression = jx::compile(source).unwrap();
            let random = jx::Random::seeded(1);
            measure_allocations(
                &format!("effect/{name}"),
                input.len(),
                smoke,
                Some(10),
                || {
                    let result = if name == "random_injected" {
                        expression.evaluate_with_random(black_box(input.as_bytes()), &random)
                    } else {
                        expression.evaluate(black_box(input.as_bytes()))
                    };
                    result
                        .unwrap()
                        .for_each(|v| {
                            black_box(v);
                        })
                        .unwrap();
                },
            );
        }
    }
    for width in [8, 128, 1024] {
        let input =
            json!({"a":(0..width).collect::<Vec<_>>(),"code":"function($x){$x*2}","other":7})
                .to_string();
        workload(
            "eval/dynamic_closure_map",
            "($f:=$eval(code);$map(a,$f))",
            &input,
            json!((0..width).map(|n| n * 2).collect::<Vec<_>>()),
            if width == 8 {
                102
            } else {
                width as u64 * 64 + 100
            },
            smoke,
        );
        workload(
            "eval/static_map",
            "a.$eval('$*2')",
            &input,
            json!((0..width).map(|n| n * 2).collect::<Vec<_>>()),
            width as u64 * 8 + 100,
            smoke,
        );
        workload(
            "effect/shuffle_sum",
            "$sum($shuffle(a))",
            &input,
            json!([(width - 1) * width / 2]),
            width as u64 + 16,
            smoke,
        );
    }
}
