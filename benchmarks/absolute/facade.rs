//! Matched public API boundaries. All core modes use the same cancellable serializer.
use super::*;

pub fn run(options: &jx::CompileOptions, vars: &str) {
    eprintln!(
        "layout,{}",
        serde_json::json!({"instruction_cell":jx::execution::Program::layout_sizes(),"workspace":std::mem::size_of::<jx::execution::Workspace>(),"acquisition":std::mem::size_of::<jx::execution::Acquisition>(),"program":std::mem::size_of::<jx::execution::Program>(),"expression":std::mem::size_of::<jx::Expression>()})
    );
    let mode = std::env::var("ABS_MODE").unwrap_or_else(|_| "facade".into());
    let phase = std::env::var("ABS_PHASE").unwrap_or_else(|_| "full".into());
    let filter = std::env::var("ABS_CASE").unwrap_or_default();
    let mut cases = fixtures::cases();
    for (name, source, input) in [
        (
            "field/500",
            "metrics.v0",
            fixtures::record(500, 8, 1, false),
        ),
        (
            "eval/literal",
            "$eval('metrics.v0+metrics.v1')",
            fixtures::record(500, 8, 1, false),
        ),
        (
            "eval/dynamic",
            "$eval(source)",
            br#"{"source":"a+b","a":1,"b":2}"#.to_vec(),
        ),
        (
            "context/128",
            "groups.items.{'v':v,'p':%.name}",
            format!(
                "{{\"groups\":[{{\"name\":\"parent\",\"items\":[{}]}}]}}",
                vec!["{\"v\":1}"; 128].join(",")
            )
            .into_bytes(),
        ),
    ] {
        cases.push(Case {
            name: name.into(),
            source: source.into(),
            inputs: vec![Some(input)],
            kind: Kind::Scalar,
            keys: vec![],
            width: 0,
            rows: 0,
            worker: false,
        });
    }
    println!(
        "case,mode,phase,sample,records,wall_seconds,cpu_seconds,input_bytes,output_bytes,allocations,allocated_bytes,peak_bytes,probe_output_bytes"
    );
    for c in cases
        .iter()
        .filter(|c| filter.is_empty() || filter.split('|').any(|n| n == c.name))
    {
        if c.worker {
            super::api_worker(c, vars, &mode);
            continue;
        }
        let e = options.compile_execution(&c.source).unwrap();
        if let Ok(threads) = std::env::var("ABS_API_THREADS") {
            parallel(c, &e, threads.parse().unwrap(), &mode);
            continue;
        }
        let old = options.compile(&c.source).unwrap();
        let p = e.execution_program().unwrap();
        let plan = jx::InputPlan::new([&e]);
        let mut workspace = jx::execution::Workspace::default();
        let mut acquisition = jx::execution::Acquisition::new([p]);
        let mut output = Vec::with_capacity(4096);
        let raw = c
            .inputs
            .iter()
            .map(|b| jx::validate(b.as_ref().unwrap()).unwrap())
            .collect::<Vec<_>>();
        let mut at = 0;
        let evaluate = |index: usize,
                        workspace: &mut jx::execution::Workspace,
                        acquisition: &mut jx::execution::Acquisition,
                        output: &mut Vec<u8>| {
            let input = c.inputs[index].as_ref().unwrap();
            let eval = match mode.as_str() {
                "legacy" => {
                    if phase == "validated" {
                        old.evaluate_validated(raw[index])
                    } else {
                        old.evaluate(input)
                    }
                }
                "scan" => p.evaluate_scan(if phase == "validated" {
                    raw[index]
                } else {
                    jx::validate(input).unwrap()
                }),
                "program" => {
                    if phase == "validated" {
                        p.evaluate_validated(raw[index])
                    } else {
                        p.evaluate_bytes(input)
                    }
                }
                "focus" => e.evaluate_with(
                    Some(input),
                    jx::EvaluationOptions {
                        focus: Some(jx::Value::Raw(raw[index])),
                        ..Default::default()
                    },
                ),
                "facade" => {
                    if phase == "validated" {
                        e.evaluate_validated(raw[index])
                    } else {
                        e.evaluate(input)
                    }
                }
                "workspace" => {
                    if phase == "validated" {
                        e.evaluate_validated_with_workspace(
                            Some(raw[index]),
                            Default::default(),
                            workspace,
                        )
                    } else {
                        e.evaluate_with_workspace(Some(input), Default::default(), workspace)
                    }
                }
                "prepared" => {
                    let d = if phase == "validated" {
                        acquisition.prepare_validated(raw[index])
                    } else {
                        acquisition.prepare(input).unwrap()
                    };
                    p.evaluate(&d)
                }
                "shared" => {
                    let d = if phase == "validated" {
                        plan.prepare_validated(raw[index])
                    } else {
                        plan.prepare(input).unwrap()
                    };
                    d.evaluate(0)
                }
                _ => panic!("API mode {mode}"),
            }
            .unwrap();
            output.clear();
            super::write(eval, output);
        };
        // Check every input, not just the warmed representative.
        for (i, input) in c.inputs.iter().enumerate() {
            let mut expected = Vec::new();
            super::write(
                old.evaluate(input.as_ref().unwrap()).unwrap(),
                &mut expected,
            );
            evaluate(i, &mut workspace, &mut acquisition, &mut output);
            assert_eq!(json(&output), json(&expected), "{} {mode}", c.name);
        }
        if phase == "compile" {
            metrics::measure(&c.name, &mode, &phase, 0., || {
                if mode == "legacy" {
                    black_box(options.compile(&c.source).unwrap());
                } else if mode == "program" {
                    black_box(options.compile_program(&c.source).unwrap());
                } else {
                    black_box(options.compile_execution(&c.source).unwrap());
                }
                0
            });
        } else {
            metrics::measure(&c.name, &mode, &phase, input_bytes(c), || {
                let index = at % c.inputs.len();
                at += 1;
                evaluate(index, &mut workspace, &mut acquisition, &mut output);
                black_box(&output);
                output.len()
            });
        }
        eprintln!("workspace,{},{}", c.name, workspace.retained_bytes());
    }
}

