// The benchmark's counting allocator is the only unsafe code in this workspace.
#![allow(unsafe_code)]
#[path = "workloads/aggregates.rs"]
mod aggregates;
#[path = "workloads/compiler.rs"]
mod compiler;
#[path = "workloads/constructors.rs"]
mod constructors;
#[path = "workloads/filters.rs"]
mod filters;
#[path = "workloads/lexical.rs"]
mod lexical;
#[path = "workloads/navigation.rs"]
mod navigation;
#[path = "workloads/scalars.rs"]
mod scalars;
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

fn measure(name: &str, bytes: usize, smoke: bool, operation: impl FnMut()) {
    measure_allocations(name, bytes, smoke, Some(0), operation);
}

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

fn benchmark(label: &str, input: &[u8], smoke: bool) {
    measure(&format!("{label}/validate"), input.len(), smoke, || {
        black_box(jx::validate(black_box(input)).unwrap());
    });
    for (name, source) in [
        ("identity", "$"),
        ("shallow", "id"),
        ("nested", "customer.id"),
        ("missing", "absent"),
        ("array_value", "items"),
    ] {
        let expression = jx::compile(source).unwrap();
        measure(&format!("{label}/{name}"), input.len(), smoke, || {
            expression
                .evaluate(black_box(input))
                .unwrap()
                .for_each(|value| {
                    black_box(value.as_raw().unwrap().as_bytes());
                })
                .unwrap();
        });
    }
    let expression = jx::compile("$").unwrap();
    // Reuse a sufficiently sized Vec so this includes actual output copies but
    // excludes allocator growth and physical I/O.
    let mut output = Vec::with_capacity(input.len());
    measure(
        &format!("{label}/identity_write"),
        input.len(),
        smoke,
        || {
            output.clear();
            expression
                .evaluate(black_box(input))
                .unwrap()
                .for_each(|value| {
                    value.write_compact(&mut output).unwrap();
                })
                .unwrap();
            black_box(&output);
        },
    );
}

fn array_workload(name: &str, source: &str, input: &str, expected_count: usize, smoke: bool) {
    let expression = jx::compile(source).unwrap();
    let mut count = 0;
    expression
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|_| count += 1)
        .unwrap();
    assert_eq!(count, expected_count, "{name}");
    measure(name, input.len(), smoke, || {
        expression
            .evaluate(black_box(input.as_bytes()))
            .unwrap()
            .for_each(|value| {
                black_box(value.as_raw().unwrap().as_bytes());
            })
            .unwrap();
    });
}

fn arrays(smoke: bool) {
    for width in [1, 8, 16, 128, 1024, 16384] {
        let rows = (0..width)
            .map(|id| {
                format!(
                    r#"{{"id":{id},"details":{{"price":42}},"tags":[1,2],"padding":"abcdefgh"}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let root = format!("[{rows}]");
        let object = format!(r#"{{"orders":{root}}}"#);
        array_workload("array/shallow", "orders.id", &object, width, smoke);
        array_workload(
            "array/nested",
            "orders.details.price",
            &object,
            width,
            smoke,
        );
        array_workload(
            "array/flatten",
            "orders.tags",
            &object,
            if width == 1 { 1 } else { width * 2 },
            smoke,
        );
        array_workload("array/missing", "orders.absent", &object, 0, smoke);
        array_workload("array/root", "id", &root, width, smoke);
        let expression = jx::compile("orders.id").unwrap();
        measure("array/cancel_first", object.len(), smoke, || {
            let result = expression
                .evaluate(black_box(object.as_bytes()))
                .unwrap()
                .try_for_each(|value| {
                    black_box(value.as_raw().unwrap().as_bytes());
                    Err(())
                });
            assert_eq!(result, Err(jx::ConsumeError::Consumer(())));
        });
    }
    let nested = format!(
        r#"{{"groups":[{}]}}"#,
        vec![r#"[{"orders":[{"id":1},{"id":2}]},null,[{"orders":{"id":3}}]]"#; 16].join(",")
    );
    array_workload(
        "array/nested_contexts",
        "groups.orders.id",
        &nested,
        48,
        smoke,
    );
    let sparse = format!(
        "[{}]",
        vec![r#"null,{},[],{"id":null},[{"id":1}],[{"id":[2,3]}]"#; 16].join(",")
    );
    array_workload("array/sparse", "id", &sparse, 64, smoke);
    let deep = format!("{}{{\"id\":7}}{}", "[".repeat(64), "]".repeat(64));
    array_workload("array/deep", "id", &deep, 1, smoke);
    array_workload("array/empty", "id", "[]", 0, smoke);
    array_workload("array/singleton", "$.id", r#"[{"id":[1]}]"#, 1, smoke);
    array_workload(
        "array/nested_values",
        "id",
        r#"[{"id":[[1],[2]]},{"id":[[3]]}]"#,
        3,
        smoke,
    );
}

fn main() {
    let smoke = std::env::args().any(|arg| arg == "--smoke");
    println!(
        "workload,input_bytes,sample,records,seconds,records_per_second,input_bytes_per_second,allocations_per_record,allocated_bytes_per_record"
    );
    measure_allocations("compile", 0, smoke, None, || {
        black_box(jx::compile(black_box("customer.id")).unwrap());
    });
    for size in [100, 500, 1024, 10 * 1024, 64 * 1024, 1024 * 1024] {
        let base = r#"{"id":7,"customer":{"id":42},"items":[1,2,3],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        assert_eq!(input.len(), size);
        benchmark("ascii", input.as_bytes(), smoke);
    }
    let structured = format!(
        r#"{{"id":7,"customer":{{"id":42}},"items":[{}]}}"#,
        (0..24)
            .map(|i| format!(r#"{{"n":{i},"ok":true}}"#))
            .collect::<Vec<_>>()
            .join(",")
    );
    benchmark("structured", structured.as_bytes(), smoke);
    let unicode = format!(
        r#"{{"\u0069d":7,"customer":{{"\u0069d":42}},"items":[],"text":"{}"}}"#,
        "é中😀".repeat(100)
    );
    benchmark("unicode_escaped_keys", unicode.as_bytes(), smoke);
    arrays(smoke);
    scalars::run(smoke);
    filters::run(smoke);
    aggregates::run(smoke);
    constructors::run(smoke);
    lexical::run(smoke);
    navigation::run(smoke);
    compiler::run(smoke);
}
