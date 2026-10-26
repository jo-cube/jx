//! Default-feature public API and actual-worker controls. No backend-selection API.
#![allow(unsafe_code, dead_code)]
mod fixtures;
mod handwritten;
mod metrics;
#[path = "reference/mod.rs"]
mod reference;
use fixtures::{Case, Kind};
use std::{collections::HashMap, hint::black_box};
fn run(case: &Case, vars: &str, labels: &HashMap<String, String>) {
    use reference::jsonata::{
        Action, ErrorPolicies, EvaluationPolicy, InvalidJsonPolicy, PassPayload, Worker,
    };
    let drops = if case.kind == Kind::Project {
        vec!["env!='production'".into()]
    } else {
        vec![]
    };
    let tombstones = if case.kind == Kind::Project {
        vec!["deleted=true".into()]
    } else {
        vec![]
    };
    let plan = reference::build_plan(
        &drops,
        &tombstones,
        false,
        Some(&case.source),
        Some(vars),
        false,
    )
    .unwrap();
    let input = plan.input_plan();
    let mut worker = Worker::new(&plan, &input, false, 16384);
    let policies = ErrorPolicies {
        invalid_json: InvalidJsonPolicy::Fail,
        evaluation: EvaluationPolicy::Fail,
    };
    let expected = case
        .inputs
        .iter()
        .map(|input| {
            let Some(input) = input else {
                return (2, vec![]);
            };
            if case.kind == Kind::Project {
                let row = handwritten::parse(input, case.width, true);
                if row.env != "production" {
                    return (1, vec![]);
                }
                if row.deleted {
                    return (2, vec![]);
                }
            }
            let mut output = vec![];
            handwritten::run(case, input, labels, &mut output, true);
            (3, output)
        })
        .collect::<Vec<_>>();
    let mut spare = Vec::with_capacity(case.inputs.iter().flatten().map(Vec::len).max().unwrap());
    let mut i = 0;
    metrics::measure(
        &case.name,
        "public",
        "worker",
        case.inputs.iter().flatten().map(Vec::len).sum::<usize>() as f64 / case.inputs.len() as f64,
        || {
            let record = &case.inputs[i % case.inputs.len()];
            i += 1;
            spare.clear();
            let mut source = record.as_ref().map(|b| {
                spare.extend_from_slice(b);
                std::mem::take(&mut spare)
            });
            let result = worker.execute_report(&mut source, policies).unwrap();
            if i <= expected.len() {
                let (tag, bytes) = match &result.action {
                    Action::Drop => (1, &[][..]),
                    Action::Tombstone => (2, &[][..]),
                    Action::Project(b) => (3, b.as_slice()),
                    _ => panic!("action"),
                };
                assert_eq!(tag, expected[i - 1].0);
                let json = |b: &[u8]| {
                    if b.is_empty() {
                        None
                    } else {
                        Some(serde_json::from_slice::<serde_json::Value>(b).unwrap())
                    }
                };
                assert_eq!(json(bytes), json(&expected[i - 1].1));
            }
            let n = match result.action {
                Action::Project(bytes)
                | Action::PassThrough(PassPayload::Exact(bytes))
                | Action::PassThrough(PassPayload::Json { bytes, .. }) => {
                    let n = bytes.len();
                    black_box(&bytes);
                    spare = bytes;
                    n
                }
                _ => {
                    if let Some(bytes) = source {
                        spare = bytes
                    }
                    0
                }
            };
            black_box(&result.issue);
            n
        },
    );
}
fn expression(case: &Case, vars: &str) {
    let options = jx::CompileOptions::default()
        .constant_binding("vars", jx::OwnedValue::from_json(vars.as_bytes()).unwrap());
    let expression = options.compile(&case.source).unwrap();
    let mut output = Vec::with_capacity(4096);
    let mut i = 0;
    let phase = std::env::var("ABS_PHASE").unwrap_or_else(|_| "full".into());
    if phase == "compile" {
        metrics::measure(&case.name, "public", "compile", 0.0, || {
            black_box(options.compile(black_box(&case.source)).unwrap());
            0
        });
        return;
    }
    metrics::measure_cycle(
        &case.name,
        "public",
        "full",
        case.inputs.iter().flatten().map(Vec::len).sum::<usize>() as f64 / case.inputs.len() as f64,
        case.inputs.len(),
        || {
            let input = case.inputs[i % case.inputs.len()].as_ref().unwrap();
            i += 1;
            output.clear();
            if case.kind == Kind::Validate {
                black_box(jx::validate(black_box(input)).unwrap());
            } else {
                expression
                    .evaluate(black_box(input))
                    .unwrap()
                    .for_each(|value| {
                        value.write_compact(&mut output).unwrap();
                        output.push(b'\n');
                    })
                    .unwrap();
            }
            black_box(&output);
            output.len()
        },
    );
}
fn main() {
    let labels = (0..4096)
        .map(|i| (format!("k{i}"), format!("label-{i}")))
        .collect::<HashMap<_, _>>();
    let vars = serde_json::to_string(&serde_json::json!({"labels":labels})).unwrap();
    println!(
        "case,mode,phase,sample,records,wall_seconds,cpu_seconds,input_bytes,output_bytes,allocations,allocated_bytes,peak_heap,probe_output_bytes"
    );
    expression(
        &Case {
            name: "field/500".into(),
            kind: Kind::Scalar,
            source: "id".into(),
            width: 0,
            rows: 0,
            keys: vec![],
            inputs: vec![Some(fixtures::record(500, 8, 0, false))],
            worker: false,
        },
        &vars,
    );
    for case in fixtures::cases() {
        if case.worker {
            run(&case, &vars, &labels)
        } else {
            expression(&case, &vars)
        }
    }
}
