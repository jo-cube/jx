#![allow(unsafe_code)]

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

#[path = "foundation/mod.rs"]
mod foundation;
#[path = "reference/mod.rs"]
mod reference;

struct Case {
    name: String,
    drops: Vec<String>,
    tombstones: Vec<String>,
    projection: Option<String>,
    records: Vec<Option<Vec<u8>>>,
}
fn record(size: usize, fields: usize, flag: usize) -> String {
    let metrics = (0..fields)
        .map(|i| format!("\"v{i}\":{}", i + 1))
        .collect::<Vec<_>>()
        .join(",");
    let mut s = format!(
        r#"{{"id":"k{}","name":"Ad\u0061","deleted":{},"env":"{}","flag":{},"metrics":{{{metrics}}},"profile":{{"region":"eu"}},"pad":""}}"#,
        flag % 64,
        flag % 13 == 0,
        if flag % 10 == 0 { "test" } else { "production" },
        flag % 2 == 0
    );
    let pad = size.saturating_sub(s.len());
    s = s.replace("\"pad\":\"\"", &format!("\"pad\":\"{}\"", "x".repeat(pad)));
    s
}
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (size, fields) in [(500, 8), (1024, 40), (10240, 80)] {
        let members = |base: &str| {
            (0..fields)
                .map(|i| format!("'v{i}':{base}v{i}+{base}v{i}"))
                .chain([
                    "'id':id".into(),
                    "'name':$uppercase(name)".into(),
                    "'label':$lookup($vars.labels,id)".into(),
                    "'region':profile.region".into(),
                ])
                .collect::<Vec<_>>()
                .join(",")
        };
        for (form, expr) in [
            ("direct", format!("{{{}}}", members("metrics."))),
            ("alias", format!("($m:=metrics;{{{}}})", members("$m."))),
            (
                "conditional",
                format!("($m:=flag?metrics:metrics;{{{}}})", members("$m.")),
            ),
        ] {
            cases.push(Case {
                name: format!("project/{size}/{fields}/{form}"),
                drops: vec!["env!='production'".into()],
                tombstones: vec!["deleted=true".into()],
                projection: Some(expr),
                records: (0..64)
                    .map(|i| {
                        if i == 63 {
                            None
                        } else {
                            Some(record(size, fields, i).into_bytes())
                        }
                    })
                    .collect(),
            });
        }
    }
    for (name, expr) in [
        (
            "lexical",
            "items[($m:=metrics;$m.v0>0)].{'id':id,'name':$uppercase(name),'x':metrics.v0+metrics.v1,'label':$lookup($vars.labels,id)}",
        ),
        (
            "direct",
            "items[metrics.v0>0].{'id':id,'name':$uppercase(name),'x':metrics.v0+metrics.v1,'label':$lookup($vars.labels,id)}",
        ),
        (
            "map",
            "$map(items,function($v){{'id':$v.id,'name':$uppercase($v.name),'x':$v.metrics.v0+$v.metrics.v1,'label':$lookup($vars.labels,$v.id)}})",
        ),
        (
            "fold",
            "$sum((items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))",
        ),
    ] {
        for rows in [8, 1024] {
            let input = format!(
                "{{\"items\":[{}]}}",
                (0..rows)
                    .map(|i| record(500, 8, i))
                    .collect::<Vec<_>>()
                    .join(",")
            );
            cases.push(Case {
                name: format!("array/{rows}/{name}"),
                drops: vec![],
                tombstones: vec![],
                projection: Some(expr.into()),
                records: vec![Some(input.into_bytes())],
            });
        }
    }
    for (name, projection) in [
        ("pass", None),
        ("validate", None),
        ("lookup", Some("$lookup($vars.labels,id)".to_owned())),
    ] {
        cases.push(Case {
            name: name.into(),
            drops: vec![],
            tombstones: vec![],
            projection,
            records: (0..64)
                .map(|i| Some(record(1024, 8, i).into_bytes()))
                .collect(),
        });
    }
    cases
}
macro_rules! setup {
    ($module:ident,$case:ident,$vars:ident) => {
        $module::build_plan(
            &$case.drops,
            &$case.tombstones,
            false,
            $case.projection.as_deref(),
            Some(&$vars),
            $case.name == "validate",
        )
        .unwrap()
    };
}
macro_rules! worker_run {
    ($worker:ident,$policies:ident,$module:ident,$record:expr,$spare:ident,$timed:expr) => {{
        $spare.clear();
        let mut source = $record.as_ref().map(|bytes| {
            $spare.extend_from_slice(bytes);
            std::mem::take(&mut $spare)
        });
        let at = if $timed { Some(Instant::now()) } else { None };
        let result = $worker.execute_report(&mut source, $policies).unwrap();
        match result.action {
            $module::jsonata::Action::Project(bytes)
            | $module::jsonata::Action::PassThrough($module::jsonata::PassPayload::Exact(bytes))
            | $module::jsonata::Action::PassThrough($module::jsonata::PassPayload::Json {
                bytes,
                ..
            }) => {
                black_box(&bytes);
                $spare = bytes;
            }
            _ => {
                if let Some(bytes) = source {
                    $spare = bytes;
                }
            }
        }
        black_box(&result.issue);
        at.map(|at| at.elapsed().as_nanos())
    }};
}

