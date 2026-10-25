#![allow(unsafe_code, dead_code)]
use fixtures::{Case, Kind};
use std::{collections::HashMap, hint::black_box};
mod facade;
#[path = "facade_worker/mod.rs"]
mod facade_worker;
mod fixtures;
#[path = "foundation/mod.rs"]
mod foundation;
mod handwritten;
mod metrics;
#[path = "reference/mod.rs"]
mod reference;
mod typed;

fn vars() -> (String, HashMap<String, String>) {
    let labels = (0..4096)
        .map(|i| (format!("k{i}"), format!("label-{i}")))
        .collect::<HashMap<_, _>>();
    (
        serde_json::to_string(&serde_json::json!({"labels":labels})).unwrap(),
        labels,
    )
}
fn write(eval: jx::Evaluation<'_, '_>, out: &mut Vec<u8>) {
    let mut n = 0;
    eval.for_each(|v| {
        if n == 1 {
            out.insert(0, b'[')
        }
        if n > 0 {
            out.push(b',')
        }
        v.write_compact(&mut *out).unwrap();
        n += 1;
    })
    .unwrap();
    if n > 1 {
        out.push(b']')
    }
}
fn json(bytes: &[u8]) -> Option<serde_json::Value> {
    if bytes.is_empty() {
        None
    } else {
        Some(serde_json::from_slice(bytes).unwrap())
    }
}
fn input_bytes(case: &Case) -> f64 {
    case.inputs.iter().flatten().map(Vec::len).sum::<usize>() as f64 / case.inputs.len() as f64
}
fn core(
    case: &Case,
    mode: &str,
    phase: &str,
    options: &jx::CompileOptions,
    labels: &HashMap<String, String>,
) {
    let program = options.compile_program(&case.source).unwrap();
    let expression = options.compile(&case.source).unwrap();
    let mut acquire = jx::execution::Acquisition::new([&program]);
    let plan = jx::InputPlan::new([&expression]);
    if phase == "compile" {
        metrics::measure(&case.name, mode, phase, 0., || {
            if mode == "foundation" {
                black_box(options.compile_program(black_box(&case.source)).unwrap());
            } else {
                black_box(options.compile(black_box(&case.source)).unwrap());
            }
            0
        });
        return;
    }
    // Pure validation is tested against the same root scanner in both engine modes.
    let mut output = Vec::with_capacity(4096);
    let mut i = 0;
    let raw = jx::validate(case.inputs[0].as_ref().unwrap()).unwrap();
    if phase == "prepared" || phase == "values" {
        let prepared = acquire.prepare_validated(raw);
        let old = plan.prepare_validated(raw);
        let bytes = raw.as_bytes();
        let row = handwritten::parse(
            if case.rows == 0 && !matches!(case.kind, Kind::Fold | Kind::Filtered | Kind::Callback)
            {
                bytes
            } else {
                b"{}"
            },
            case.width.max(2),
            true,
        );
        let mut pairs = Vec::new();
        if (mode == "rust" || mode.starts_with("typed"))
            && matches!(case.kind, Kind::Fold | Kind::Filtered)
        {
            handwritten::each(bytes, false, |r| pairs.push((r.metrics[0], r.metrics[1])));
        }
        if (mode == "rust" || mode.starts_with("typed")) && case.kind == Kind::Callback {
            return;
        }
        metrics::measure(&case.name, mode, phase, input_bytes(case), || {
            output.clear();
            if mode == "foundation" {
                if phase == "values" {
                    program
                        .evaluate(&prepared)
                        .unwrap()
                        .for_each(|v| {
                            black_box(v);
                        })
                        .unwrap()
                } else {
                    program.write_json(&prepared, &mut output).unwrap();
                }
            } else if mode == "legacy" {
                if phase == "values" {
                    old.evaluate(0)
                        .unwrap()
                        .for_each(|v| {
                            black_box(v);
                        })
                        .unwrap()
                } else {
                    write(old.evaluate(0).unwrap(), &mut output)
                }
            } else {
                if matches!(case.kind, Kind::Fold | Kind::Filtered) {
                    let mut sum = 0.;
                    let mut count = 0;
                    for &(a, b) in black_box(&pairs) {
                        if case.kind == Kind::Fold || a > 0. {
                            sum += a * b + a;
                            count += 1;
                        }
                    }
                    if count > 0 {
                        handwritten::number(&mut output, sum);
                    }
                } else {
                    handwritten::render(case, black_box(&row), labels, &mut output)
                }
            }
            black_box(&output);
            output.len()
        });
        return;
    }
    metrics::measure(&case.name, mode, phase, input_bytes(case), || {
        let input = black_box(case.inputs[i % case.inputs.len()].as_ref().unwrap());
        i += 1;
        output.clear();
        if case.kind == Kind::Validate || phase == "validate" {
            if mode == "serde" || mode == "typed-serde" {
                let _: serde::de::IgnoredAny = serde_json::from_slice(input).unwrap();
            } else if mode == "typed-raw" {
                typed::validate_raw(input);
            } else {
                black_box(jx::validate(input).unwrap());
            }
        } else if mode == "rust" || mode == "serde" || mode.starts_with("typed") {
            if phase == "prepare" {
                jx::validate(input).unwrap();
                black_box(handwritten::parse(input, case.width.max(2), true));
            } else {
                if mode.starts_with("typed") {
                    if mode == "typed-raw" {
                        typed::validate_raw(input);
                    }
                    typed::run(
                        case,
                        input,
                        labels,
                        &mut output,
                        mode != "typed-serde" && mode != "typed-raw",
                        mode == "typed-vec",
                    )
                } else {
                    handwritten::run(case, input, labels, &mut output, mode == "rust")
                }
            }
        } else if mode == "foundation" {
            if phase == "validated" {
                let p = acquire.prepare_validated(raw);
                program.write_json(&p, &mut output).unwrap();
            } else {
                let p = acquire.prepare(input).unwrap();
                if phase != "prepare" {
                    program.write_json(&p, &mut output).unwrap();
                } else {
                    black_box(p.raw());
                }
            }
        } else {
            if phase == "validated" {
                let p = plan.prepare_validated(raw);
                write(p.evaluate(0).unwrap(), &mut output)
            } else {
                let p = plan.prepare(input).unwrap();
                if phase != "prepare" {
                    write(p.evaluate(0).unwrap(), &mut output)
                } else {
                    black_box(p.as_raw());
                }
            }
        }
        black_box(&output);
        output.len()
    });
}
macro_rules! worker {
    ($module:ident,$case:ident,$vars:expr,$mode:expr,$measure:expr) => {{
        use $module::jsonata::{
            Action, ErrorPolicies, EvaluationPolicy, InvalidJsonPolicy, PassPayload, Worker,
        };
        let drops = if $case.kind == Kind::Project {
            vec!["env!='production'".into()]
        } else {
            vec![]
        };
        let tombstones = if $case.kind == Kind::Project {
            vec!["deleted=true".into()]
        } else {
            vec![]
        };
        let p = $module::build_plan(
            &drops,
            &tombstones,
            false,
            Some(&$case.source),
            Some($vars),
            false,
        )
        .unwrap();
        let input = p.input_plan();
        let mut w = Worker::new(&p, &input, false, 16384);
        let policies = ErrorPolicies {
            invalid_json: InvalidJsonPolicy::Fail,
            evaluation: EvaluationPolicy::Fail,
        };
        let mut spare =
            Vec::with_capacity($case.inputs.iter().flatten().map(Vec::len).max().unwrap());
        let expected = expected_worker(&$case);
        let mut i = 0;
        let mut op = || {
            let record = &$case.inputs[i % $case.inputs.len()];
            i += 1;
            spare.clear();
            let mut source = record.as_ref().map(|b| {
                spare.extend_from_slice(b);
                std::mem::take(&mut spare)
            });
            let result = w.execute_report(&mut source, policies).unwrap();
            if i <= expected.len() {
                let (tag, bytes) = match &result.action {
                    Action::Drop => (1, &[][..]),
                    Action::Tombstone => (2, &[][..]),
                    Action::Project(b) => (3, b.as_slice()),
                    _ => panic!("unexpected worker action"),
                };
                assert_eq!(tag, expected[i - 1].0);
                assert_eq!(
                    json(bytes),
                    json(&expected[i - 1].1),
                    "worker {}",
                    $case.name
                );
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
                Action::Drop | Action::Tombstone => {
                    if let Some(b) = source {
                        spare = b;
                    }
                    0
                }
            };
            black_box(&result.issue);
            n
        };
        if $measure {
            metrics::measure(&$case.name, $mode, "worker", input_bytes(&$case), &mut op)
        } else {
            for _ in 0..$case.inputs.len() {
                op();
            }
        }
    }};
}
fn rust_worker(case: &Case, labels: &HashMap<String, String>, mode: &str, measure: bool) {
    let mut spare = Vec::with_capacity(case.inputs.iter().flatten().map(Vec::len).max().unwrap());
    let mut output = Vec::new();
    let mut i = 0;
    let mut op = || {
        let record = &case.inputs[i % case.inputs.len()];
        i += 1;
        spare.clear();
        let mut source = record.as_ref().map(|b| {
            spare.extend_from_slice(b);
            std::mem::take(&mut spare)
        });
        let Some(payload) = source.as_ref() else {
            return 0;
        };
        if mode == "typed-raw" {
            typed::validate_raw(payload);
        } else if mode == "rust" || mode.starts_with("typed") && mode != "typed-serde" {
            jx::validate(payload).unwrap();
        }
        output.clear();
        if case.kind == Kind::Project {
            let row = handwritten::parse(payload, case.width, true);
            if row.env != "production" || row.deleted {
                spare = source.take().unwrap();
                return 0;
            }
            handwritten::render(case, black_box(&row), labels, &mut output);
        } else {
            if mode.starts_with("typed") {
                typed::run(
                    case,
                    payload,
                    labels,
                    &mut output,
                    false,
                    mode == "typed-vec",
                )
            } else {
                handwritten::run(case, payload, labels, &mut output, false)
            }
        }
        let mut payload = source.take().unwrap();
        payload.clear();
        let bytes = if payload.capacity() <= 16384 {
            std::mem::replace(&mut output, payload)
        } else {
            std::mem::take(&mut output)
        };
        let n = bytes.len();
        black_box(&bytes);
        spare = bytes;
        n
    };
    if measure {
        metrics::measure(&case.name, mode, "worker", input_bytes(case), &mut op)
    } else {
        for _ in 0..case.inputs.len() {
            op();
        }
    }
}
fn check(case: &Case, options: &jx::CompileOptions, labels: &HashMap<String, String>) {
    let program = options.compile_program(&case.source).unwrap();
    let expression = options.compile(&case.source).unwrap();
    let mut acquire = jx::execution::Acquisition::new([&program]);
    for input in case.inputs.iter().flatten() {
        if case.kind == Kind::Validate {
            assert!(jx::validate(input).is_ok());
            continue;
        }
        let mut a = Vec::new();
        let mut b = Vec::new();
        let mut c = Vec::new();
        write(expression.evaluate(input).unwrap(), &mut a);
        program
            .write_json(&acquire.prepare(input).unwrap(), &mut b)
            .unwrap();
        handwritten::run(case, input, labels, &mut c, true);
        assert_eq!(json(&a), json(&b), "engine {}", case.name);
        assert_eq!(
            json(&a),
            json(&c),
            "Rust {}: {} vs {}",
            case.name,
            String::from_utf8_lossy(&a),
            String::from_utf8_lossy(&c)
        );
        assert_eq!(a.len(), c.len(), "output byte obligation {}", case.name);
        for materialize in [false, true] {
            let mut d = vec![];
            typed::run(case, input, labels, &mut d, true, materialize);
            assert_eq!(json(&a), json(&d), "typed Rust {}", case.name);
            assert_eq!(a.len(), d.len());
        }
    }
}
fn main() {
    let (vars, labels) = vars();
    let options = jx::CompileOptions::default()
        .constant_binding("vars", jx::OwnedValue::from_json(vars.as_bytes()).unwrap());
    if std::env::var_os("ABS_API").is_some() {
        facade::run(&options, &vars);
        return;
    }
    let mode = std::env::var("ABS_MODE").unwrap_or_else(|_| "foundation".into());
    let filter = std::env::var("ABS_CASE").unwrap_or_default();
    assert!(
        [
            "legacy",
            "foundation",
            "rust",
            "serde",
            "typed",
            "typed-vec",
            "typed-raw",
            "typed-serde"
        ]
        .contains(&mode.as_str()),
        "unknown ABS_MODE"
    );
    let phase = std::env::var("ABS_PHASE").unwrap_or_else(|_| "full".into());
    let cases = fixtures::cases();
    if !filter.is_empty() {
        for requested in filter.split('|') {
            assert!(
                cases.iter().any(|c| c.name == requested),
                "unknown ABS_CASE {requested}"
            );
        }
    }
    if std::env::var_os("ABS_LIST").is_some() {
        for c in &cases {
            let mut outputs = Vec::new();
            for input in &c.inputs {
                let mut out = Vec::new();
                if let Some(input) = input {
                    handwritten::run(c, input, &labels, &mut out, true);
                }
                outputs.push(out.len())
            }
            println!(
                "{}",
                serde_json::json!({"name":c.name,"input_bytes":input_bytes(c),"input_sizes":c.inputs.iter().map(|x|x.as_ref().map_or(0,Vec::len)).collect::<Vec<_>>(),"output_sizes_before_worker_policy":outputs,"width":c.width,"rows":c.rows,"source":c.source,"worker":c.worker})
            );
        }
        return;
    }
    if let Ok(n) = std::env::var("ABS_THREADS") {
        let n = n.parse::<usize>().unwrap();
        let case = cases.iter().find(|c| c.name == filter).unwrap();
        check(case, &options, &labels);
        metrics::parallel_disabled();
        let barrier = std::sync::Barrier::new(n + 1);
        let count = std::thread::scope(|s| {
            let handles = (0..n)
                .map(|_| {
                    let barrier = &barrier;
                    let mode = &mode;
                    let filter = &filter;
                    s.spawn(move || {
                        // no engine worker pool
                        // Each worker owns compiled state and private output; shared immutable input corpus is generated independently.
                        parallel_run(filter, mode, barrier)
                    })
                })
                .collect::<Vec<_>>();
            barrier.wait();
            let cpu = metrics::cpu();
            let at = std::time::Instant::now();
            let count = handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .sum::<usize>();
            (count, at.elapsed().as_secs_f64(), metrics::cpu() - cpu)
        });
        println!(
            "parallel,{filter},{mode},{n},{},{},{},{}",
            count.0,
            count.1,
            count.2,
            input_bytes(case)
        );
        return;
    }
    println!(
        "case,mode,phase,sample,records,wall_seconds,cpu_seconds,input_bytes,output_bytes,allocations,allocated_bytes,peak_heap,probe_output_bytes"
    );
    for case in cases
        .iter()
        .filter(|c| filter.is_empty() || filter.split('|').any(|f| c.name == f))
    {
        check(case, &options, &labels);
        if std::env::var_os("ABS_CHECK").is_some() {
            eprintln!("checked,{}", case.name);
            continue;
        }
        if case.worker {
            if phase == "borrowed-worker" {
                borrowed_worker(case, &mode, &options, &labels);
                continue;
            }
            match mode.as_str() {
                "legacy" => worker!(reference, case, &vars, &mode, true),
                "foundation" => worker!(foundation, case, &vars, &mode, true),
                _ => rust_worker(case, &labels, &mode, true),
            }
        } else {
            core(case, &mode, &phase, &options, &labels)
        }
    }
}
fn api_worker(case: &Case, vars: &str, mode: &str) {
    match mode {
        "legacy" => worker!(reference, case, vars, mode, true),
        "prepared" => worker!(foundation, case, vars, mode, true),
        "facade" => worker!(facade_worker, case, vars, mode, true),
        _ => panic!("worker API mode: {mode}"),
    }
}
fn parallel_run(filter: &str, mode: &str, barrier: &std::sync::Barrier) -> usize {
    let (vars, labels) = vars();
    let options = jx::CompileOptions::default()
        .constant_binding("vars", jx::OwnedValue::from_json(vars.as_bytes()).unwrap());
    let case = fixtures::cases()
        .into_iter()
        .find(|c| c.name == filter)
        .unwrap();
    let program = options.compile_program(&case.source).unwrap();
    let expression = options.compile(&case.source).unwrap();
    let mut acquire = jx::execution::Acquisition::new([&program]);
    let plan = jx::InputPlan::new([&expression]);
    let mut output = Vec::with_capacity(4096);
    let duration = std::env::var("ABS_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1500);
    barrier.wait();
    let at = std::time::Instant::now();
    let mut n = 0;
    while at.elapsed().as_millis() < (duration as u128) {
        for _ in 0..32 {
            let input = black_box(case.inputs[n % case.inputs.len()].as_ref().unwrap());
            output.clear();
            match mode {
                "legacy" => write(
                    plan.prepare(input).unwrap().evaluate(0).unwrap(),
                    &mut output,
                ),
                "foundation" => {
                    program
                        .write_json(&acquire.prepare(input).unwrap(), &mut output)
                        .unwrap();
                }
                m if m.starts_with("typed") => {
                    typed::run(&case, input, &labels, &mut output, true, false)
                }
                _ => handwritten::run(&case, input, &labels, &mut output, true),
            }
            black_box(&output);
            n += 1;
        }
    }
    n
}

fn expected_worker(case: &Case) -> Vec<(u8, Vec<u8>)> {
    let (_, labels) = vars();
    case.inputs
        .iter()
        .map(|input| {
            let Some(input) = input else {
                return (2, vec![]);
            };
            if case.kind == Kind::Project {
                let r = handwritten::parse(input, case.width, true);
                if r.env != "production" {
                    return (1, vec![]);
                }
                if r.deleted {
                    return (2, vec![]);
                }
            }
            let mut out = vec![];
            handwritten::run(case, input, &labels, &mut out, true);
            (3, out)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_outputs_and_worker_policies() {
        let (vars, labels) = vars();
        let options = jx::CompileOptions::default()
            .constant_binding("vars", jx::OwnedValue::from_json(vars.as_bytes()).unwrap());
        for case in fixtures::cases() {
            if case.name.starts_with("worker/")
                || [
                    "sequence/0/48/50/group",
                    "sequence/1/48/50/group",
                    "nested/8",
                    "project/1024/40/direct",
                    "predicate/1024",
                ]
                .contains(&case.name.as_str())
            {
                check(&case, &options, &labels);
                if case.worker {
                    let mode = "test";
                    worker!(reference, case, &vars, mode, false);
                    worker!(foundation, case, &vars, mode, false);
                }
            }
        }
    }
    #[test]
    fn strict_validation_precedes_selective_parsing() {
        for bad in [
            b"{\"id\":\"k1\",\"unused\":[1,]}".as_slice(),
            b"{\"unused\":\"\xff\"}",
            b"{} false",
        ] {
            assert!(jx::validate(bad).is_err());
        }
        let row = handwritten::parse(br#"{"name":"Ad\u0061","metrics":{"v0":1,"v1":2}}"#, 2, true);
        assert_eq!(row.name, "Ada");
        assert_eq!(row.metrics[1], 2.);
    }
}

// Public-API boundary control: identical worker expressions/corpus/policy order,
// but borrowed input and reused output. Ownership/copying is deliberately excluded.
fn borrowed_worker(
    case: &Case,
    mode: &str,
    options: &jx::CompileOptions,
    labels: &HashMap<String, String>,
) {
    assert!(
        ["legacy", "foundation", "typed"].contains(&mode),
        "borrowed-worker mode must use legacy/foundation/typed"
    );
    let mut sources = Vec::new();
    if case.kind == Kind::Project {
        sources.extend(["env!='production'".to_owned(), "deleted=true".to_owned()]);
    }
    sources.push(case.source.clone());
    let last = sources.len() - 1;
    let programs = sources
        .iter()
        .map(|s| options.compile_program(s).unwrap())
        .collect::<Vec<_>>();
    let expressions = sources
        .iter()
        .map(|s| options.compile(s).unwrap())
        .collect::<Vec<_>>();
    let mut acquire = jx::execution::Acquisition::new(programs.iter());
    let plan = jx::InputPlan::new(expressions.iter());
    let expected = expected_worker(case);
    let mut output = Vec::with_capacity(4096);
    let mut i = 0;
    metrics::measure(
        &case.name,
        mode,
        "borrowed-worker",
        input_bytes(case),
        || {
            let record = &case.inputs[i % case.inputs.len()];
            i += 1;
            output.clear();
            let mut action = 2;
            if let Some(input) = record {
                action = 3;
                if mode == "foundation" {
                    let p = acquire.prepare(input).unwrap();
                    for (n, program) in programs[..last].iter().enumerate() {
                        if program
                            .evaluate(&p)
                            .unwrap()
                            .single()
                            .unwrap()
                            .unwrap()
                            .as_bool()
                            .unwrap()
                        {
                            action = if n == 0 { 1 } else { 2 };
                            break;
                        }
                    }
                    if action == 3 {
                        programs[last].write_json(&p, &mut output).unwrap();
                    }
                } else if mode == "legacy" {
                    let p = plan.prepare(input).unwrap();
                    for n in 0..last {
                        if p.evaluate(n)
                            .unwrap()
                            .single()
                            .unwrap()
                            .unwrap()
                            .as_bool()
                            .unwrap()
                        {
                            action = if n == 0 { 1 } else { 2 };
                            break;
                        }
                    }
                    if action == 3 {
                        write(p.evaluate(last).unwrap(), &mut output);
                    }
                } else {
                    jx::validate(input).unwrap();
                    if case.kind == Kind::Project {
                        let row = handwritten::parse(input, case.width, true);
                        if row.env != "production" {
                            action = 1
                        } else if row.deleted {
                            action = 2
                        } else {
                            handwritten::render(case, &row, labels, &mut output)
                        }
                    } else {
                        typed::run(case, input, labels, &mut output, false, false)
                    }
                }
            }
            if i <= expected.len() {
                assert_eq!(action, expected[i - 1].0);
                assert_eq!(json(&output), json(&expected[i - 1].1));
            }
            black_box(action);
            black_box(&output);
            output.len()
        },
    );
}
