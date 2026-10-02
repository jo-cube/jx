// Allocation and live-heap peaks are measured separately from throughput.
#![allow(unsafe_code)]
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed},
};
struct Counter;
static LIVE: AtomicI64 = AtomicI64::new(0);
static PEAK: AtomicI64 = AtomicI64::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
fn add(bytes: i64) {
    let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(live, Relaxed);
}
// SAFETY: forward layouts/pointers unchanged to System; counters never inspect storage.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            add(layout.size() as i64);
            CALLS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as i64, Relaxed);
        unsafe { System.dealloc(p, layout) };
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, layout, size) };
        if !q.is_null() {
            add(size as i64 - layout.size() as i64);
            CALLS.fetch_add(1, Relaxed);
            BYTES.fetch_add(size as u64, Relaxed);
        }
        q
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
fn main() {
    println!(
        "workload,width,input_bytes,iteration,allocations,requested_bytes,peak_live_bytes,live_at_delivery_bytes,after_drop_bytes"
    );
    for width in [8_u64, 128, 1024, 16384] {
        let input=serde_json::json!({"rows":(0..width).map(|i|serde_json::json!({"a":i%7,"b":width-i})).collect::<Vec<_>>(),"code":"function($r){$r.a+$r.b+$r.a}"}).to_string();
        for (name, source) in [
            (
                "transient",
                "$map(rows,function($r){($f:=function(){$r.a};$f()+$r.b)})",
            ),
            (
                "tail_transient",
                "$map(rows,function($r){($f:=function(){$r.a+$r.b};$f())})",
            ),
            (
                "recursive_local",
                "$map(rows,function($r){($f:=function($n){$n=0?$r.a:$f($n-1)};$f(2)+$r.b)})",
            ),
            (
                "escaping",
                "($fns:=$map(rows,function($r){function(){$r.a+$r.b}});$map($fns,function($f){$f()}))",
            ),
            (
                "mixed_escape",
                "($fns:=$map(rows,function($r){function(){$r.a+$r.b}});$temps:=$map(rows,function($r){($f:=function(){$r.a};$f()+$r.b)});$sum($map($fns,function($f){$f()}))+$sum($temps))",
            ),
            (
                "reduce",
                "$reduce(rows,function($acc,$r){($f:=function(){$r.a+$r.b};$acc+$f())},0)",
            ),
            ("dynamic_scalar", "($f:=$eval(code);$map(rows,$f))"),
            ("planned", "$map(rows,function($r){$r.a+$r.b+$r.a})"),
        ] {
            let expr = jx::compile(source).unwrap();
            expr.evaluate(input.as_bytes())
                .unwrap()
                .for_each(|_| {})
                .unwrap();
            for iteration in 0..5 {
                let base = LIVE.load(Relaxed);
                PEAK.store(base, Relaxed);
                CALLS.store(0, Relaxed);
                BYTES.store(0, Relaxed);
                let mut delivery = 0;
                expr.evaluate(input.as_bytes())
                    .unwrap()
                    .for_each(|v| {
                        delivery = delivery.max(LIVE.load(Relaxed) - base);
                        std::hint::black_box(v);
                    })
                    .unwrap();
                let after = LIVE.load(Relaxed) - base;
                assert_eq!(after, 0, "evaluation leaked heap storage");
                println!(
                    "{name},{width},{},{iteration},{},{},{},{delivery},{after}",
                    input.len(),
                    CALLS.load(Relaxed),
                    BYTES.load(Relaxed),
                    PEAK.load(Relaxed) - base
                );
            }
        }
    }
}
