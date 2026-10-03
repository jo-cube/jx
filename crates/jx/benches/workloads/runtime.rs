use super::navigation::workload;
use serde_json::json;

pub(super) fn run(smoke: bool) {
    for width in [8_u64, 128, 1024, 16384] {
        let rows = (0..width)
            .map(|i| json!({"a":i%7,"b":width-i,"detail":{"label":"abc","n":i}}))
            .collect::<Vec<_>>();
        let input = json!({"rows":rows,"code":"function($r){$r.a+$r.b+$r.a}","objectCode":"function($r){ {'n':$r.a+$r.b} }"}).to_string();
        let values = (0..width).map(|i| i % 7 + width - i).collect::<Vec<_>>();
        let total = values.iter().sum::<u64>();
        for (name, source, expected, budget) in [
            (
                "transient",
                "$map(rows,function($r){($f:=function(){$r.a};$f()+$r.b)})",
                json!(values),
                width * 8 + 128,
            ),
            (
                "tail_transient",
                "$map(rows,function($r){($f:=function(){$r.a+$r.b};$f())})",
                json!(values),
                width * 12 + 128,
            ),
            (
                "recursive_local",
                "$map(rows,function($r){($f:=function($n){$n=0?$r.a:$f($n-1)};$f(2)+$r.b)})",
                json!(values),
                width * 12 + 128,
            ),
            (
                "escaping",
                "($fns:=$map(rows,function($r){function(){$r.a+$r.b}});$map($fns,function($f){$f()}))",
                json!(values),
                width * 8 + 128,
            ),
            (
                "mixed_escape",
                "($fns:=$map(rows,function($r){function(){$r.a+$r.b}});$temps:=$map(rows,function($r){($f:=function(){$r.a};$f()+$r.b)});$sum($map($fns,function($f){$f()}))+$sum($temps))",
                json!([total * 2]),
                width * 16 + 128,
            ),
            (
                "filter",
                "$filter(rows,function($r){($p:=function(){$r.a>2};$p() and $r.b>0)}).b",
                json!(
                    (0..width)
                        .filter(|i| i % 7 > 2)
                        .map(|i| width - i)
                        .collect::<Vec<_>>()
                ),
                width * 12 + 128,
            ),
            (
                "reduce",
                "$reduce(rows,function($acc,$r){($f:=function(){$r.a+$r.b};$acc+$f())},0)",
                json!([total]),
                width * 12 + 128,
            ),
            (
                "sort",
                "$sort(rows,function($a,$b){($key:=function($r){$r.a+$r.b};$key($a)>$key($b))}).b",
                json!(
                    ({
                        let mut order = (0..width).collect::<Vec<_>>();
                        order.sort_by_key(|i| i % 7 + width - i);
                        order.into_iter().map(|i| width - i).collect::<Vec<_>>()
                    })
                ),
                width * 128 + 128,
            ),
            (
                "partial",
                "$map(rows,function($r,$base){$r.a+$r.b+$base}(?,3))",
                json!(values.iter().map(|v| v + 3).collect::<Vec<_>>()),
                32,
            ),
            (
                "strings",
                "$map(rows,function($r){$r.detail.label & $r.detail.label & $r.detail.label})",
                json!(vec!["abcabcabc"; width as usize]),
                width * 8 + 128,
            ),
            (
                "dynamic_scalar",
                "($f:=$eval(code);$map(rows,$f))",
                json!(
                    (0..width)
                        .map(|i| 2 * (i % 7) + width - i)
                        .collect::<Vec<_>>()
                ),
                width * 64 + 128,
            ),
            (
                "dynamic_object",
                "($f:=$eval(objectCode);$map(rows,$f))",
                json!(
                    (0..width)
                        .map(|i| json!({"n":i%7+width-i}))
                        .collect::<Vec<_>>()
                ),
                width * 64 + 128,
            ),
            (
                "planned",
                "$map(rows,function($r){$r.a+$r.b+$r.a})",
                json!(
                    (0..width)
                        .map(|i| 2 * (i % 7) + width - i)
                        .collect::<Vec<_>>()
                ),
                24,
            ),
        ] {
            // Comparator callbacks grow with n log n; the request budget is a ceiling.
            workload(
                &format!("runtime/{name}"),
                source,
                &input,
                expected,
                if name == "sort" {
                    width * 512 + 128
                } else {
                    budget
                },
                smoke,
            );
        }
        let data=json!({"a":(0..width).rev().collect::<Vec<_>>(),"reduceCode":"function($a,$b){$a+$b}","sortCode":"function($a,$b){$a>$b}"}).to_string();
        workload(
            "runtime/dynamic_reduce",
            "($f:=$eval(reduceCode);$reduce(a,$f,0))",
            &data,
            json!([width * (width - 1) / 2]),
            width * 64 + 128,
            smoke,
        );
        workload(
            "runtime/dynamic_sort",
            "($f:=$eval(sortCode);$sort(a,$f))",
            &data,
            json!([(0..width).collect::<Vec<_>>()]),
            width * 512 + 128,
            smoke,
        );
    }
}
