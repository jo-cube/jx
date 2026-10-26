// Allocation instrumentation is confined to benchmarks.
#![allow(unsafe_code)]
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::{Duration, Instant},
};

struct CountingAllocator;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

// SAFETY: all requests are forwarded unchanged to System. Counters do not
// allocate, and no pointer is inspected or retained by this wrapper.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(layout.size() as u64, Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        BYTES.fetch_add(size as u64, Relaxed);
        unsafe { System.realloc(ptr, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn measure_allocations(
    name: &str,
    bytes: usize,
    smoke: bool,
    max_allocations: Option<u64>,
    mut operation: impl FnMut(),
) {
    // Select and extend measurements for profiling; smoke always checks everything.
    if !smoke {
        if let Ok(filter) = std::env::var("JX_BENCH_FILTER")
            && !name.contains(&filter)
        {
            return;
        }
        if let Ok(size) = std::env::var("JX_BENCH_BYTES")
            && bytes != size.parse::<usize>().expect("JX_BENCH_BYTES is an integer")
        {
            return;
        }
    }
    let sample_time = Duration::from_millis(
        std::env::var("JX_BENCH_SAMPLE_MS")
            .map(|ms| ms.parse().expect("JX_BENCH_SAMPLE_MS is an integer"))
            .unwrap_or(50),
    );
    for _ in 0..64 {
        operation();
    }
    let samples = if smoke { 1 } else { 7 };
    for sample in 0..samples {
        ALLOCS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        let start = Instant::now();
        let mut records = 0;
        loop {
            for _ in 0..32 {
                operation();
            }
            records += 32;
            if smoke || start.elapsed() >= sample_time {
                break;
            }
        }
        let elapsed = start.elapsed().as_secs_f64();
        let allocs = ALLOCS.load(Relaxed);
        let allocated = BYTES.load(Relaxed);
        let rate = records as f64 / elapsed;
        println!(
            "{name},{bytes},{sample},{records},{elapsed:.9},{rate:.0},{:.0},{:.6},{:.6}",
            rate * bytes as f64,
            allocs as f64 / records as f64,
            allocated as f64 / records as f64
        );
        if let Some(limit) = max_allocations {
            assert!(
                allocs <= records * limit,
                "allocation budget exceeded: {name}, {allocs} calls for {records} records"
            );
        }
    }
}

fn workload(
    name: &str,
    source: &str,
    input: &[u8],
    expected: Vec<serde_json::Value>,
    budget: Option<u64>,
    smoke: bool,
) {
    let options = jx::CompileOptions::default().constant_binding(
        "vars",
        jx::OwnedValue::from_json(br#"{"labels":{"k7":"seven"}}"#).unwrap(),
    );
    let expression = options.compile(source).unwrap();
    let mut output = Vec::with_capacity(input.len().max(65536));
    let mut run = || {
        output.clear();
        expression
            .evaluate(black_box(input))
            .unwrap()
            .for_each(|v| {
                v.write_compact(&mut output).unwrap();
                output.push(b'\n');
            })
            .unwrap();
        black_box(&output);
    };
    run();
    // Every timed shape has an independently constructed expected value.
    assert_eq!(
        serde_json::Deserializer::from_slice(&output)
            .into_iter::<serde_json::Value>()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        expected,
        "{name}"
    );
    measure_allocations(name, input.len(), smoke, budget, || {
        output.clear();
        expression
            .evaluate(black_box(input))
            .unwrap()
            .for_each(|v| {
                v.write_compact(&mut output).unwrap();
                output.push(b'\n');
            })
            .unwrap();
        black_box(&output);
    });
}
fn main() {
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    println!(
        "workload,input_bytes,sample,records,seconds,records_s,input_bytes_s,allocations,allocated_bytes"
    );
    let input = serde_json::json!({"id":"k7","x":3,"y":4,"name":"Ada","padding":"x".repeat(440)})
        .to_string();
    measure_allocations("validate", input.len(), smoke, Some(0), || {
        black_box(jx::validate(black_box(input.as_bytes())).unwrap());
    });
    measure_allocations("compile", 0, smoke, None, || {
        black_box(jx::compile(black_box("x*y+x")).unwrap());
    });
    for (name, source, expected, budget) in [
        ("field", "id", serde_json::json!("k7"), Some(0)),
        ("scalar", "x*y+x", serde_json::json!(15), Some(0)),
        (
            "lookup",
            "$lookup($vars.labels,id)",
            serde_json::json!("seven"),
            Some(8),
        ),
        (
            "conditional_call",
            "($f:=function($r){$r.x*$r.y};flag?$f($):x+y)",
            serde_json::json!(7),
            Some(16),
        ),
        (
            "invariant_eval",
            "$eval('x*y+x')",
            serde_json::json!(15),
            Some(32),
        ),
    ] {
        workload(
            name,
            source,
            input.as_bytes(),
            vec![expected],
            budget,
            smoke,
        );
    }
    for width in [8, 32, 40, 128] {
        let input = serde_json::Value::Object(
            (0..width)
                .map(|i| (format!("v{i}"), serde_json::json!(i)))
                .collect(),
        )
        .to_string();
        let source = format!(
            "{{{}}}",
            (0..width)
                .map(|i| format!("'v{i}':v{i}+v{i}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let expected = serde_json::Value::Object(
            (0..width)
                .map(|i| (format!("v{i}"), serde_json::json!(2 * i)))
                .collect(),
        );
        workload(
            &format!("projection/{width}"),
            &source,
            input.as_bytes(),
            vec![expected.clone()],
            Some(16),
            smoke,
        );
        workload(
            &format!("projection_call/{width}"),
            &format!(
                "function($r){{{} }}($)",
                source.replace(":v", ":$r.v").replace("+v", "+$r.v")
            ),
            input.as_bytes(),
            vec![expected],
            Some(64),
            smoke,
        );
    }
    for n in [8, 128, 1024] {
        let input=serde_json::json!({"rows":(0..n).map(|i|serde_json::json!({"v":i,"w":2})).collect::<Vec<_>>()}).to_string();
        workload(
            &format!("fold/{n}"),
            "$sum(rows.(v*w))",
            input.as_bytes(),
            vec![serde_json::json!((0..n).sum::<usize>() * 2)],
            Some(16),
            smoke,
        );
        workload(
            &format!("filtered/{n}"),
            "$sum((rows[v>3]).(v*w+v))",
            input.as_bytes(),
            vec![serde_json::json!((4..n).sum::<usize>() * 3)],
            Some(16),
            smoke,
        );
        workload(
            &format!("callback/{n}"),
            "$sum($map(rows,function($r){$r.v*$r.w}))",
            input.as_bytes(),
            vec![serde_json::json!((0..n).sum::<usize>() * 2)],
            None,
            smoke,
        );
    }
    workload(
        "context_capture",
        "groups.items.($p:=%;$map(v,function($n){$p.name & ':' & $string($n)}))",
        br#"{"groups":[{"name":"g","items":[{"v":[1,2]}]}]}"#,
        vec![serde_json::json!("g:1"), serde_json::json!("g:2")],
        None,
        smoke,
    );
}
