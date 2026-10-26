use super::*;
pub(super) fn run(options: &CompileOptions) {
    for rows in [8, 128, 1024] {
        let input = format!(
            "{{\"items\":[{}]}}",
            vec![record(500, 8, true, false); rows].join(",")
        );
        for (kind, source) in [
            (
                "path",
                "items.{'x':metrics.v0+metrics.v1,'s':$uppercase(name)}",
            ),
            (
                "map",
                "$map(items,function($v){{'x':$v.metrics.v0+$v.metrics.v1,'s':$uppercase($v.name)}})",
            ),
            (
                "predicate_path",
                "items[metrics.v0>0].{'x':metrics.v0+metrics.v1,'s':$uppercase(name)}",
            ),
            (
                "grouped_path",
                "(items[metrics.v0>0]).{'x':metrics.v0+metrics.v1,'s':$uppercase(name)}",
            ),
        ] {
            let e = options.compile(source).unwrap();
            let p = options.compile_program(source).unwrap();
            let mut a = jx::execution::Acquisition::new([&p]);
            let mut output = Vec::with_capacity(1024);
            let mut modes = ["engine", "foundation"];
            if std::env::var_os("ARCH_REVERSE").is_some() {
                modes.reverse();
            }
            for mode in modes {
                let name = format!("cancel/{rows}/{kind}/{mode}");
                measure(&name, input.as_bytes(), || {
                    output.clear();
                    let result = if mode == "engine" {
                        e.evaluate(black_box(input.as_bytes()))
                            .unwrap()
                            .try_for_each(|v| {
                                v.write_compact(&mut output).unwrap();
                                Err(())
                            })
                    } else {
                        let d = a.prepare(black_box(input.as_bytes())).unwrap();
                        p.try_for_each(&d, |v| {
                            v.write_compact(&mut output).unwrap();
                            Err(())
                        })
                    };
                    assert_eq!(result, Err(jx::ConsumeError::Consumer(())));
                    black_box(&output);
                });
            }
        }
    }
    // Dense scalar widths probe the inline-frame allocation transition as well
    // as the removed capture eligibility bound. No specialized source recognizer.
    for n in [1, 2, 3, 4, 8, 12, 16, 24, 32, 40, 64, 80] {
        let source = (0..n)
            .map(|i| format!("metrics.v{i}"))
            .collect::<Vec<_>>()
            .join("+");
        let input = record(if n > 40 { 2048 } else { 1024 }, n, true, false);
        bench(&format!("numeric_width/{n}"), &[source], &input, options);
    }
    // Equivalent object projections test acquisition through a conditional join.
    // All candidate bases are present objects; a missing base changes path
    // cardinality and is deliberately outside this equivalence comparison.
    for n in [8, 24, 40] {
        let object = |offset: usize| {
            (0..n)
                .map(|i| format!("\"v{i}\":{}", i + offset))
                .collect::<Vec<_>>()
                .join(",")
        };
        for flag in [true, false] {
            let mut input = format!(
                "{{\"flag\":{flag},\"a\":{{{}}},\"b\":{{{}}},\"pad\":\"\"}}",
                object(1),
                object(2)
            );
            let target = if n > 24 { 2048 } else { 1024 };
            let pad = target - input.len();
            input = input.replace("\"pad\":\"\"", &format!("\"pad\":\"{}\"", "x".repeat(pad)));
            let project = |base: &str| {
                format!(
                    "{{{}}}",
                    (0..n)
                        .map(|i| format!("'v{i}':{base}v{i}+{base}v{i}"))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            for (form, source) in [
                (
                    "branches",
                    format!("flag ? {} : {}", project("a."), project("b.")),
                ),
                ("alias", format!("($m:=flag?a:b;{})", project("$m."))),
                ("scope", format!("(flag?a:b).{}", project(""))),
            ] {
                bench(
                    &format!("select/{n}/{flag}/{form}"),
                    &[source],
                    &input,
                    options,
                );
            }
        }
    }
    for (name, source) in [
        ("scalar", "nested.a.x*nested.a.y+nested.a.x".to_string()),
        ("projection24", projection(24, false)),
        ("projection40", projection(40, true)),
        (
            "callback",
            "$map(items,function($v){($m:=$v.metrics;{'x':$m.v0+$m.v1,'s':$uppercase($v.name)})})"
                .to_string(),
        ),
    ] {
        let mut modes = ["engine", "foundation"];
        if std::env::var_os("ARCH_REVERSE").is_some() {
            modes.reverse();
        }
        for mode in modes {
            measure(&format!("compile/{name}/{mode}"), source.as_bytes(), || {
                if mode == "engine" {
                    black_box(options.compile(black_box(&source)).unwrap());
                } else {
                    black_box(options.compile_program(black_box(&source)).unwrap());
                }
            });
        }
    }
}
