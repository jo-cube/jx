use super::measure_allocations;
use jx::{CompileOptions, Evaluation, InputPlan, OwnedValue};
use serde_json::json;
use std::hint::black_box;

fn serialize(evaluation: Evaluation<'_, '_>, output: &mut Vec<u8>) {
    output.clear();
    evaluation
        .for_each(|value| value.write_compact(&mut *output).unwrap())
        .unwrap();
    black_box(&*output);
}

// Many small fields and nested values rather than one large padding string.
fn record(size: usize, escaped: bool) -> String {
    let mut input = String::from("{");
    let mut field = 0;
    while input.len() + 190 < size {
        input.push_str(&format!(
            r#""f{field}":{{"id":{field},"ok":true,"text":"abcdefghijk"}},"#
        ));
        field += 1;
    }
    let tail = if escaped {
        r#""id":"k17","name":"Ad\u0061 Lovelace","env":"production","deleted":false,"price":7,"quantity":3,"pad":""}"#
    } else {
        r#""id":"k17","name":"Ada Lovelace","env":"production","deleted":false,"price":7,"quantity":3,"pad":""}"#
    };
    input.push_str(&tail.replace(
        r#""pad":"""#,
        &format!(r#""pad":"{}""#, "x".repeat(size - input.len() - tail.len())),
    ));
    assert_eq!(input.len(), size);
    input
}

fn expression(
    name: &str,
    source: &str,
    input: &str,
    options: &CompileOptions,
    budget: u64,
    smoke: bool,
) {
    let expression = options.compile(source).unwrap();
    #[cfg(feature = "jit")]
    let expression = {
        let mut expression = expression;
        if std::env::var_os("JX_BENCH_NATIVE").is_some() {
            expression.enable_native();
        }
        expression
    };
    let plan = InputPlan::new([&expression]);
    let mut output = Vec::with_capacity(input.len());
    serialize(expression.evaluate(input.as_bytes()).unwrap(), &mut output);
    let expected = output.clone();
    serialize(
        plan.prepare(input.as_bytes()).unwrap().evaluate(0).unwrap(),
        &mut output,
    );
    assert_eq!(output, expected, "{source}");
    for mode in ["full", "prepared"] {
        measure_allocations(
            &format!("realistic/{name}/{mode}"),
            input.len(),
            smoke,
            smoke.then_some(budget),
            || {
                let evaluation = if mode == "full" {
                    expression.evaluate(black_box(input.as_bytes())).unwrap()
                } else {
                    plan.prepare(black_box(input.as_bytes()))
                        .unwrap()
                        .evaluate(0)
                        .unwrap()
                };
                serialize(evaluation, &mut output);
            },
        );
    }
}

pub(super) fn run(smoke: bool) {
    let table = (0..4096)
        .map(|i| (format!("k{i}"), json!(format!("tenant-{i}"))))
        .collect::<serde_json::Map<_, _>>();
    let vars = json!({"labels":table,"skip":"staging"}).to_string();
    let options = CompileOptions::default()
        .constant_binding("vars", OwnedValue::from_json(vars.as_bytes()).unwrap());
    for (name, source) in [
        ("numeric", "price*quantity+price*price+quantity*quantity"),
        (
            "numeric_alias",
            "($p:=price;$q:=quantity;$p*$q+$p*$p+$q*$q)",
        ),
        (
            "projection",
            "{'id':id,'label':$lookup($vars.labels,id),'name':$uppercase(name),'total':price*quantity+price}",
        ),
    ] {
        measure_allocations(&format!("realistic/compile/{name}"), 0, smoke, None, || {
            black_box(options.compile(black_box(source)).unwrap());
        });
    }
    for size in [500, 1024, 10240] {
        for escaped in [false, true] {
            let input = record(size, escaped);
            let encoding = if escaped { "escaped" } else { "utf8" };
            for (name, source) in [
                ("contains", "$contains(name,'Lovelace')"),
                ("case", "$uppercase(name)"),
                ("substring", "$substring(name,1,6)"),
                ("before", "$substringBefore(name,' ')"),
                (
                    "repeated",
                    "$contains(name,'Ada') and $length(name)>3 and $contains(name,'Lovelace')",
                ),
                (
                    "projection",
                    "{'id':id,'label':$lookup($vars.labels,id),'name':$uppercase(name),'length':$length(name),'total':price*quantity+price}",
                ),
                ("lookup", "$lookup($vars.labels,id)"),
                (
                    "lookup_alias",
                    "($labels:=$vars.labels;$lookup($labels,id))",
                ),
                ("numeric", "price*quantity+price*price+quantity*quantity"),
                (
                    "numeric_alias",
                    "($p:=price;$q:=quantity;$p*$q+$p*$p+$q*$q)",
                ),
            ] {
                expression(
                    &format!("{encoding}/{name}"),
                    source,
                    &input,
                    &options,
                    match name {
                        "contains" => {
                            if escaped {
                                5
                            } else {
                                0
                            }
                        }
                        "case" => {
                            if escaped {
                                11
                            } else {
                                3
                            }
                        }
                        "substring" => {
                            if escaped {
                                8
                            } else {
                                2
                            }
                        }
                        "before" => {
                            if escaped {
                                6
                            } else {
                                2
                            }
                        }
                        "repeated" => {
                            if escaped {
                                9
                            } else {
                                0
                            }
                        }
                        "projection" => {
                            if escaped {
                                14
                            } else {
                                6
                            }
                        }
                        "lookup_alias" => 6,
                        _ => 0,
                    },
                    smoke,
                );
            }
            let expressions = ["env=$vars.skip", "deleted=true", "{'id':id,'label':$lookup($vars.labels,id),'name':$uppercase(name),'length':$length(name),'total':price*quantity+price}"]
                .map(|s| options.compile(s).unwrap());
            let plan = InputPlan::new(&expressions);
            let mut output = Vec::with_capacity(size);
            measure_allocations(
                &format!("realistic/{encoding}/jkq"),
                size,
                smoke,
                smoke.then_some(if escaped { 14 } else { 6 }),
                || {
                    let input = plan.prepare(black_box(input.as_bytes())).unwrap();
                    for index in 0..2 {
                        assert_eq!(
                            input
                                .evaluate(index)
                                .unwrap()
                                .single()
                                .unwrap()
                                .unwrap()
                                .as_bool(),
                            Some(false)
                        );
                    }
                    serialize(input.evaluate(2).unwrap(), &mut output);
                },
            );
        }
    }
    for width in [8_u64, 128, 1024] {
        let rows = (0..width).map(|i| json!({"price":i%7,"quantity":i%3+1,"name":format!("item-{i}"),"nested":{"value":i},"pad":"abcdefghijk"})).collect::<Vec<_>>();
        let input = json!({"rows":rows}).to_string();
        for (name, source) in [
            ("fold_path", "$sum(rows[price>2].(price*quantity+price))"),
            ("fold_field", "$sum(rows[price>2].price)"),
            (
                "fold_grouped",
                "$sum((rows[price>2]).(price*quantity+price))",
            ),
            (
                "fold_callback",
                "$sum($map($filter(rows,function($r){$r.price>2}),function($r){$r.price*$r.quantity+$r.price}))",
            ),
            (
                "strings_path",
                "rows.{'name':$uppercase(name),'length':$length(name),'value':nested.value+nested.value}",
            ),
            (
                "strings_callback",
                "$map(rows,function($r){{'name':$uppercase($r.name),'length':$length($r.name),'value':$r.nested.value+$r.nested.value}})",
            ),
        ] {
            expression(name, source, &input, &options, width * 16 + 128, smoke);
        }
        let expected = (0..width)
            .filter(|i| i % 7 > 2)
            .map(|i| (i % 7) * (i % 3 + 2))
            .sum::<u64>();
        let raw = jx::validate(input.as_bytes()).unwrap();
        measure_allocations("realistic/rust_fold", input.len(), smoke, Some(0), || {
            let input = jx::Value::Raw(black_box(raw));
            let mut sum = 0.0;
            for row in input.get("rows").unwrap().array_items().unwrap() {
                let price = row.get("price").unwrap().as_number().unwrap();
                if price > 2.0 {
                    sum += price * row.get("quantity").unwrap().as_number().unwrap() + price;
                }
            }
            assert_eq!(black_box(sum), expected as f64);
        });
    }
    for width in [8, 1024] {
        for bad_at in [0, width - 1] {
            let rows=(0..width).map(|i|json!({"price":3,"quantity":if i==bad_at {json!(null)} else {json!(2)},"name":"retained"})).collect::<Vec<_>>();
            let input = json!({"rows":rows}).to_string();
            for (name, source) in [
                ("path", "$sum(rows[price>2].(price*quantity+price))"),
                ("grouped", "$sum((rows[price>2]).(price*quantity+price))"),
            ] {
                let expression = options.compile(source).unwrap();
                let label = format!(
                    "realistic/fallback/{name}/{}",
                    if bad_at == 0 { "early" } else { "late" }
                );
                measure_allocations(&label, input.len(), smoke, None, || {
                    let result =
                        expression
                            .evaluate(black_box(input.as_bytes()))
                            .and_then(|evaluation| {
                                evaluation.for_each(|_| panic!("partial aggregate output"))
                            });
                    assert_eq!(
                        black_box(result.unwrap_err().kind),
                        jx::ErrorKind::TypeError
                    );
                });
            }
        }
    }
}