// Concurrent callers share the same immutable Expression, not recompilations or
// per-thread program clones. Only offsets, consumers and result graphs are local.
fn parallel(c: &Case, e: &jx::Expression, threads: usize, mode: &str) {
    assert!(threads > 0);
    assert!(["facade", "workspace"].contains(&mode));
    let ms = std::env::var("ABS_MS")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(500);
    metrics::parallel_disabled();
    let barrier = std::sync::Barrier::new(threads + 1);
    let (records, wall, cpu) = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..threads {
            let barrier = &barrier;
            handles.push(scope.spawn(move || {
                let mut workspace = jx::execution::Workspace::default();
                let mut output = Vec::with_capacity(4096);
                let mut n = 0;
                let mut evaluate = |n: usize| {
                    let input = c.inputs[n % c.inputs.len()].as_ref().unwrap();
                    let eval = if mode == "facade" {
                        e.evaluate(input)
                    } else {
                        e.evaluate_with_workspace(Some(input), Default::default(), &mut workspace)
                    }
                    .unwrap();
                    output.clear();
                    super::write(eval, &mut output);
                    black_box(&output);
                };
                for i in 0..64 {
                    evaluate(i);
                }
                barrier.wait();
                let start = std::time::Instant::now();
                while start.elapsed() < std::time::Duration::from_millis(ms) {
                    for _ in 0..32 {
                        evaluate(n);
                        n += 1;
                    }
                }
                n
            }));
        }
        barrier.wait();
        let start = std::time::Instant::now();
        let cpu = metrics::cpu();
        let n = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .sum::<usize>();
        (n, start.elapsed().as_secs_f64(), metrics::cpu() - cpu)
    });
    println!(
        "{}",
        serde_json::json!({"case":c.name,"mode":mode,"threads":threads,"records":records,"wall_seconds":wall,"cpu_seconds":cpu,"records_per_second":records as f64/wall,"input_bytes_per_second":input_bytes(c)*records as f64/wall})
    );
}