fn check(vars: &str) {
    let sources = [
        (vec![], vec![], None),
        (vec!["true".into()], vec![], Some("$number('bad')")),
        (vec!["false".into(), "missing".into()], vec![], Some("id")),
        (vec!["items.v>0".into()], vec![], Some("id")),
        (vec![], vec!["true".into()], Some("$number('bad')")),
        (vec![], vec![], Some("missing")),
        (vec![], vec![], Some("[]")),
        (vec![], vec![], Some("items.v")),
        (vec![], vec![], Some("items#$i.{'i':$i,'v':v}")),
        (vec![], vec![], Some("items.{'id':%.id,'v':v}")),
        (vec![], vec![], Some("items#$i.(function(){$i}) ~> $map(function($f){$f()})")),
        (vec![], vec![], Some("$eval('items#$i.v')")),
        (vec![], vec![], Some("$sum((items[true]).(2))")),
        (vec![], vec![], Some("items[($x:=v;$x>0)].{'v':v*2}")),
        (vec![], vec![], Some("items.(flag?v:$number(s))")),
        (
            vec![],
            vec![],
            Some("($m:=flag?a:b;{'x':$m.x,'same':$m in [$m]})"),
        ),
        (vec![], vec![], Some("$lookup($vars.labels,id)")),
        (vec![], vec![], Some("{'s':$replace(id,/[0-9]+/,'X')}")),
        (vec![], vec![], Some("$formatNumber($count(items),'0.0')")),
        (vec![], vec![], Some("$eval('id')")),
        (vec![], vec![], Some("$ ~> |items|{'v':v+1}|")),
        (vec![], vec![], Some("{'f':/x/}")),
    ];
    let inputs = [
        None,
        Some("invalid"),
        Some("{\"a\":01}"),
        Some("{}"),
        Some("null"),
        Some("[1,2]"),
        Some("{\"id\":\"k1\",\"items\":[{\"v\":1},{\"v\":2}]}"),
        Some("{\"items\":[{\"v\":1},{\"v\":null}]}"),
        Some("{\"items\":[{\"v\":2,\"flag\":true},{\"s\":\"bad\"}]}"),
        Some("{\"flag\":true,\"a\":{\"x\":1,\"x\":2}}"),
    ];
    let mut compared = 0;
    for (drops, tombstones, projection) in sources {
        for drop_tombstones in [false, true] {
            for embeds in [false, true] {
                let r = reference::build_plan(
                    &drops,
                    &tombstones,
                    drop_tombstones,
                    projection,
                    Some(vars),
                    embeds,
                )
                .unwrap();
                let f = foundation::build_plan(
                    &drops,
                    &tombstones,
                    drop_tombstones,
                    projection,
                    Some(vars),
                    embeds,
                )
                .unwrap();
                let input = r.input_plan();
                let mut rw = reference::jsonata::Worker::new(&r, &input, embeds, 16384);
                let finput = f.input_plan();
                let mut fw = foundation::jsonata::Worker::new(&f, &finput, embeds, 16384);
                for invalid in 0..4 {
                    for evaluation in 0..3 {
                        for input in inputs {
                            use foundation::jsonata as fj;
                            use reference::jsonata as rj;
                            let rp = rj::ErrorPolicies {
                                invalid_json: [
                                    rj::InvalidJsonPolicy::Fail,
                                    rj::InvalidJsonPolicy::Drop,
                                    rj::InvalidJsonPolicy::Tombstone,
                                    rj::InvalidJsonPolicy::Pass,
                                ][invalid],
                                evaluation: [
                                    rj::EvaluationPolicy::Fail,
                                    rj::EvaluationPolicy::Drop,
                                    rj::EvaluationPolicy::Tombstone,
                                ][evaluation],
                            };
                            let fp = fj::ErrorPolicies {
                                invalid_json: [
                                    fj::InvalidJsonPolicy::Fail,
                                    fj::InvalidJsonPolicy::Drop,
                                    fj::InvalidJsonPolicy::Tombstone,
                                    fj::InvalidJsonPolicy::Pass,
                                ][invalid],
                                evaluation: [
                                    fj::EvaluationPolicy::Fail,
                                    fj::EvaluationPolicy::Drop,
                                    fj::EvaluationPolicy::Tombstone,
                                ][evaluation],
                            };
                            let mut a = input.map(|s| s.as_bytes().to_vec());
                            let mut b = a.clone();
                            let left = rw.execute_report(&mut a, rp);
                            let right = fw.execute_report(&mut b, fp);
                            assert_eq!(
                                format!("{left:?}"),
                                format!("{right:?}"),
                                "{projection:?} {input:?} invalid {invalid} evaluation {evaluation}"
                            );
                            assert_eq!(a, b, "source ownership {projection:?} {input:?}");
                            compared += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(reference::build_plan(&[], &[], false, Some("{'f':/x/}"), None, false).is_ok());
    assert!(reference::build_plan(&[], &[], false, Some("items@$v.v"), None, false).is_ok());
    assert!(foundation::build_plan(&[], &[], false, Some("items@$v.v"), None, false).is_ok());
    assert!(reference::build_plan(&[], &[], false, Some("(a+b).%"), None, false).is_err());
    assert!(foundation::build_plan(&[], &[], false, Some("(a+b).%"), None, false).is_err());
    eprintln!("correctness,{compared},shared_compile_boundary,1");
}
fn main() {
    let vars = format!(
        "{{\"labels\":{{{}}}}}",
        (0..4096)
            .map(|i| format!("\"k{i}\":\"label-{i}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    if std::env::var_os("JKQ_CHECK").is_some() {
        check(&vars);
        return;
    }
    let mode = std::env::var("JKQ_ENGINE").unwrap_or_else(|_| "foundation".into());
    let filter = std::env::var("JKQ_FILTER").unwrap_or_default();
    let rp = reference::jsonata::ErrorPolicies {
        invalid_json: reference::jsonata::InvalidJsonPolicy::Fail,
        evaluation: reference::jsonata::EvaluationPolicy::Fail,
    };
    let fp = foundation::jsonata::ErrorPolicies {
        invalid_json: foundation::jsonata::InvalidJsonPolicy::Fail,
        evaluation: foundation::jsonata::EvaluationPolicy::Fail,
    };
    println!(
        "workload,input_bytes,version,sample,records,seconds,records_per_second,allocations,requested_bytes,peak_live_bytes,p50_ns,p95_ns,p99_ns"
    );
    for case in cases().into_iter().filter(|c| {
        if std::env::var_os("JKQ_EXACT").is_some() {
            c.name == filter
        } else {
            c.name.contains(&filter)
        }
    }) {
        let input_bytes = case.records.iter().flatten().map(Vec::len).sum::<usize>() as f64
            / case.records.len() as f64;
        let r = setup!(reference, case, vars);
        let f = setup!(foundation, case, vars);
        let input = r.input_plan();
        let mut rw = reference::jsonata::Worker::new(&r, &input, false, 16384);
        let finput = f.input_plan();
        let mut fw = foundation::jsonata::Worker::new(&f, &finput, false, 16384);
        for record in &case.records {
            let mut a = record.clone();
            let mut b = record.clone();
            let left = rw.execute_report(&mut a, rp);
            let right = fw.execute_report(&mut b, fp);
            assert_eq!(format!("{left:?}"), format!("{right:?}"), "{}", case.name);
        }
        let mut spare = Vec::with_capacity(
            case.records
                .iter()
                .flatten()
                .map(Vec::len)
                .max()
                .unwrap_or(0),
        );
        let mut n = 0usize;
        let mut operation = |timed: bool| {
            let record = &case.records[n % case.records.len()];
            n += 1;
            if mode == "reference" {
                worker_run!(rw, rp, reference, record, spare, timed)
            } else {
                worker_run!(fw, fp, foundation, record, spare, timed)
            }
        };
        for _ in 0..256 {
            operation(false);
        }
        let base = LIVE.load(Relaxed);
        CALLS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        PEAK.store(base, Relaxed);
        for _ in 0..64 {
            operation(false);
        }
        let calls = CALLS.load(Relaxed) as f64 / 64.;
        let bytes = BYTES.load(Relaxed) as f64 / 64.;
        let peak = PEAK.load(Relaxed) - base;
        assert_eq!(LIVE.load(Relaxed), base, "retained record {}", case.name);
        let samples = std::env::var("JKQ_SAMPLES")
            .ok()
            .map(|s| s.parse().unwrap())
            .unwrap_or(7);
        for sample in 0..samples {
            ENABLED.store(false, Relaxed);
            let start = Instant::now();
            let mut count = 0;
            while start.elapsed() < Duration::from_millis(200) {
                for _ in 0..32 {
                    operation(false);
                }
                count += 32;
            }
            let seconds = start.elapsed().as_secs_f64();
            let mut latency = Vec::with_capacity(1024);
            for _ in 0..1024 {
                latency.push(operation(true).unwrap());
            }
            latency.sort_unstable();
            ENABLED.store(true, Relaxed);
            println!(
                "{},{input_bytes},{mode},{sample},{count},{seconds},{},{calls},{bytes},{peak},{},{},{}",
                case.name,
                count as f64 / seconds,
                latency[512],
                latency[972],
                latency[1013]
            );
            // Latency storage was allocated while counting was disabled.
            ENABLED.store(false, Relaxed);
            drop(latency);
            ENABLED.store(true, Relaxed);
            assert_eq!(LIVE.load(Relaxed), base, "timed residual {}", case.name);
        }
    }
}
