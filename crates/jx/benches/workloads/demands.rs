use super::compiler::workload;
use serde_json::json;

pub(super) fn run(smoke: bool) {
    for size in [100, 500, 1024, 10240, 1048576] {
        let base = r#"{"active":true,"payload":{"x":7,"y":3,"padding":""}}"#;
        let input = base.replace(
            r#""padding":"""#,
            &format!(r#""padding":"{}""#, "x".repeat(size - base.len())),
        );
        for (name, source, expected) in [
            (
                "nested",
                "payload.x*payload.x+payload.y*payload.y",
                json!([58]),
            ),
            (
                "branch",
                "active ? payload.x*payload.x+payload.y : 0",
                json!([52]),
            ),
            (
                "repeated",
                "payload.x+payload.x+payload.x+payload.x",
                json!([28]),
            ),
        ] {
            workload(
                &format!("demand/{name}"),
                source,
                input.as_bytes(),
                expected,
                0,
                smoke,
            );
        }
        let input = input.replace("true", "false");
        workload(
            "demand/untaken_nested",
            "active ? payload.x*payload.x+payload.y : 0",
            input.as_bytes(),
            json!([0]),
            0,
            smoke,
        );
    }
    for count in [8, 128, 1024] {
        let rows = (0..count)
            .map(|n| json!({"active":n%2==0,"item":{"x":n%16,"y":2},"ignored":{"text":"abcdef"}}))
            .collect::<Vec<_>>();
        let input = serde_json::to_vec(&json!({"payload":{"rows":rows}})).unwrap();
        let total: u64 = (0..count)
            .filter(|n| n % 2 == 0)
            .map(|n| (n % 16 * 2 + 1) as u64)
            .sum();
        workload(
            "demand/nested_fold",
            "$sum(payload.rows[active].(item.x*item.y+1))",
            &input,
            json!([total]),
            0,
            smoke,
        );
        workload(
            "demand/nested_fold_plan",
            "$sum(payload.rows[active and item.x>=0].(item.x*item.y+1))",
            &input,
            json!([total]),
            0,
            smoke,
        );
    }
    for width in [16, 512, 4096] {
        let members = (0..width)
            .map(|n| format!(r#""unused{n}":{{"text":"abcdef","n":{n}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let input = format!(r#"{{{members},"payload":{{{members},"x":7,"y":3}}}}"#);
        workload(
            "demand/wide_nested",
            "payload.x*payload.x+payload.y*payload.y",
            input.as_bytes(),
            json!([58]),
            0,
            smoke,
        );
    }
    let table = (0..4096)
        .map(|n| (format!("k{n}"), json!(n)))
        .collect::<serde_json::Map<_, _>>();
    let table = serde_json::to_string(&table).unwrap();
    let source = format!(
        "$lookup({table},payload.key)*payload.qty + $lookup({table},payload.key)+payload.qty"
    );
    for size in [100, 500, 10240, 1048576] {
        let base = r#"{"payload":{"key":"k4095","qty":2},"padding":""}"#;
        let input = base.replace(
            r#""padding":"""#,
            &format!(r#""padding":"{}""#, "x".repeat(size - base.len())),
        );
        workload(
            "demand/repeated_lookup",
            &source,
            input.as_bytes(),
            json!([12287]),
            0,
            smoke,
        );
    }
}
