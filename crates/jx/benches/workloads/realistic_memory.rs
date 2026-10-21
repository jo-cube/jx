use super::{BYTES, CALLS, LIVE, PEAK, Relaxed};
use jx::{CompileOptions, OwnedValue};
use std::{hint::black_box, sync::Arc};

fn begin() -> i64 {
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    CALLS.store(0, Relaxed);
    BYTES.store(0, Relaxed);
    base
}
fn report(name: &str, width: usize, bytes: usize, iteration: usize, base: i64, delivery: i64) {
    let after = LIVE.load(Relaxed) - base;
    assert_eq!(after, 0, "{name} retained storage after drop");
    println!(
        "{name},{width},{bytes},{iteration},{},{},{},{delivery},{after}",
        CALLS.load(Relaxed),
        BYTES.load(Relaxed),
        PEAK.load(Relaxed) - base
    );
}
pub(super) fn run() {
    let table = (0..4096)
        .map(|i| (format!("k{i}"), serde_json::json!(format!("tenant-{i}"))))
        .collect::<serde_json::Map<_, _>>();
    let vars = serde_json::json!({"labels":table}).to_string();
    let vars = Arc::new(OwnedValue::from_json(vars.as_bytes()).unwrap());
    let options = CompileOptions::default().constant_binding("vars", vars.clone());
    let input = serde_json::json!({"id":"k17","name":"Ada Lovelace","price":7,"quantity":3,"pad":"x".repeat(922)}).to_string();
    for (name, source) in [
        (
            "projection",
            "{'id':id,'label':$lookup($vars.labels,id),'name':$uppercase(name),'length':$length(name),'total':price*quantity+price}",
        ),
        (
            "numeric_alias",
            "($p:=price;$q:=quantity;$p*$q+$p*$p+$q*$q)",
        ),
        (
            "lookup_alias",
            "($labels:=$vars.labels;$lookup($labels,id))",
        ),
    ] {
        let compile_name = format!("compile/{name}");
        let clone_name = format!("clone/{name}");
        let evaluate_name = format!("evaluate/{name}");
        let base = begin();
        let expression = options.compile(source).unwrap();
        let retained = LIVE.load(Relaxed) - base;
        drop(expression);
        report(&compile_name, 4096, input.len(), 0, base, retained);
        let expression = options.compile(source).unwrap();
        let base = begin();
        let cloned = expression.clone();
        let retained = LIVE.load(Relaxed) - base;
        drop(cloned);
        report(&clone_name, 4096, input.len(), 0, base, retained);
        let mut output = Vec::with_capacity(input.len());
        for iteration in 0..5 {
            let base = begin();
            let mut delivery = 0;
            output.clear();
            expression
                .evaluate(black_box(input.as_bytes()))
                .unwrap()
                .for_each(|value| {
                    delivery = delivery.max(LIVE.load(Relaxed) - base);
                    value.write_compact(&mut output).unwrap();
                })
                .unwrap();
            report(&evaluate_name, 4096, input.len(), iteration, base, delivery);
        }
    }
    for width in [8, 128, 1024] {
        let input=serde_json::json!({"rows":(0..width).map(|i|serde_json::json!({"name":format!("item-{i}"),"nested":{"value":i}})).collect::<Vec<_>>()}).to_string();
        for (name, source) in [
            (
                "strings_path",
                "rows.{'name':$uppercase(name),'length':$length(name),'value':nested.value+nested.value}",
            ),
            (
                "strings_callback",
                "$map(rows,function($r){{'name':$uppercase($r.name),'length':$length($r.name),'value':$r.nested.value+$r.nested.value}})",
            ),
        ] {
            let expression = jx::compile(source).unwrap();
            for iteration in 0..5 {
                let base = begin();
                let mut delivery = 0;
                expression
                    .evaluate(input.as_bytes())
                    .unwrap()
                    .for_each(|value| {
                        delivery = delivery.max(LIVE.load(Relaxed) - base);
                        black_box(value);
                    })
                    .unwrap();
                report(name, width, input.len(), iteration, base, delivery);
            }
        }
    }
}
