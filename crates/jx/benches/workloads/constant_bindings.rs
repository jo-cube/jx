use super::measure_allocations;
use jx::{CompileOptions, Evaluation, EvaluationOptions, InputPlan, OwnedValue};
use std::{hint::black_box, sync::Arc};

fn consume(evaluation: Evaluation<'_, '_>) {
    evaluation
        .for_each(|value| {
            black_box(value);
        })
        .unwrap();
}
fn output(evaluation: Evaluation<'_, '_>) -> Vec<Vec<u8>> {
    let mut values = Vec::new();
    evaluation
        .for_each(|value| {
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            values.push(bytes);
        })
        .unwrap();
    values
}
fn workload(
    name: &str,
    sources: &[&str],
    literals: &[&str],
    input: &[u8],
    config: &Arc<OwnedValue>,
    budget: u64,
    smoke: bool,
) {
    let runtime = CompileOptions::default().binding("config");
    let constant = CompileOptions::default().constant_binding("config", config.clone());
    let expressions = [
        sources
            .iter()
            .map(|s| runtime.compile(s).unwrap())
            .collect::<Vec<_>>(),
        sources
            .iter()
            .map(|s| constant.compile(s).unwrap())
            .collect(),
        literals.iter().map(|s| jx::compile(s).unwrap()).collect(),
    ];
    let plans = expressions.iter().map(InputPlan::new).collect::<Vec<_>>();
    let raw = jx::validate(input).unwrap();
    for (index, source) in sources.iter().enumerate() {
        let expected = output(
            expressions[0][index]
                .evaluate_validated_with(
                    Some(raw),
                    EvaluationOptions {
                        bindings: vec![("config", config.as_value())],
                        ..Default::default()
                    },
                )
                .unwrap(),
        );
        for expressions in &expressions[1..] {
            assert_eq!(
                output(expressions[index].evaluate_validated(raw).unwrap()),
                expected,
                "{source}"
            );
        }
    }
    let mut modes = [(0, "runtime"), (1, "constant"), (2, "literal")];
    if std::env::var_os("JX_BENCH_CONSTANT_REVERSE").is_some() {
        modes.reverse();
    }
    for (index, mode) in modes {
        for acquisition in ["full", "validated", "independent_validated"] {
            measure_allocations(
                &format!("constant_bindings/{name}/{mode}/{acquisition}"),
                input.len(),
                smoke,
                Some(if index == 0 { 64 } else { budget }),
                || {
                    let prepared = match acquisition {
                        "full" => Some(plans[index].prepare(black_box(input)).unwrap()),
                        "validated" => Some(plans[index].prepare_validated(black_box(raw))),
                        _ => None,
                    };
                    for (at, expression) in expressions[index].iter().enumerate() {
                        let evaluation = match (&prepared, index) {
                            (Some(prepared), 0) => prepared.evaluate_with(
                                at,
                                EvaluationOptions {
                                    bindings: vec![("config", config.as_value())],
                                    ..Default::default()
                                },
                            ),
                            (Some(prepared), _) => prepared.evaluate(at),
                            (None, 0) => expression.evaluate_validated_with(
                                Some(black_box(raw)),
                                EvaluationOptions {
                                    bindings: vec![("config", config.as_value())],
                                    ..Default::default()
                                },
                            ),
                            (None, _) => expression.evaluate_validated(black_box(raw)),
                        };
                        consume(evaluation.unwrap());
                    }
                },
            );
        }
    }
}
pub(super) fn run(smoke: bool) {
    let config=Arc::new(OwnedValue::from_json(br#"{"skip":"never","deleted":"deleted","label":"retained","nested":{"label":"retained"},"map":{"x":"retained","never":"never"},"n":3}"#).unwrap());
    for size in [32, 1024] {
        let base = r#"{"id":"x","n":2,"key":"label"}"#;
        let input = if size == 32 {
            format!("{base:width$}", width = size)
        } else {
            format!(
                r#"{},"pad":"{}"}}"#,
                &base[..base.len() - 1],
                "x".repeat(size - base.len() - 9)
            )
        };
        assert_eq!(input.len(), size);
        for (name, sources, literals, budget) in [
            ("predicate", vec!["id=$config.skip"], vec!["id='never'"], 0),
            (
                "projection",
                vec!["{'id':id,'label':$config.label}"],
                vec!["{'id':id,'label':'retained'}"],
                3,
            ),
            (
                "multi",
                vec![
                    "id=$config.skip",
                    "id=$config.deleted",
                    "{'id':id,'label':$config.label}",
                ],
                vec!["id='never'", "id='deleted'", "{'id':id,'label':'retained'}"],
                3,
            ),
            (
                "repeated_access",
                vec![
                    "id=$config.skip or id=$lookup($config,'deleted') or $config.label=$lookup($config.nested,'label')",
                ],
                vec!["id='never' or id='deleted' or 'retained'='retained'"],
                0,
            ),
            (
                "lookup",
                vec!["$lookup($config, key)"],
                vec![
                    "$lookup({'skip':'never','deleted':'deleted','label':'retained','nested':{'label':'retained'},'map':{'x':'retained','never':'never'},'n':3},key)",
                ],
                0,
            ),
            (
                "nested_lookup",
                vec!["$lookup($config.map,id)"],
                vec!["$lookup({'x':'retained','never':'never'},id)"],
                0,
            ),
            ("numeric_plan", vec!["n*$config.n+n>5"], vec!["n*3+n>5"], 0),
            (
                "fallback",
                vec!["($config := {'skip':'never'}; id=$config.skip)"],
                vec!["($config := {'skip':'never'}; id=$config.skip)"],
                16,
            ),
        ] {
            workload(
                name,
                &sources,
                &literals,
                input.as_bytes(),
                &config,
                budget,
                smoke,
            );
        }
    }
}
