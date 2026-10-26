use super::*;
pub(super) fn run(options: &CompileOptions) {
    eprintln!("layout,{:?}", jx::execution::Program::layout_sizes());
    foundation(options);
}
fn foundation(options: &CompileOptions) {
    for (name, source, input) in [
        (
            "root24",
            projection(24, false),
            record(1024, 24, true, false),
        ),
        (
            "root40",
            projection(40, false),
            record(1024, 40, true, false),
        ),
        (
            "large80",
            projection(80, false),
            record(1048576, 80, true, false),
        ),
        (
            "call40",
            format!("($f:=function($doc){{{}}};$f($))", projection(40, false)),
            record(1024, 40, true, false),
        ),
        (
            "parameter40",
            format!(
                "($f:=function($doc){{{}}};$f($))",
                super::calls::parameter_projection(40)
            ),
            record(1024, 40, true, false),
        ),
        (
            "identity40",
            format!(
                "($f:=function($doc){{$doc}};$doc:=$f($);{})",
                super::calls::parameter_projection(40)
            ),
            record(1024, 40, true, false),
        ),
        (
            "path",
            "items.{'n':metrics.v0+metrics.v1,'s':$uppercase(name)}".into(),
            format!(
                "{{\"items\":[{}]}}",
                vec![record(500, 8, true, false); 1024].join(",")
            ),
        ),
        (
            "map",
            "$map(items,function($v){{'n':$v.metrics.v0+$v.metrics.v1,'s':$uppercase($v.name)}})"
                .into(),
            format!(
                "{{\"items\":[{}]}}",
                vec![record(500, 8, true, false); 1024].join(",")
            ),
        ),
    ] {
        let base = LIVE.load(Relaxed);
        CALLS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        PEAK.store(base, Relaxed);
        let e = options.compile(&source).unwrap();
        println!(
            "owned_compile,{name},engine,{},{},{},{}",
            CALLS.load(Relaxed),
            BYTES.load(Relaxed),
            LIVE.load(Relaxed) - base,
            PEAK.load(Relaxed) - base
        );
        drop(e);
        assert_eq!(LIVE.load(Relaxed), base);
        CALLS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        PEAK.store(base, Relaxed);
        let p = options.compile_program(&source).unwrap();
        println!(
            "owned_compile,{name},foundation,{},{},{},{}",
            CALLS.load(Relaxed),
            BYTES.load(Relaxed),
            LIVE.load(Relaxed) - base,
            PEAK.load(Relaxed) - base
        );
        let mut output = Vec::with_capacity(input.len() * 8 + 4096);
        let base = LIVE.load(Relaxed);
        let mut a = jx::execution::Acquisition::new([&p]);
        println!(
            "acquisition_setup,{name},{},{}",
            LIVE.load(Relaxed) - base,
            a.retained_bytes()
        );
        let base = LIVE.load(Relaxed);
        CALLS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        PEAK.store(base, Relaxed);
        {
            let d = a.prepare(input.as_bytes()).unwrap();
            p.try_for_each(&d, |v| {
                v.write_compact(&mut output).unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
        }
        println!(
            "scoped_execution,{name},{},{},{},{}",
            CALLS.load(Relaxed),
            BYTES.load(Relaxed),
            LIVE.load(Relaxed) - base,
            PEAK.load(Relaxed) - base
        );
        assert_eq!(LIVE.load(Relaxed), base);
    }
}
