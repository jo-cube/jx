use super::{measure_allocations, navigation::workload};
use serde_json::json;
use std::hint::black_box;

pub(super) fn run(smoke: bool) {
    for (name, source) in [
        ("signature", "($f:=function($x,$y)<n-n:n>{$x+$y};n.$f(2))"),
        (
            "tail",
            "($f:=function($n,$a)<nn:n>{$n=0?$a:$f($n-1,$a+1)};$f(n,0))",
        ),
    ] {
        measure_allocations(&format!("compile/function_{name}"), 0, smoke, None, || {
            black_box(jx::compile(black_box(source)).unwrap());
        });
    }
    for size in [100, 500, 1024, 10 * 1024, 1024 * 1024] {
        let base = r#"{"n":7,"v":42,"a":[1,2,3],"obj":{"label":"keep bytes"},"padding":""}"#;
        let input = base.replace(
            "\"padding\":\"\"",
            &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
        );
        for (name, source, expected, limit) in [
            ("builtin", "$abs(v)", json!([42]), 0),
            ("direct", "function($x){$x+1}(v)", json!([43]), 8),
            ("signed", "function($x)<n:n>{$x+1}(v)", json!([43]), 8),
            ("named", "($f:=function($x){$x+1};$f(v))", json!([43]), 10),
            (
                "signed_named",
                "($f:=function($x)<n:n>{$x+1};$f(v))",
                json!([43]),
                10,
            ),
            (
                "signature_context",
                "($f:=function($x,$y)<n-n:n>{$x+$y};n.$f(2))",
                json!([9]),
                10,
            ),
            (
                "signature_array",
                "function($x)<a<n>:a>{$x}(a)",
                json!([[1, 2, 3]]),
                8,
            ),
            (
                "signature_promote",
                "function($x)<a<n>:a>{$x}(v)",
                json!([[42]]),
                10,
            ),
            (
                "signature_optional",
                "function($x,$y)<ns?:n>{$x+1}(v)",
                json!([43]),
                8,
            ),
            (
                "signature_variadic",
                "function($x,$y)<n+n:n>{$x+$y}(n,v,3)",
                json!([49]),
                8,
            ),
            (
                "closure",
                "($base:=n;$f:=function($x)<n:n>{$base+$x};$f(v))",
                json!([49]),
                10,
            ),
            (
                "partial",
                "($f:=function($x,$y)<nn:n>{$x+$y};$p:=$f(?,n);$p(v))",
                json!([49]),
                14,
            ),
            (
                "callback",
                "$map(a,function($x){$x+1})",
                json!([2, 3, 4]),
                12,
            ),
            (
                "signed_callback",
                "$map(a,function($x)<n:n>{$x+1})",
                json!([2, 3, 4]),
                12,
            ),
            (
                "partial_callback",
                "$map(a,function($x,$y)<nn:n>{$x+$y}(?,n))",
                json!([8, 9, 10]),
                16,
            ),
            (
                "mixed",
                "($f:=function($x)<n:n>{$x*2};{'values':$map(a,$f),'sum':$sum(a),'label':obj.label})",
                json!([{"values":[2,4,6],"sum":6,"label":"keep bytes"}]),
                20,
            ),
        ] {
            workload(
                &format!("function/{name}"),
                source,
                &input,
                expected,
                limit,
                smoke,
            );
        }
        let select = jx::compile("v").unwrap();
        measure_allocations("rust/function_scalar", input.len(), smoke, Some(0), || {
            select
                .evaluate(black_box(input.as_bytes()))
                .unwrap()
                .for_each(|value| {
                    let n = value.as_raw().unwrap().as_str().parse::<f64>().unwrap();
                    black_box(n + 1.0);
                })
                .unwrap();
        });
    }
    for n in [8, 32, 64, 1024, 100_000] {
        let input = json!({"n":n,"obj":{"label":"borrowed"}}).to_string();
        for (name, source, expected, budget) in [
            (
                "tail",
                "($f:=function($n,$a){$n=0?$a:$f($n-1,$a+1)};$f(n,0))",
                json!([n]),
                10,
            ),
            (
                "signed_tail",
                "($f:=function($n,$a)<nn:n>{$n=0?$a:$f($n-1,$a+1)};$f(n,0))",
                json!([n]),
                10,
            ),
            (
                "tail_block",
                "($f:=function($n,$a){($next:=$n-1;$n=0?$a:$f($next,$a+1))};$f(n,0))",
                json!([n]),
                12,
            ),
            (
                "tail_borrow",
                "($f:=function($n,$x){$n=0?$x:$f($n-1,$x)};$f(n,obj))",
                json!([{"label":"borrowed"}]),
                10,
            ),
            (
                "tail_escape_once",
                "($f:=function($n,$g){$n=0?$g():$f($n-1,$n=3?function(){$n}:$g)};$f(n,function(){0}))",
                json!([3]),
                16,
            ),
        ] {
            workload(
                &format!("function/{name}_{n}"),
                source,
                &input,
                expected,
                budget,
                smoke,
            );
        }
        if n <= 32 {
            workload(
                &format!("function/recursion_{n}"),
                "($f:=function($n){$n=0?0:1+$f($n-1)};$f(n))",
                &input,
                json!([n]),
                2 * n + 12,
                smoke,
            );
        }
        let select = jx::compile("n").unwrap();
        measure_allocations(
            &format!("rust/function_tail_{n}"),
            input.len(),
            smoke,
            Some(0),
            || {
                select
                    .evaluate(black_box(input.as_bytes()))
                    .unwrap()
                    .for_each(|value| {
                        let n = value.as_raw().unwrap().as_str().parse::<usize>().unwrap();
                        let mut total = 0.0;
                        for _ in 0..n {
                            total += black_box(1.0);
                        }
                        black_box(total);
                    })
                    .unwrap();
            },
        );
    }
    for n in [8, 128, 1024] {
        let input = json!({"n":n}).to_string();
        workload(
            &format!("function/tail_capture_each_{n}"),
            "($f:=function($n,$g){$n=0?$g():$f($n-1,function(){$n})};$f(n,function(){0}))",
            &input,
            json!([1]),
            4 * n + 24,
            smoke,
        );
    }
}
