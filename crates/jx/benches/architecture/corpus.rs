use super::*;
// Fixture DOM work occurs before measurement. Engine evaluations only borrow bytes.
pub(super) fn run(options: &CompileOptions) {
    let records = (0..64)
        .map(|i| {
            let mut v: serde_json::Value =
                serde_json::from_str(&record(1024, 24, i % 2 == 0, false)).unwrap();
            if i % 3 == 0 {
                v["metrics"]
                    .as_object_mut()
                    .unwrap()
                    .remove(&format!("v{}", i % 24));
            }
            v["name"] = serde_json::json!(if i % 4 == 0 {
                "é😀Ada"
            } else if i % 4 == 1 {
                "quote\"\\Ada"
            } else {
                "Ada"
            });
            v["id"] = serde_json::json!(if i % 5 == 0 { "unknown" } else { "k17" });
            let mut fields = v.as_object().unwrap().iter().collect::<Vec<_>>();
            let len = fields.len();
            fields.rotate_left(i % len);
            if i % 2 == 1 {
                fields.reverse();
            }
            format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(k, v)| format!("{}:{v}", serde_json::to_string(k).unwrap()))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .collect::<Vec<_>>();
    let representative = vec![0; records.iter().map(String::len).sum::<usize>() / records.len()];
    for alias in [false, true] {
        let expr = options.compile(&projection(24, alias)).unwrap();
        let program = options.compile_program(&projection(24, alias)).unwrap();
        let facade = options.compile(&projection(24, alias)).unwrap();
        let facade_plan = InputPlan::new([&facade]);
        let mut workspace = jx::execution::Workspace::default();
        let mut acquisition = jx::execution::Acquisition::new([&program]);
        let plan = InputPlan::new([&expr]);

        for row in &records {
            let reference = expr
                .evaluate(row.as_bytes())
                .unwrap()
                .collect_owned()
                .unwrap();
            let d = acquisition.prepare(row.as_bytes()).unwrap();
            let mut actual = Vec::new();
            program
                .try_for_each(&d, |v| {
                    actual.push(v.to_owned().unwrap());
                    Ok::<_, ()>(())
                })
                .unwrap();
            assert_eq!(format!("{reference:?}"), format!("{actual:?}"));
        }
        let mut output = Vec::with_capacity(16384);
        let mut at = 0;
        let modes = if std::env::var_os("ARCH_REVERSE").is_some() {
            [
                "shared",
                "workspace",
                "facade",
                "foundation",
                "vm_raw",
                "engine",
            ]
        } else {
            [
                "engine",
                "vm_raw",
                "foundation",
                "facade",
                "workspace",
                "shared",
            ]
        };
        for mode in modes {
            let name = format!("corpus/{}", if alias { "alias" } else { "direct" });
            measure(&format!("{name}/{mode}"), &representative, || {
                let input = black_box(records[at % records.len()].as_bytes());
                at += 1;
                let result = match mode {
                    "engine" => plan
                        .prepare(input)
                        .unwrap()
                        .evaluate(0)
                        .and_then(|e| write(e, &mut output)),
                    "facade" => facade.evaluate(input).and_then(|e| write(e, &mut output)),
                    "workspace" => facade
                        .evaluate_with_workspace(Some(input), Default::default(), &mut workspace)
                        .and_then(|e| write(e, &mut output)),
                    "shared" => facade_plan
                        .prepare(input)
                        .unwrap()
                        .evaluate(0)
                        .and_then(|e| write(e, &mut output)),
                    "vm_raw" => program
                        .evaluate_scan(jx::validate(input).unwrap())
                        .and_then(|e| write(e, &mut output)),
                    "foundation" => {
                        let d = acquisition.prepare(input).unwrap();
                        output.clear();
                        program
                            .try_for_each(&d, |v| {
                                v.write_compact(&mut output).unwrap();
                                Ok::<_, ()>(())
                            })
                            .map_err(|e| match e {
                                jx::ConsumeError::Evaluation(e) => e,
                                _ => unreachable!(),
                            })
                    }
                    _ => unreachable!(),
                };
                result.unwrap();
            });
        }
    }
}
