use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    measure_allocations("compile/helpers", 0, smoke, None, || {
        black_box(
            jx::compile(black_box(
                "$zip($sort([3,1,2]),[$round(4.525,2),$round(2.345,2)])",
            ))
            .unwrap(),
        );
    });
    for size in [100, 500, 1024, 10 * 1024, 1024 * 1024] {
        let base = r#"{"n":4.525,"text":"ab","a":[3,1,2],"b":[4,5,6],"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, budget) in [
            ("round", "$round(n)", json!([5]), 0),
            ("static_round", "$round(2.5)", json!([2]), 0),
            ("decimal_round", "$round(n,2)", json!([4.52]), 0),
            ("pad", "$pad(text,-8,'0')", json!(["000000ab"]), 16),
            ("pad_unchanged", "$pad(text,1)", json!(["ab"]), 0),
            (
                "uri_unchanged",
                "$encodeUrlComponent(text)",
                json!(["ab"]),
                0,
            ),
            ("base64", "$base64encode(text)", json!(["YWI="]), 8),
            ("sort", "$sort(a)", json!([[1, 2, 3]]), 8),
            ("zip", "$zip(a,b)", json!([[[3, 4], [1, 5], [2, 6]]]), 16),
            ("single", "$single(a,function($v){$v=2})", json!([2]), 32),
            ("assert", "$assert(n>0)", json!([]), 0),
            ("static_sort", "$sort([3,1,2])", json!([[1, 2, 3]]), 1),
        ] {
            workload(
                &format!("helper/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
    }
    for width in [8_u64, 128, 1024] {
        let rows: Vec<_> = (0..width)
            .rev()
            .map(|i| json!({"n":i,"kind":if i%2==0 {"a"} else {"b"}}))
            .collect();
        let input = json!({"rows":rows}).to_string();
        workload(
            "helper/sort_sequence",
            "$sort(rows.n)",
            &input,
            json!([(0..width).collect::<Vec<_>>()]),
            64,
            smoke,
        );
        workload(
            "helper/zip_sequence",
            "$zip(rows.n,rows.n)",
            &input,
            json!([(0..width).rev().map(|i| json!([i, i])).collect::<Vec<_>>()]),
            width * 2 + 64,
            smoke,
        );
        workload(
            "helper/sort_callback",
            "$sort(rows,function($a,$b){$a.n>$b.n}).n",
            &input,
            json!((0..width).collect::<Vec<_>>()),
            width * 40 + 64,
            smoke,
        );
        workload(
            "helper/single_sequence",
            "$single(rows.n,function($v){$v=1})",
            &input,
            json!([1]),
            width * 8 + 64,
            smoke,
        );
        workload(
            "helper/group_round",
            "rows{kind:$round($average(n),1)}",
            &input,
            json!([{"a":width/2-1,"b":width/2}]),
            128,
            smoke,
        );
        workload(
            "helper/filtered_sorted",
            "$sort(rows[n%2=0],function($a,$b){$a.n>$b.n}).{'n':n,'label':$pad($string(n),-4,'0')}",
            &input,
            json!(
                (0..width)
                    .filter(|i| i % 2 == 0)
                    .map(|n| json!({"n":n,"label":format!("{n:04}")}))
                    .collect::<Vec<_>>()
            ),
            width * 48 + 128,
            smoke,
        );
    }
    for length in [128, 1024, 10 * 1024, 1024 * 1024] {
        let text = "é/& ".repeat(length / 5);
        let input = json!({"text":text}).to_string();
        workload(
            "helper/uri_roundtrip",
            "$decodeUrlComponent($encodeUrlComponent(text))",
            &input,
            json!([text]),
            96,
            smoke,
        );
        workload(
            "helper/base64_roundtrip",
            "$base64decode($base64encode(text))",
            &input,
            json!([text]),
            64,
            smoke,
        );
    }
}
