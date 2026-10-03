use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    for (name, source) in [
        (
            "conversion",
            "{'label':'id=' & $string(12),'n':$number('0x12'),'data':$string({'a':[1,2]})}",
        ),
        (
            "composition",
            "($domain:=$substringAfter(?,'@') ~> $substringBefore(?,'.');$domain(text))",
        ),
    ] {
        measure_allocations(&format!("compile/{name}"), 0, smoke, None, || {
            black_box(jx::compile(black_box(source)).unwrap());
        });
    }
    for size in [100, 500, 1024, 10 * 1024, 1024 * 1024] {
        let base = r#"{"text":"user@example.com","n":"12.5","v":12.5,"a":[1,2,3],"obj":{"n":1.2},"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, budget) in [
            (
                "string_identity",
                "$string(text)",
                json!(["user@example.com"]),
                0,
            ),
            ("number_text", "$number(n)", json!([12.5]), 0),
            ("number_value", "$number(v)", json!([12.5]), 0),
            ("string_number", "$string(v)", json!(["12.5"]), 6),
            ("string_object", "$string(obj)", json!(["{\"n\":1.2}"]), 24),
            (
                "string_pretty",
                "$string(obj,true)",
                json!(["{\n  \"n\": 1.2\n}"]),
                32,
            ),
            (
                "concat",
                "'v=' & v & ':' & text",
                json!(["v=12.5:user@example.com"]),
                32,
            ),
            (
                "folded",
                "'value=' & $string(12.5)",
                json!(["value=12.5"]),
                0,
            ),
            (
                "constructor",
                "{'label':'v=' & v,'number':$number(n),'original':obj}",
                json!([{"label":"v=12.5","number":12.5,"original":{"n":1.2}}]),
                24,
            ),
            (
                "chain_direct",
                "text ~> $substringAfter('@') ~> $substringBefore('.')",
                json!(["example"]),
                20,
            ),
            (
                "chain_values",
                "($f:=$trim ~> $uppercase;$f(text))",
                json!(["USER@EXAMPLE.COM"]),
                40,
            ),
            (
                "partial",
                "($f:=$substringAfter(?,'@') ~> $substringBefore(?,'.');$f(text))",
                json!(["example"]),
                32,
            ),
            (
                "partial_reused",
                "($f:=$substring(?,0,?);$g:=$f(?,4);[$g(text),$g(text)])",
                json!([["user", "user"]]),
                40,
            ),
            ("sum_direct", "$sum(a)", json!([6]), 0),
            ("sum_chain", "a ~> $sum()", json!([6]), 0),
            ("sum_function", "a ~> $sum", json!([6]), 8),
            ("sum_partial", "a ~> $sum(?)", json!([6]), 8),
        ] {
            workload(
                &format!("conversion/{name}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
        // Fixed layout and numeric-string grammar; both controls validate the record.
        let select = jx::compile("n").unwrap();
        measure_allocations("rust/number_text", input.len(), smoke, Some(0), || {
            select
                .evaluate(black_box(input.as_bytes()))
                .unwrap()
                .for_each(|v| {
                    let text = v.as_raw().unwrap().as_str();
                    black_box(text[1..text.len() - 1].parse::<f64>().unwrap());
                })
                .unwrap();
        });
    }
    for width in [8_u64, 128, 1024] {
        let data = json!({"rows":(0..width).map(|i|json!({"n":i.to_string(),"k":i%2,"name":format!("p{i}")})).collect::<Vec<_>>()});
        let input = data.to_string();
        let total = (width / 2..width).sum::<u64>();
        let labels = (width / 2..width)
            .rev()
            .map(|i| format!("p{i}={i}"))
            .collect::<Vec<_>>()
            .join(",");
        for (name, source, expected, budget) in [
            (
                "numeric_pipeline",
                format!(
                    "rows[$number(n)>={}] ~> $map(function($r){{$number($r.n)}}) ~> $sum()",
                    width / 2
                ),
                json!([total]),
                width * 8 + 32,
            ),
            (
                "labels",
                format!(
                    "rows[$number(n)>={}]^(>$number(n)) ~> $map(function($r){{$r.name & '=' & $r.n}}) ~> $join(',')",
                    width / 2
                ),
                json!([labels]),
                width * 16 + 64,
            ),
            (
                "grouping",
                "rows{$string(k):$sum(n.$number())}".to_owned(),
                json!([{"0":width*(width-2)/4,"1":width*width/4}]),
                width * 8 + 64,
            ),
            (
                "static_lookup",
                "$map(rows,function($r){$lookup({'0':'zero','1':'one'},$string($r.k))})".to_owned(),
                json!(
                    (0..width)
                        .map(|i| if i % 2 == 0 { "zero" } else { "one" })
                        .collect::<Vec<_>>()
                ),
                width * 8 + 32,
            ),
        ] {
            workload(
                &format!("composition/{name}"),
                &source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
        let numeric_data = json!({"rows":(0..width).map(|i|json!({"n":i})).collect::<Vec<_>>()});
        let numeric_input = numeric_data.to_string();
        for (name, source) in [
            ("direct_fold", "$sum(rows[n>=0].(n*3+1))"),
            ("chained_fold", "rows[n>=0].(n*3+1) ~> $sum()"),
        ] {
            workload(
                &format!("composition/{name}"),
                source,
                &numeric_input,
                json!([3 * width * (width - 1) / 2 + width]),
                0,
                smoke,
            );
        }
    }
}
