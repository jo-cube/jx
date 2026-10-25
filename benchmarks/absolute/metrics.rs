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

pub fn cpu() -> f64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: process clock receives a valid, initialized timespec pointer.
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut t) },
        0
    );
    t.tv_sec as f64 + t.tv_nsec as f64 * 1e-9
}
pub fn measure(name: &str, mode: &str, phase: &str, input: f64, operation: impl FnMut() -> usize) {
    measure_cycle(name, mode, phase, input, 1, operation)
}
// Probe complete deterministic distributions, including late first-output records.
pub fn measure_cycle(
    name: &str,
    mode: &str,
    phase: &str,
    input: f64,
    cycle: usize,
    mut operation: impl FnMut() -> usize,
) {
    assert!(cycle > 0);
    let warm = 64_usize.div_ceil(cycle) * cycle;
    let probe = 128_usize.div_ceil(cycle) * cycle;
    let ms = std::env::var("ABS_MS")
        .ok()
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap_or(250);
    let samples = std::env::var("ABS_SAMPLES")
        .ok()
        .map(|s| s.parse::<usize>().unwrap())
        .unwrap_or(5);
    for _ in 0..warm {
        black_box(operation());
    }
    // Initialize retained stderr state before establishing the allocation baseline.
    eprintln!("ready,{name},{mode},{phase}");
    let base = LIVE.load(Relaxed);
    CALLS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    PEAK.store(base, Relaxed);
    let mut output = 0;
    for _ in 0..probe {
        output += operation();
    }
    let allocations = CALLS.load(Relaxed) as f64 / probe as f64;
    let bytes = BYTES.load(Relaxed) as f64 / probe as f64;
    let peak = PEAK.load(Relaxed) - base;
    assert_eq!(LIVE.load(Relaxed), base, "warm residual {name}");
    for sample in 0..samples {
        ENABLED.store(false, Relaxed);
        let cpu = cpu();
        let at = Instant::now();
        let mut n = 0;
        let mut produced = 0;
        while at.elapsed() < Duration::from_millis(ms) {
            for _ in 0..32 {
                produced += black_box(operation());
            }
            n += 32;
        }
        let wall = at.elapsed().as_secs_f64();
        let cpu = crate::metrics::cpu() - cpu;
        ENABLED.store(true, Relaxed);
        assert_eq!(LIVE.load(Relaxed), base, "sample residual {name}");
        println!(
            "{name},{mode},{phase},{sample},{n},{wall:.9},{cpu:.9},{input},{},{allocations},{bytes},{peak},{}",
            produced as f64 / n as f64,
            output as f64 / probe as f64
        );
    }
}
pub fn parallel_disabled() {
    ENABLED.store(false, Relaxed);
}
