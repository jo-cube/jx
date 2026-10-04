#![no_main]
#[path = "../../crates/jx/tests/support/robustness.rs"]
#[allow(dead_code)]
mod boundaries;
libfuzzer_sys::fuzz_target!(|input: &[u8]| boundaries::exercise(input));
