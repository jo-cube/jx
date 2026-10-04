use super::measure_allocations;
use jx::{CompileOptions, EvaluationOptions, Limits, Value};
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    for size in [100, 500, 1024, 10 * 1024] {
        let base = r#"{"price":2.5,"quantity":3,"label":"abc","escaped":"a\u0062c","items":[1,2],"pad":""}"#;
        let input = base.replace(
            "\"pad\":\"\"",
            &format!("\"pad\":\"{}\"", "x".repeat(size - base.len())),
        );
        let expression = jx::compile("price*quantity").unwrap();
        let options = CompileOptions::default()
            .binding("scale")
            .compile("price*$scale+quantity")
            .unwrap();
        let host = jx::HostFunction::new(2, |args, _| {
            Ok(Some(Value::Number(
                args[0].as_ref().unwrap().as_number().unwrap()
                    + args[1].as_ref().unwrap().as_number().unwrap(),
            )))
        })
        .with_signature("<nn:n>")
        .unwrap();
        let host_value = host.value();
        let call = CompileOptions::default()
            .binding("add")
            .compile("$add(price,quantity)")
            .unwrap();
        for (name, which, budget) in [
            ("direct", 0, 0),
            ("default_options", 1, 0),
            ("bindings", 2, 8),
            ("host", 3, 8),
            ("limits", 4, 8),
        ] {
            let evaluate = || match which {
                0 => expression.evaluate(input.as_bytes()),
                1 => expression.evaluate_with(Some(input.as_bytes()), EvaluationOptions::default()),
                2 => options.evaluate_with(
                    Some(input.as_bytes()),
                    EvaluationOptions {
                        bindings: vec![("scale", Value::Number(2.))],
                        ..Default::default()
                    },
                ),
                3 => call.evaluate_with(
                    Some(input.as_bytes()),
                    EvaluationOptions {
                        bindings: vec![("add", host_value.clone())],
                        ..Default::default()
                    },
                ),
                _ => expression.evaluate_with(
                    Some(input.as_bytes()),
                    EvaluationOptions {
                        limits: Some(Limits::default()),
                        ..Default::default()
                    },
                ),
            };
            let expected = match which {
                2 => 8.,
                3 => 5.5,
                _ => 7.5,
            };
            assert_eq!(
                evaluate().unwrap().single().unwrap().unwrap().as_number(),
                Some(expected)
            );
            measure_allocations(
                &format!("embedding/{name}"),
                size,
                smoke,
                Some(budget),
                || {
                    evaluate()
                        .unwrap()
                        .for_each(|v| {
                            black_box(v);
                        })
                        .unwrap();
                },
            );
        }
        for (name, source, budget) in [
            ("string_borrowed", "label", 0),
            ("string_decoded", "escaped", 2),
            ("owned_scalar", "price", 0),
            ("owned_object", "{'n':price,'items':items}", 32),
        ] {
            let expression = jx::compile(source).unwrap();
            measure_allocations(
                &format!("embedding/{name}"),
                size,
                smoke,
                Some(budget),
                || {
                    expression
                        .evaluate(black_box(input.as_bytes()))
                        .unwrap()
                        .for_each(|v| {
                            if name.starts_with("string") {
                                black_box(v.as_str().unwrap());
                            } else {
                                black_box(v.to_owned().unwrap());
                            }
                        })
                        .unwrap();
                },
            );
        }
    }
    let project = jx::HostFunction::new(1, |args, _| Ok(args[0].as_ref().unwrap().get("n")));
    let project = project.value();
    for width in [8, 128, 1024] {
        let input = format!(
            "{{\"rows\":[{}]}}",
            (0..width)
                .map(|n| format!("{{\"n\":{n}}}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let expression = CompileOptions::default()
            .binding("project")
            .compile("$sum($map(rows,$project))")
            .unwrap();
        let evaluate = || {
            expression.evaluate_with(
                Some(input.as_bytes()),
                EvaluationOptions {
                    bindings: vec![("project", project.clone())],
                    ..Default::default()
                },
            )
        };
        assert_eq!(
            evaluate().unwrap().single().unwrap().unwrap().as_number(),
            Some(((width - 1) * width / 2) as f64)
        );
        measure_allocations(
            "embedding/host_map_sum",
            input.len(),
            smoke,
            Some(32),
            || {
                evaluate()
                    .unwrap()
                    .for_each(|v| {
                        black_box(v);
                    })
                    .unwrap();
            },
        );
        let expression = jx::compile("$sum(rows[n>2].(n*n+1))").unwrap();
        measure_allocations(
            "embedding/controlled_filter_sum",
            input.len(),
            smoke,
            Some(32),
            || {
                expression
                    .evaluate_with(
                        Some(black_box(input.as_bytes())),
                        EvaluationOptions {
                            limits: Some(Limits::default()),
                            ..Default::default()
                        },
                    )
                    .unwrap()
                    .for_each(|v| {
                        black_box(v);
                    })
                    .unwrap();
            },
        );
    }
}
