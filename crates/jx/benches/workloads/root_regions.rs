use super::{measure, navigation::workload};
use serde_json::json;
use std::hint::black_box;

fn mixed(size: usize) -> String {
    let mut input = r#"{"a":7,"b":11,"n":"Ada","c":{"x":3}"#.to_owned();
    let mut field = 0;
    loop {
        let next =
            format!(r#", "field{field}":{{"ok":true,"id":{field},"text":"abcdefghijklmnop"}}"#);
        if input.len() + next.len() + 10 > size {
            break;
        }
        input.push_str(&next);
        field += 1;
    }
    input.push_str(&format!(
        r#", "p":"{}"}}"#,
        "x".repeat(size - input.len() - 9)
    ));
    assert_eq!(input.len(), size);
    input
}
pub(super) fn run(smoke: bool) {
    for size in [100, 500, 1024, 10240] {
        let input = mixed(size);
        for (name, source, expected, budget) in [
            ("string", "n&':'&$string(a)&':'&n", json!(["Ada:7:Ada"]), 11),
            (
                "constructor",
                "{'name':n&':'&n,'value':$string(a+b),'n':c.x}",
                json!([{"name":"Ada:Ada","value":"18","n":3}]),
                10,
            ),
            ("shallow", "a", json!([7]), 0),
            ("nested", "c.x", json!([3]), 0),
            ("scalar", "(a+b)*(a-b)+a*b", json!([5]), 0),
            (
                "dense",
                "(a*b+a+b)*(a-b+a*b+a*a+b*b)+(a+b)*(a-b)",
                json!([23013]),
                0,
            ),
        ] {
            workload(
                &format!("root_regions/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
        measure("root_regions/validate_mixed", input.len(), smoke, || {
            black_box(jx::validate(black_box(input.as_bytes())).unwrap());
        });
    }
    let mut input = String::from("{");
    for i in 0..4096 {
        if i == 2048 {
            input.push_str(r#""a":7,"b":11,"n":"Ada","c":{"x":3},"#);
        }
        input.push_str(&format!(
            r#""unused{i}":{{"n":{i},"s":"abcdefghijklmnop"}},"#
        ));
    }
    input.pop();
    input.push('}');
    workload(
        "root_regions/sparse_string",
        "n&':'&$string(a)&':'&n",
        &input,
        json!(["Ada:7:Ada"]),
        11,
        smoke,
    );
    for width in [8_u64, 128, 1024] {
        let rows = (0..width).map(|i| json!({"a":i%7,"b":width-i,"n":format!("row-{i}"),"c":{"x":i%5},"pad":"abcdefghijklmnopqrstuvwx"})).collect::<Vec<_>>();
        let input = json!({"rows":rows}).to_string();
        let mapped = rows.iter().map(|r| json!({"s":format!("{}:{}:{}",r["n"].as_str().unwrap(),r["a"],r["n"].as_str().unwrap()),"v":r["a"].as_u64().unwrap()*r["b"].as_u64().unwrap()+r["a"].as_u64().unwrap()})).collect::<Vec<_>>();
        let strings = mapped.iter().map(|r| r["s"].clone()).collect::<Vec<_>>();
        let total = (0..width)
            .filter(|i| i % 7 > 2)
            .map(|i| (i % 7) * (width - i) + i % 7)
            .sum::<u64>();
        workload(
            "root_regions/mapped",
            "rows.{'s':n&':'&$string(a)&':'&n,'v':a*b+a}",
            &input,
            json!(mapped),
            width * 16 + 128,
            smoke,
        );
        workload(
            "root_regions/callback",
            "$map(rows,function($r){$r.n&':'&$string($r.a)&':'&$r.n})",
            &input,
            json!(strings),
            width * 16 + 128,
            smoke,
        );
        workload(
            "root_regions/fold",
            "$sum($map($filter(rows,function($r){$r.a>2 and $r.b>0}),function($r){$r.a*$r.b+$r.a}))",
            &input,
            json!([total]),
            0,
            smoke,
        );
        let dense_total = (0..width)
            .map(|i| {
                let a = (i % 7) as i64;
                let b = (width - i) as i64;
                (a * b + a + b) * (a - b + a * b + a * a + b * b) + (a + b) * (a - b)
            })
            .sum::<i64>();
        workload(
            "root_regions/dense_fold",
            "$sum(rows.((a*b+a+b)*(a-b+a*b+a*a+b*b)+(a+b)*(a-b)))",
            &input,
            json!([dense_total]),
            0,
            smoke,
        );
        let mut sorted = rows.clone();
        sorted.sort_by_key(|r| format!("{0}:{0}", r["n"].as_str().unwrap()));
        workload(
            "root_regions/sort",
            "rows^(n&':'&n).a",
            &input,
            json!(sorted.iter().map(|r| r["a"].clone()).collect::<Vec<_>>()),
            width * 8 + 128,
            smoke,
        );
        let mut groups = serde_json::Map::new();
        for i in 0..width {
            let key = format!("{0}:{0}", i % 5);
            let sum = groups.get(&key).and_then(|v| v.as_u64()).unwrap_or(0) + i % 7;
            groups.insert(key, json!(sum));
        }
        workload(
            "root_regions/group",
            "rows{$string(c.x)&':'&$string(c.x):$sum(a)}",
            &input,
            json!([groups]),
            width * 12 + 128,
            smoke,
        );
    }
    for size in [500, 1024, 10240, 262144, 1048576] {
        for (name, text) in [
            ("ascii", "x".repeat(size)),
            ("unicode", "é中😀".repeat(size / 9)),
            ("escaped", "quote\"slash\\tab\t".repeat(size / 16)),
        ] {
            let input = json!({"a":7,"b":11,"n":"Ada","pad":text}).to_string();
            measure(
                &format!("root_regions/validate_{name}"),
                input.len(),
                smoke,
                || {
                    black_box(jx::validate(black_box(input.as_bytes())).unwrap());
                },
            );
            workload(
                &format!("root_regions/traverse_{name}"),
                "n&':'&$string(a)&':'&n",
                &input,
                json!(["Ada:7:Ada"]),
                11,
                smoke,
            );
        }
    }
}
