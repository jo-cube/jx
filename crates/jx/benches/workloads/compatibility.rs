use super::navigation::workload;
use serde_json::json;

pub(super) fn run(smoke: bool) {
    for size in [500, 10 * 1024] {
        let base = json!({"text":"Été Αθήνα", "date":"2024年12月31日", "n":"0x10", "padding":""})
            .to_string();
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, budget) in [
            ("unicode_icase", "$contains(text,/été/i)", json!([true]), 16),
            (
                "unicode_replace",
                "$replace(text,/ÉTÉ/i,'summer')",
                json!(["summer Αθήνα"]),
                32,
            ),
            (
                "unicode_date_static",
                "$toMillis(date,'[Y0001]年[M01]月[D01]日')",
                json!([1735603200000_i64]),
                8,
            ),
            ("numeric_partial", "($f:=$abs(?);$f(n))", json!([16]), 16),
        ] {
            workload(
                &format!("compatibility/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
    }
    for width in [8usize, 128, 1024] {
        let input =
            json!({"rows":(0..width).map(|i|json!({"n":width-i})).collect::<Vec<_>>()}).to_string();
        workload(
            "compatibility/sorted_tuple_filter",
            "rows#$i^(n)[i%2=0].$i",
            &input,
            json!((0..width).rev().filter(|i| i % 2 == 0).collect::<Vec<_>>()),
            width as u64 * 16 + 64,
            smoke,
        );
        workload(
            "compatibility/sorted_tuple_group",
            "rows#$i^(n)[i%2=0]{'total':$sum(`@`.n),'indices':$sum(i)}",
            &input,
            json!([{"total":width/2*(width/2+1),"indices":width/2*(width/2-1)}]),
            width as u64 * 24 + 64,
            smoke,
        );
    }
}
