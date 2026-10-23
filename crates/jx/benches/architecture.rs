#![allow(unsafe_code)]
use jx::{CompileOptions, Evaluation, InputPlan, OwnedValue};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed},
    time::{Duration, Instant},
};
struct Allocator;
static ENABLED: AtomicBool = AtomicBool::new(true);
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicI64 = AtomicI64::new(0);
static PEAK: AtomicI64 = AtomicI64::new(0);
fn added(n: usize) {
    CALLS.fetch_add(1, Relaxed);
    BYTES.fetch_add(n as u64, Relaxed);
    let live = LIVE.fetch_add(n as i64, Relaxed) + n as i64;
    PEAK.fetch_max(live, Relaxed);
}
// SAFETY: instrumentation forwards unchanged allocations to System. No engine unsafe.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() && ENABLED.load(Relaxed) {
            added(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if ENABLED.load(Relaxed) {
            LIVE.fetch_sub(l.size() as i64, Relaxed);
        }
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let p = unsafe { System.realloc(p, l, n) };
        if !p.is_null() && ENABLED.load(Relaxed) {
            LIVE.fetch_sub(l.size() as i64, Relaxed);
            added(n);
        }
        p
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn write(e: Evaluation<'_, '_>, output: &mut Vec<u8>) -> Result<(), jx::Error> {
    output.clear();
    e.for_each(|v| v.write_compact(&mut *output).unwrap())?;
    black_box(&*output);
    Ok(())
}
fn measure(name: &str, input: &[u8], mut operation: impl FnMut()) {
    if let Ok(filter) = std::env::var("ARCH_FILTER")
        && !name.contains(&filter)
    {
        return;
    }
    if let Ok(modes) = std::env::var("ARCH_MODES")
        && !modes.split(',').any(|m| name.ends_with(&format!("/{m}")))
    {
        return;
    }
    for _ in 0..if input.len() > 64 * 1024 { 4 } else { 128 } {
        operation();
    }
    let sample_ms = std::env::var("ARCH_MS")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(200);
    let samples = std::env::var("ARCH_SAMPLES")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(7);
    let base = LIVE.load(Relaxed);
    CALLS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    PEAK.store(base, Relaxed);
    operation();
    let calls = CALLS.load(Relaxed);
    let bytes = BYTES.load(Relaxed);
    let peak = PEAK.load(Relaxed) - base;
    assert_eq!(LIVE.load(Relaxed), base, "per-record residual {name}");
    if std::env::var_os("ARCH_PROFILE").is_some() {
        eprintln!("measuring,{name}");
    }
    for sample in 0..samples {
        let counting = std::env::var_os("ARCH_ORDINARY").is_none();
        CALLS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        PEAK.store(base, Relaxed);
        ENABLED.store(counting, Relaxed);
        let start = Instant::now();
        let mut n = 0;
        loop {
            let batch = if input.len() > 256 * 1024 {
                1
            } else if input.len() > 64 * 1024 {
                8
            } else {
                32
            };
            for _ in 0..batch {
                operation();
            }
            n += batch;
            if start.elapsed() >= Duration::from_millis(sample_ms) {
                break;
            }
        }
        let seconds = start.elapsed().as_secs_f64();
        ENABLED.store(true, Relaxed);
        let (calls, bytes, peak) = if counting {
            (
                CALLS.load(Relaxed) as f64 / n as f64,
                BYTES.load(Relaxed) as f64 / n as f64,
                PEAK.load(Relaxed) - base,
            )
        } else {
            (calls as f64, bytes as f64, peak)
        };
        assert_eq!(LIVE.load(Relaxed), base, "sample residual {name}");
        println!(
            "{name},{},{sample},{n},{seconds:.9},{:.0},{calls},{bytes},{peak}",
            input.len(),
            n as f64 / seconds
        );
    }
}
fn record(size: usize, n: usize, late: bool, escaped: bool) -> String {
    let metrics = (0..n)
        .map(|i| format!("\"v{i}\":{}", i + 1))
        .collect::<Vec<_>>()
        .join(",");
    let fields = format!(
        r#""id":"k17","name":"{}","env":"production","deleted":false,"metrics":{{{metrics}}},"profile":{{"region":"eu","tag":"sensor"}},"nested":{{"a":{{"x":7,"y":3}}}}"#,
        if escaped { r#"Ad\u0061"# } else { "Ada" }
    );
    let mut filler = String::new();
    let mut i = 0;
    while fields.len() + filler.len() + 60 < size {
        filler.push_str(&format!(r#""unused{i}":{{"a":{i},"b":true}},"#));
        i += 1;
    }
    let mut record = if late {
        format!("{{{filler}{fields},\"pad\":\"\"}}")
    } else {
        format!("{{{fields},{filler}\"pad\":\"\"}}")
    };
    let pad = size
        .checked_sub(record.len())
        .unwrap_or_else(|| panic!("size {size} fields {n} record {}", record.len()));
    record = record.replace("\"pad\":\"\"", &format!("\"pad\":\"{}\"", "x".repeat(pad)));
    assert_eq!(record.len(), size);
    record
}
fn projection(n: usize, alias: bool) -> String {
    let mut entries = (0..n)
        .map(|i| {
            format!(
                "'n{i}':{}v{i}+{}v{i}",
                if alias { "$m." } else { "metrics." },
                if alias { "$m." } else { "metrics." }
            )
        })
        .collect::<Vec<_>>();
    entries.extend([
        "'name':$uppercase(name)".into(),
        "'valid':$contains(name,'a') and env='production'".into(),
        "'label':$lookup($vars.labels,id)".into(),
        "'nested':nested.a.x*nested.a.y".into(),
        "'region':profile.region".into(),
    ]);
    let obj = format!("{{{}}}", entries.join(","));
    if alias {
        format!("($m:=metrics;{obj})")
    } else {
        obj
    }
}
fn bench(name: &str, sources: &[String], input: &str, options: &CompileOptions) {
    if std::env::var("ARCH_FILTER").is_ok_and(|filter| !name.contains(&filter)) {
        return;
    }
    let expressions = sources
        .iter()
        .map(|s| options.compile(s).unwrap())
        .collect::<Vec<_>>();
    let programs = sources
        .iter()
        .map(|s| options.compile_program(s).unwrap())
        .collect::<Vec<_>>();
    let plan = InputPlan::new(expressions.iter());
    let expected = expressions
        .iter()
        .map(|e| e.evaluate(input.as_bytes()).and_then(|v| v.collect_owned()))
        .collect::<Vec<_>>();
    for p in &programs {
        let index = programs.iter().position(|v| std::ptr::eq(v, p)).unwrap();
        let actual = p
            .evaluate_bytes(input.as_bytes())
            .and_then(|v| v.collect_owned());
        assert_eq!(
            format!("{expected:?}"),
            format!(
                "{:?}",
                expected
                    .iter()
                    .enumerate()
                    .map(|(i, v)| if i == index {
                        actual.clone()
                    } else {
                        v.clone()
                    })
                    .collect::<Vec<_>>()
            ),
            "{name} raw"
        );
    }
    let mut output = Vec::with_capacity(input.len() * 8 + 4096);
    let mut modes = vec![("engine", ()), ("vm_raw", ()), ("foundation", ())];
    if std::env::var_os("ARCH_REVERSE").is_some() {
        modes.reverse();
    }
    for (label, _) in modes {
        if label == "engine" {
            measure(&format!("{name}/engine"), input.as_bytes(), || {
                let prepared = plan.prepare(black_box(input.as_bytes())).unwrap();
                for i in 0..expressions.len() {
                    let _ = black_box(prepared.evaluate(i).and_then(|r| write(r, &mut output)));
                }
            });
            continue;
        }
        if label == "foundation" {
            let mut acquisition = jx::execution::Acquisition::new(programs.iter());
            {
                let doc = acquisition.prepare(input.as_bytes()).unwrap();
                for (p, expected) in programs.iter().zip(&expected) {
                    let mut values = Vec::new();
                    let result = p.try_for_each(&doc, |v| {
                        values.push(v.to_owned().unwrap());
                        Ok::<_, ()>(())
                    });
                    let actual = match result {
                        Ok(()) => Ok(values),
                        Err(jx::ConsumeError::Evaluation(e)) => Err(e),
                        Err(jx::ConsumeError::Consumer(())) => unreachable!(),
                    };
                    assert_eq!(
                        format!("{expected:?}"),
                        format!("{actual:?}"),
                        "{name} foundation"
                    );
                }
            }
            eprintln!(
                "retained,{name},foundation,{}",
                acquisition.retained_bytes()
            );
            measure(&format!("{name}/foundation"), input.as_bytes(), || {
                let doc = acquisition.prepare(black_box(input.as_bytes())).unwrap();
                for p in &programs {
                    output.clear();
                    let result = p.try_for_each(&doc, |v| {
                        v.write_compact(&mut output).unwrap();
                        Ok::<_, ()>(())
                    });
                    let _ = black_box(result);
                    black_box(&output);
                }
            });
            continue;
        }
        if label == "vm_raw" {
            measure(&format!("{name}/vm_raw"), input.as_bytes(), || {
                let raw = jx::validate(black_box(input.as_bytes())).unwrap();
                for p in &programs {
                    let _ = black_box(
                        p.evaluate_validated(raw)
                            .and_then(|r| write(r, &mut output)),
                    );
                }
            });
            continue;
        }
    }
}
#[path = "architecture/calls.rs"]
mod calls;
#[path = "architecture/corpus.rs"]
mod corpus;
#[path = "architecture/foundation.rs"]
mod foundation;
#[path = "architecture/memory.rs"]
mod memory;
#[path = "architecture/scopes.rs"]
mod scopes;
#[path = "architecture/sequences.rs"]
mod sequences;
fn main() {
    println!(
        "workload,input_bytes,sample,records,seconds,records_per_second,allocations,requested_bytes,peak_live_bytes"
    );
    let labels = (0..4096)
        .map(|i| {
            (
                format!("k{i}"),
                serde_json::Value::String(format!("label-{i}")),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let vars = OwnedValue::from_json(
        serde_json::to_string(&serde_json::json!({"labels":labels}))
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    let options = CompileOptions::default().constant_binding("vars", vars);
    if std::env::var_os("ARCH_MEMORY").is_some() {
        memory::run(&options);
        return;
    }
    if std::env::var_os("ARCH_EXPANDED").is_some()
        || std::env::var("ARCH_FILTER").is_ok_and(|s| s.starts_with("delivery/"))
    {
        calls::run(&options);
    }
    if std::env::var_os("ARCH_CONVERGENCE").is_some() {
        sequences::convergence(&options);
    }
    if std::env::var("ARCH_FILTER").is_ok_and(|s| s.starts_with("pipelines/")) {
        sequences::pipelines(&options);
        return;
    }
    sequences::run(&options);
    scopes::run(&options);
    foundation::run(&options);
    corpus::run(&options);
    for (size, n) in [(500, 8), (1024, 24), (1024, 40), (10240, 80), (1048576, 80)] {
        for (dist, late, escaped) in [
            ("late", true, false),
            ("early", false, false),
            ("escaped", true, true),
        ] {
            // Keep the full matrix opt-in at 1 MiB to avoid an excessive smoke run.
            if size == 1048576 && dist != "late" {
                continue;
            }
            let input = record(size, n, late, escaped);
            for alias in [false, true] {
                let form = if alias { "alias" } else { "direct" };
                bench(
                    &format!("project/{size}/{n}/{dist}/{form}"),
                    &[projection(n, alias)],
                    &input,
                    &options,
                );
            }
            if dist == "late" {
                bench(
                    &format!("multi/{size}/{n}"),
                    &[
                        "deleted".into(),
                        "env!='production'".into(),
                        projection(n, false),
                    ],
                    &input,
                    &options,
                );
                bench(
                    &format!("scalar/{size}/{n}"),
                    &["nested.a.x*nested.a.y+nested.a.x".into()],
                    &input,
                    &options,
                );
            }
        }
    }
    let noise = vec![r#"{"id":"ignored","a":[1,2,3,4,5,6,7,8]}"#; 1024].join(",");
    let input = record(1024, 24, true, false);
    let collision = format!("{{\"a\":[{noise}],{}", &input[1..]);
    bench("collision", &[projection(24, false)], &collision, &options);
    for n in [26, 27, 28, 32] {
        let input = record(1024, n, true, false);
        bench(
            &format!("width/{n}"),
            &[projection(n, false)],
            &input,
            &options,
        );
    }
    let simple_input = record(1024, 24, true, false);
    // Build conversions structurally rather than repairing expression parentheses.
    let direct = (0..24)
        .map(|i| format!("'n{i}':$number(metrics.v{i})*$number(metrics.v{i})"))
        .collect::<Vec<_>>()
        .join(",");
    let alias = (0..24)
        .map(|i| format!("'n{i}':($v:=$number(metrics.v{i});$v*$v)"))
        .collect::<Vec<_>>()
        .join(",");
    for (name, source) in [
        ("conversion/direct", format!("{{{direct}}}")),
        ("conversion/alias", format!("{{{alias}}}")),
        (
            "repeat/direct",
            format!(
                "{{{}}}",
                (0..8)
                    .map(|i| format!("'n{i}':$uppercase(name)"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
        (
            "repeat/alias",
            format!(
                "($n:=$uppercase(name);{{{}}})",
                (0..8)
                    .map(|i| format!("'n{i}':$n"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ),
    ] {
        bench(name, &[source], &simple_input, &options);
    }
    for (name, source) in [
        ("lookup/direct", "$lookup($vars.labels,id)"),
        ("lookup/alias", "($t:=$vars.labels;$lookup($t,id))"),
    ] {
        bench(name, &[source.into()], &simple_input, &options);
    }
    let body = "{'id':$v.id,'name':$uppercase($v.name),'region':$v.profile.region,'x':$v.metrics.v0+$v.metrics.v1,'label':$lookup($vars.labels,$v.id)}";
    for rows in [8, 128, 1024] {
        let row = record(500, 8, true, false);
        let input = format!("{{\"items\":[{}]}}", vec![row; rows].join(","));
        let path = body.replace("$v.", "");
        for (label, source) in [
            ("map", format!("$map(items,function($v){{{body}}})")),
            ("path", format!("items.{path}")),
            ("predicate_path",format!("items[metrics.v0>0].{path}")),
            ("predicate_grouped",format!("items[(metrics.v0>0)].{path}")),
            ("predicate_local",format!("items[($m:=metrics;$m.v0>0)].{path}")),
            ("predicate_string",format!("items[$contains(name,'a')].{path}")),
            ("predicate_map",format!("$map($filter(items,function($v){{$v.metrics.v0>0}}),function($v){{{body}}})")),
            ("table_alias","($t:=$vars.labels;$map(items,function($v){$lookup($t,$v.id)}))".to_string()),
            (
                "locals",
                "$map(items,function($v){($m:=$v.metrics;{'a':$m.v0+$m.v1,'b':$m.v0*$m.v1,'s':$uppercase($v.name)})})".to_string(),
            ),
            (
                "fold_map",
                "$sum($map(items,function($v){$v.metrics.v0*$v.metrics.v1+$v.metrics.v0}))".into(),
            ),
            (
                "fold_path",
                "$sum((items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))".into(),
            ),
        ] {
            bench(
                &format!("array/{rows}/{label}"),
                &[source],
                &input,
                &options,
            );
        }
    }
    for bad in [0, 127] {
        let mut rows = vec![r#"{"metrics":{"v0":1,"v1":2}}"#.to_owned(); 128];
        rows[bad] = r#"{"metrics":{"v0":null,"v1":2}}"#.into();
        let input = format!("{{\"items\":[{}]}}", rows.join(","));
        bench(
            &format!("errors/{bad}"),
            &["$sum((items).(metrics.v0*metrics.v1+metrics.v0))".into()],
            &input,
            &options,
        );
    }
}
