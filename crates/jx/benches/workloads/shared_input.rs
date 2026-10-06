use super::measure_allocations;
use jx::{Evaluation, Expression, InputPlan};
use std::hint::black_box;

fn consume(evaluation: Evaluation<'_, '_>) {
    evaluation
        .for_each(|value| {
            black_box(value);
        })
        .unwrap();
}
fn outcome(evaluation: Evaluation<'_, '_>) -> Vec<Vec<u8>> {
    let mut results = Vec::new();
    evaluation
        .for_each(|value| {
            let mut bytes = Vec::new();
            value.write_compact(&mut bytes).unwrap();
            results.push(bytes);
        })
        .unwrap();
    results
}
fn workload(name: &str, expressions: &[Expression], input: &[u8], budget: u64, smoke: bool) {
    let plan = InputPlan::new(expressions);
    let raw = jx::validate(input).unwrap();
    let prepared = plan.prepare(input).unwrap();
    for (index, expression) in expressions.iter().enumerate() {
        let expected = outcome(expression.evaluate_validated(raw).unwrap());
        let actual = outcome(prepared.evaluate(index).unwrap());
        assert_eq!(actual, expected, "{name}");
    }
    let reverse = std::env::var_os("JX_BENCH_SHARED_REVERSE").is_some();
    let mut modes = [
        "independent_full",
        "shared_full",
        "independent_validated",
        "shared_validated",
    ];
    if reverse {
        modes.reverse();
    }
    for mode in modes {
        measure_allocations(
            &format!("shared_input/{name}/{mode}"),
            input.len(),
            smoke,
            Some(budget),
            || match mode {
                "independent_full" => {
                    let raw = jx::validate(black_box(input)).unwrap();
                    for expression in expressions {
                        consume(expression.evaluate_validated(raw).unwrap());
                    }
                }
                "shared_full" => {
                    let prepared = plan.prepare(black_box(input)).unwrap();
                    for index in 0..expressions.len() {
                        consume(prepared.evaluate(index).unwrap());
                    }
                }
                "independent_validated" => {
                    for expression in expressions {
                        consume(expression.evaluate_validated(black_box(raw)).unwrap());
                    }
                }
                _ => {
                    let prepared = plan.prepare_validated(black_box(raw));
                    for index in 0..expressions.len() {
                        consume(prepared.evaluate(index).unwrap());
                    }
                }
            },
        );
    }
}
fn fixture(base: &str, size: usize) -> String {
    if size == 32 {
        format!("{base:width$}", width = size)
    } else {
        format!(
            r#"{},"pad":"{}"}}"#,
            &base[..base.len() - 1],
            "x".repeat(size - base.len() - 9)
        )
    }
}
pub(super) fn run(smoke: bool) {
    for size in [32, 1024] {
        let root = fixture(r#"{"id":"x","a":2,"b":3,"c":4}"#, size);
        let nested = fixture(r#"{"n":{"id":"x"},"a":2,"b":3}"#, size);
        for (name, sources, input, budget) in [
            (
                "same_two",
                vec!["id='never'", "id='absent'"],
                root.as_str(),
                0,
            ),
            (
                "same_three",
                vec!["id='never'", "id='absent'", "id='deleted'"],
                root.as_str(),
                0,
            ),
            ("different", vec!["a>1", "b>1", "c>1"], root.as_str(), 0),
            (
                "nested",
                vec!["n.id='never'", "n.id='absent'", "n.id='deleted'"],
                nested.as_str(),
                0,
            ),
            (
                "projection",
                vec!["id='never'", "id='absent'", "{'id':id}"],
                root.as_str(),
                3,
            ),
            (
                "ineligible",
                vec!["$uppercase(id)", "$lowercase(id)"],
                root.as_str(),
                12,
            ),
            (
                "mixed",
                vec!["id='never'", "$uppercase(id)", "{'id':id}"],
                root.as_str(),
                9,
            ),
        ] {
            let expressions = sources
                .into_iter()
                .map(|s| jx::compile(s).unwrap())
                .collect::<Vec<_>>();
            workload(name, &expressions, input.as_bytes(), budget, smoke);
        }
        let control = jx::compile("id!='never' and id!='absent' and id!='deleted'").unwrap();
        measure_allocations("shared_input/region_control", size, smoke, Some(0), || {
            consume(control.evaluate(black_box(root.as_bytes())).unwrap())
        });
    }
    let input = format!(r#"{{"id":"00000000","data":"{}"}}"#, "x".repeat(1024 - 27));
    assert_eq!(input.len(), 1024);
    let expressions =
        ["id='never'", "id='absent'", "id='deleted'", "{'id':id}"].map(|s| jx::compile(s).unwrap());
    workload("multi_1k", &expressions, input.as_bytes(), 3, smoke);
}
