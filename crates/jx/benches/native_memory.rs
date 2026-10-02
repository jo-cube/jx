// Isolated allocation/retention measurement; does not instrument throughput runs.
#![allow(unsafe_code)]
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicI64, AtomicU64, Ordering::Relaxed},
};
struct Counter;
static LIVE: AtomicI64 = AtomicI64::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
// SAFETY: requests and returned pointers are forwarded unchanged; no allocation
// is inspected, retained or used by the counter itself.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            LIVE.fetch_add(layout.size() as i64, Relaxed);
            CALLS.fetch_add(1, Relaxed);
            BYTES.fetch_add(layout.size() as u64, Relaxed);
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as i64, Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            LIVE.fetch_add(size as i64 - layout.size() as i64, Relaxed);
            CALLS.fetch_add(1, Relaxed);
            BYTES.fetch_add(size as u64, Relaxed);
        }
        result
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;
fn main() {
    let source = "((x+y)*(x-y)+(x*x+y*y))/(x+1)-y*3";
    // Warm process-global ISA/compiler initialization before measuring ownership.
    for _ in 0..3 {
        let mut e = jx::compile(source).unwrap();
        e.enable_native();
    }
    println!(
        "iteration,compile_requests,compile_requested_bytes,retained_heap_bytes,code_bytes,after_clone_drop_bytes,after_final_drop_bytes"
    );
    for iteration in 0..8 {
        let baseline = LIVE.load(Relaxed);
        let mut expr = jx::compile(source).unwrap();
        let interpreted = LIVE.load(Relaxed);
        CALLS.store(0, Relaxed);
        BYTES.store(0, Relaxed);
        let stats = expr.enable_native();
        assert_eq!(stats.failures, 0);
        assert_eq!(stats.kernels, 1);
        let requests = CALLS.load(Relaxed);
        let requested = BYTES.load(Relaxed);
        let retained = LIVE.load(Relaxed) - interpreted;
        let cloned = expr.clone();
        drop(expr);
        let after_clone = LIVE.load(Relaxed) - baseline;
        cloned
            .evaluate(br#"{"x":7,"y":3}"#)
            .unwrap()
            .for_each(|_| {})
            .unwrap();
        drop(cloned);
        let after_drop = LIVE.load(Relaxed) - baseline;
        assert_eq!(
            after_drop, 0,
            "retained allocation after final native expression drop"
        );
        println!(
            "{iteration},{requests},{requested},{retained},{},{after_clone},{after_drop}",
            stats.code_bytes
        );
    }
}
