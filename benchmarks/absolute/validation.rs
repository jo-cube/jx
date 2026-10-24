//! Independent complete-JSON validation controls, with explicit policy probes.
#![allow(unsafe_code, dead_code)]
mod fixtures;
mod metrics;
use std::hint::black_box;
fn accept(mode: &str, input: &[u8]) -> bool {
    match mode {
        "jx" => jx::validate(input).is_ok(),
        "serde_raw" => serde_json::from_slice::<&serde_json::value::RawValue>(input).is_ok(),
        _ => serde_json::from_slice::<serde::de::IgnoredAny>(input).is_ok(),
    }
}
fn main() {
    let mode = std::env::var("ABS_MODE").unwrap_or_else(|_| "jx".into());
    assert!(
        ["jx", "serde_raw", "serde_ignored"].contains(&mode.as_str()),
        "unknown validator mode"
    );
    if std::env::var_os("ABS_CHECK").is_some() {
        let mut probes = vec![
            b"{\"unused\":\"\xff\"}".to_vec(),
            b"{\"unused\":\"\\uD800\"}".to_vec(),
            b"1e9999".to_vec(),
            b"{} false".to_vec(),
            b"{\"a\":[1,]}".to_vec(),
            b"{\"a\":1,\"a\":2}".to_vec(),
            b"\"\\uZZZZ\"".to_vec(),
        ];
        probes.push(format!("{}0{}", "[".repeat(200), "]".repeat(200)).into_bytes());
        for (i, p) in probes.iter().enumerate() {
            println!(
                "{}",
                serde_json::json!({"probe":i,"bytes":p,"jx":accept("jx",p),"serde_raw":accept("serde_raw",p),"serde_ignored":accept("serde_ignored",p)})
            );
        }
        return;
    }
    println!(
        "case,mode,phase,sample,records,wall_seconds,cpu_seconds,input_bytes,output_bytes,allocations,allocated_bytes,peak_heap,probe_output_bytes"
    );
    for size in [500, 1024, 10240, 1048576] {
        let input = fixtures::record(size, 8, 17, false);
        // Report the actual byte count, rather than relying on the nominal label.
        assert!(accept("jx", input.as_slice()));
        assert!(accept("serde_raw", input.as_slice()));
        metrics::measure(
            &format!("json/{size}"),
            &mode,
            "validate",
            input.len() as f64,
            || {
                assert!(accept(&mode, black_box(input.as_slice())));
                0
            },
        );
    }
    let row = r#"{"metrics":{"v0":1,"v1":2},"name":"Ad\u0061","unused":{"a":[1,2,3],"b":true}}"#;
    for n in [8, 1024] {
        let input = format!("{{\"items\":[{}]}}", vec![row; n].join(","));
        metrics::measure(
            &format!("json/rows/{n}"),
            &mode,
            "validate",
            input.len() as f64,
            || {
                assert!(accept(&mode, black_box(input.as_bytes())));
                0
            },
        );
    }
}
