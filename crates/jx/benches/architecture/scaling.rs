#![allow(unsafe_code)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
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

fn runtime(family: &str, depth: usize, p: &jx::execution::Program) {
    if !matches!(family, "conditional" | "alias") || depth > 96 {
        return;
    }
    let mut nested = String::from("{\"x\":7}");
    if family == "conditional" {
        for _ in 0..depth {
            nested = format!("{{\"a\":{nested},\"b\":{{}}}}");
        }
    }
    let active = format!("{{\"flag\":true,{}", &nested[1..]);
    for (distribution, input) in [("absent", "{\"flag\":true}"), ("active", active.as_str())] {
        let mut a = jx::execution::Acquisition::new([p]);
        let mut output = Vec::with_capacity(1024);
        let mut operation = || {
            output.clear();
            let d = a.prepare(std::hint::black_box(input.as_bytes())).unwrap();
            p.try_for_each(&d, |v| {
                v.write_compact(&mut output).unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
            std::hint::black_box(&output);
        };
        for _ in 0..32 {
            operation();
        }
        let base = LIVE.load(Relaxed);
        CALLS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        PEAK.store(base, Relaxed);
        operation();
        let calls = CALLS.load(Relaxed);
        let requested = BYTES.load(Relaxed);
        let peak = PEAK.load(Relaxed) - base;
        assert_eq!(LIVE.load(Relaxed), base);
        ENABLED.store(false, Relaxed);
        let start = Instant::now();
        let mut n = 0;
        while start.elapsed() < Duration::from_millis(200) {
            for _ in 0..32 {
                operation();
            }
            n += 32;
        }
        let rate = n as f64 / start.elapsed().as_secs_f64();
        ENABLED.store(true, Relaxed);
        println!(
            "{family},{depth},{distribution},{},{rate},{calls},{requested},{peak}",
            input.len()
        );
        assert_eq!(
            output,
            if distribution == "active" {
                b"7".as_slice()
            } else {
                b""
            }
        );
    }
}
fn main() {
    let run = std::env::var_os("SCALING_RUNTIME").is_some();
    if run {
        println!(
            "family,depth,distribution,input_bytes,records_per_second,allocations,requested_bytes,peak_live_bytes"
        );
    } else {
        println!(
            "family,depth,instructions,registers,compile_ns,compile_calls,compile_requested,compile_retained,compile_peak,setup_ns,setup_calls,setup_requested,setup_retained,setup_peak,spans"
        );
    }
    let max = std::env::var("SCALE_MAX")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(96);
    for family in ["conditional", "alias", "callback", "nested", "constructor", "calls", "call_fanout", "constructors"] {
        for depth in [1, 2, 4, 8, 12, 24, 48, 96, 192]
            .into_iter()
            .filter(|n| *n <= max)
        {
            let source = match family {
                "conditional" => format!("($m:=$;{}$m.x)", "$m:=flag?$m.a:$m.b;".repeat(depth)),
                "callback" => format!(
                    "($m:=$;{}$map(items,function($v){{{{'x':$m.x,'v':$v}}}}))",
                    "$m:=flag?$m.a:$m.b;".repeat(depth)
                ),
                "alias" => format!("($m:=$;{}$m.x)", "$m:=flag?$m:$m;".repeat(depth)),
                "calls" => {
                    let mut body=String::from("{'x':$d.a.x,'y':$d.b.y}");
                    for _ in 0..depth { body=format!("function($d){{{body}}}($d)"); }
                    format!("($d:=$;{body})")
                }
                "call_fanout" => {
                    let mut body = String::from("$d.a.x+$d.b.y");
                    for _ in 0..depth { body = format!("($f:=function($d){{{body}}};$f($d)+$f($d))"); }
                    format!("function($d){{{body}}}($)")
                }
                "constructors" => {
                    let mut body = String::from("{'v':v}");
                    for _ in 0..depth { body = format!("{{'child':{body},'v':v}}"); }
                    format!("items.({body})")
                }
                "nested" => format!(
                    "($m:=a;{{{}}})",
                    (0..depth)
                        .map(|i| format!(
                            "'v{i}':$m.{}x+$m.{}x",
                            "a.".repeat(i % 8),
                            "a.".repeat(i % 8)
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
                _ => format!(
                    "{{{}}}",
                    (0..depth)
                        .map(|i| format!("'v{i}':flag?{{'a':a.x}}:{{'b':b.y}}"))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            };
            let baseline = LIVE.load(Relaxed);
            CALLS.store(0, Relaxed);
            BYTES.store(0, Relaxed);
            PEAK.store(baseline, Relaxed);
            let at = Instant::now();
            let p = match jx::CompileOptions::default().compile_program(&source) {
                Ok(p) => p,
                Err(e) => { eprintln!("rejected,{family},{depth},{e}"); continue; }
            };
            let compile = at.elapsed().as_nanos();
            let cc = CALLS.load(Relaxed);
            let cb = BYTES.load(Relaxed);
            let cr = LIVE.load(Relaxed) - baseline;
            let cp = PEAK.load(Relaxed) - baseline;
            if run {
                runtime(family, depth, &p);
                drop(p);
                assert_eq!(LIVE.load(Relaxed), baseline);
                continue;
            }
            let base = LIVE.load(Relaxed);
            CALLS.store(0, Relaxed);
            BYTES.store(0, Relaxed);
            PEAK.store(base, Relaxed);
            let at = Instant::now();
            let a = jx::execution::Acquisition::new([&p]);
            let setup = at.elapsed().as_nanos();
            let sc = CALLS.load(Relaxed);
            let sb = BYTES.load(Relaxed);
            let sr = LIVE.load(Relaxed) - base;
            let sp = PEAK.load(Relaxed) - base;
            println!(
                "{family},{depth},{},{},{compile},{cc},{cb},{cr},{cp},{setup},{sc},{sb},{sr},{sp},{}",
                p.instruction_count(),
                p.register_count(),
                a.retained_bytes()
            );
            drop(a);
            drop(p);
            assert_eq!(LIVE.load(Relaxed), baseline);
        }
    }
}
