use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    for (name, source) in [
        ("regex", r"/[a-z]+@(\w+)\.(com|org)/i"),
        (
            "regex_callback",
            r#"$replace(text,/(\d+)F/,function($m){($number($m.groups[0])-32)*5/9 & 'C'})"#,
        ),
    ] {
        measure_allocations(&format!("compile/{name}"), 0, smoke, None, || {
            black_box(jx::compile(black_box(source)).unwrap());
        });
    }
    for size in [100, 500, 1024, 10 * 1024, 1024 * 1024] {
        let base = r#"{"text":"red Hat 68F; blue hat 32F","padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, budget) in [
            ("contains", "$contains(text,/hat/i)", json!([true]), 16),
            ("no_match", "$contains(text,/missing/)", json!([false]), 16),
            (
                "literal_contains",
                "$contains(text,'hat')",
                json!([true]),
                16,
            ),
            (
                "captures",
                "$match(text,/(\\d+)F/)",
                json!([{"match":"68F","index":8,"groups":["68"]},{"match":"32F","index":22,"groups":["32"]}]),
                48,
            ),
            (
                "replace",
                "$replace(text,/hat/i,'cap')",
                json!(["red cap 68F; blue cap 32F"]),
                48,
            ),
            (
                "replace_captures",
                "$replace(text,/(\\d+)F/,'<$1>')",
                json!(["red Hat <68>; blue hat <32>"]),
                64,
            ),
            (
                "replace_callback",
                "$replace(text,/(\\d+)F/,function($m){($number($m.groups[0])-32)*5/9 & 'C'})",
                json!(["red Hat 20C; blue hat 0C"]),
                128,
            ),
            (
                "split",
                "$split(text,/[ ;]+/)",
                json!([["red", "Hat", "68F", "blue", "hat", "32F"]]),
                64,
            ),
            (
                "closure",
                "($r:=/hat/i;$f:=function($s){$contains($s,$r)};$f(text))",
                json!([true]),
                32,
            ),
            (
                "next",
                "($r:=/hat/i;$m:=$r(text);[$m.start,$m.next().start])",
                json!([[4, 18]]),
                64,
            ),
            (
                "lookaround",
                r"$contains(text,/(?<=blue )hat(?= )/)",
                json!([true]),
                32,
            ),
            (
                "literal_replace",
                "$replace(text,'hat','cap')",
                json!(["red Hat 68F; blue cap 32F"]),
                24,
            ),
        ] {
            workload(
                &format!("matcher/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
        let select = jx::compile("text").unwrap();
        let regex = regress::Regex::with_flags("hat", "i").unwrap();
        measure_allocations("rust/matcher_contains", input.len(), smoke, Some(8), || {
            select
                .evaluate(black_box(input.as_bytes()))
                .unwrap()
                .for_each(|value| {
                    let raw = value.as_raw().unwrap();
                    let text = raw.as_str();
                    black_box(regex.find_ascii(&text[1..text.len() - 1]).is_some());
                })
                .unwrap();
        });
    }
    for size in [128, 4096, 65536] {
        let input = json!({"text":"a".repeat(size)+" marker123"}).to_string();
        for (name, source, expected, budget) in [
            (
                "long_contains",
                "$contains(text,/marker[0-9]+$/)",
                json!([true]),
                16,
            ),
            (
                "long_no_match",
                "$contains(text,/not_here/)",
                json!([false]),
                16,
            ),
            (
                "long_replace",
                "$replace(text,/marker([0-9]+)$/,'id=$1')",
                json!(["a".repeat(size) + " id=123"]),
                64,
            ),
        ] {
            workload(
                &format!("matcher/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
        let input = json!({"text":"ab12 ".repeat(size/4)}).to_string();
        workload(
            "matcher/repeated_replace",
            r"$replace(text,/ab(\d+)/,'n=$1')",
            &input,
            json!(["n=12 ".repeat(size / 4)]),
            size as u64 * 8 + 64,
            smoke,
        );
        workload(
            "matcher/limited_captures",
            r"$count($match(text,/ab(\d+)/,4))",
            &input,
            json!([4]),
            128,
            smoke,
        );
    }
    for width in [16usize, 256, 4096] {
        let data = json!({"text":"é😀a ".repeat(width)});
        let input = data.to_string();
        for (name, source, expected, budget) in [
            ("unicode_contains", "$contains(text,/a/)", json!([true]), 24),
            (
                "unicode_replace",
                "$replace(text,/a/,'X',2)",
                json!(["é😀X ".repeat(2) + &"é😀a ".repeat(width - 2)]),
                64,
            ),
            (
                "unicode_captures",
                "$count($match(text,/(.)a/,2))",
                json!([2]),
                64,
            ),
            (
                "unicode_zero_limit",
                "$replace(text,/a/,'X',0)",
                json!([data["text"]]),
                2,
            ),
        ] {
            workload(
                &format!("matcher/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
    }
    for width in [8u64, 128, 1024] {
        let input=json!({"rows":(0..width).map(|i|json!({"n":i,"label":if i%2==0 {"red Hat"}else{"coat"}})).collect::<Vec<_>>()}).to_string();
        let sum = width * (width - 2) / 4;
        workload(
            "matcher/filtered_sum",
            "$sum(rows[label ~> /hat/i].n)",
            &input,
            json!([sum]),
            width * 40 + 64,
            smoke,
        );
        workload(
            "matcher/mixed",
            "{'total':$sum(rows[$contains(label,/hat/i)].n),'labels':$map(rows[label~>/hat/i],function($r){$replace($r.label,/hat/i,'cap')})}",
            &input,
            json!([{"total":sum,"labels":(0..width/2).map(|_|"red cap").collect::<Vec<_>>()}]),
            width * 80 + 64,
            smoke,
        );
        workload(
            "matcher/grouped",
            "rows{$replace(label,/hat/i,'cap'):$sum(n)}",
            &input,
            json!([{"red cap":sum,"coat":width*width/4}]),
            width * 40 + 64,
            smoke,
        );
    }
}
