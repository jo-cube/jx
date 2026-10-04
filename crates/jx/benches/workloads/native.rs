use super::measure_allocations;
use std::{hint::black_box, time::Instant};

const ARITHMETIC: &str = "((x+y)*(x-y)+(x*x+y*y))/(x+1)-y*3";
const DENSE: &str = "((x*y+x-y)*(x+y)+x*x+y*y)/(x+1)+((x-y)*(x-y)+y)/(y+1)";
fn arithmetic(x: f64, y: f64) -> f64 {
    ((x + y) * (x - y) + (x * x + y * y)) / (x + 1.0) - y * 3.0
}
fn dense(x: f64, y: f64) -> f64 {
    ((x * y + x - y) * (x + y) + x * x + y * y) / (x + 1.0) + ((x - y) * (x - y) + y) / (y + 1.0)
}
fn fields(raw: &str) -> (f64, f64) {
    let x = raw
        .split_once("\"x\":")
        .unwrap()
        .1
        .split(',')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let y = raw
        .split_once("\"y\":")
        .unwrap()
        .1
        .split([',', '}'])
        .next()
        .unwrap()
        .parse()
        .unwrap();
    (x, y)
}
fn compiled(source: &str) -> jx::Expression {
    let mut expression = jx::compile(source).unwrap();
    if std::env::var_os("JX_BENCH_NATIVE").is_some() {
        let stats = expression.enable_native();
        assert_eq!(stats.failures, 0);
        assert!(stats.kernels > 0, "not compiled: {source}");
    }
    expression
}
fn workload(name: &str, source: &str, input: &[u8], smoke: bool) {
    let expression = compiled(source);
    let mut metadata = jx::compile(source).unwrap();
    let start = Instant::now();
    let stats = metadata.enable_native();
    eprintln!(
        "native_region,{name},{},{},{},{}",
        input.len(),
        start.elapsed().as_nanos(),
        stats.kernels,
        stats.code_bytes
    );
    let interpreted = jx::compile(source).unwrap();
    let result = |e: &jx::Expression| {
        let mut output = Vec::new();
        e.evaluate(input)
            .unwrap()
            .for_each(|v| v.write_compact(&mut output).unwrap())
            .unwrap();
        output
    };
    assert_eq!(result(&expression), result(&interpreted), "{name}");
    measure_allocations(name, input.len(), smoke, Some(0), || {
        expression
            .evaluate(black_box(input))
            .unwrap()
            .for_each(|v| {
                black_box(v);
            })
            .unwrap();
    });
}
pub(super) fn run(smoke: bool) {
    for (name, source) in [
        ("arithmetic", ARITHMETIC),
        ("dense", DENSE),
        (
            "branch",
            "x>y ? ((x+y)*(x-y)+x*x+y*y)/(x+1) : (x-y)*(y+1)+x*x",
        ),
    ] {
        let start = Instant::now();
        let mut expr = jx::compile(source).unwrap();
        let parsed = start.elapsed();
        let start = Instant::now();
        let stats = expr.enable_native();
        let native = start.elapsed();
        eprintln!(
            "native_metadata,{name},{},{},{},{},{}",
            parsed.as_nanos(),
            native.as_nanos(),
            stats.kernels,
            stats.code_bytes,
            stats.failures
        );
        measure_allocations(&format!("jit/compile_{name}"), 0, smoke, None, || {
            black_box(compiled(black_box(source)));
        });
        for size in [100, 500, 1024, 10240, 1048576] {
            let base = r#"{"x":7,"y":3,"padding":""}"#;
            let input = base.replace(
                "\"padding\":\"\"",
                &format!("\"padding\":\"{}\"", "x".repeat(size - base.len())),
            );
            workload(&format!("jit/{name}"), source, input.as_bytes(), smoke);
            measure_allocations(
                &format!("jit/rust_{name}"),
                input.len(),
                smoke,
                Some(0),
                || {
                    let raw = jx::validate(black_box(input.as_bytes())).unwrap();
                    let (x, y) = fields(raw.as_str());
                    black_box(match name {
                        "arithmetic" => arithmetic(x, y),
                        "dense" => dense(x, y),
                        _ => {
                            if x > y {
                                ((x + y) * (x - y) + x * x + y * y) / (x + 1.0)
                            } else {
                                (x - y) * (y + 1.0) + x * x
                            }
                        }
                    });
                },
            );
        }
    }
    let source = "x>y ? (x>10 ? (x*x+y*y)/(x+1) : (x+y)*(x-y)) : ((x-y)*(y+1)+x*x)";
    let expression = compiled(source);
    let interpreted = jx::compile(source).unwrap();
    let inputs = (0..32)
        .map(|i| {
            let base = format!(
                r#"{{"x":{},"y":{},"padding":""}}"#,
                i * 17 % 19 - 3,
                i * 13 % 17
            );
            base.replace(
                "\"padding\":\"\"",
                &format!("\"padding\":\"{}\"", "x".repeat(500 - base.len())),
            )
        })
        .collect::<Vec<_>>();
    for input in &inputs {
        let snapshot = |e: &jx::Expression| {
            let mut bytes = Vec::new();
            e.evaluate(input.as_bytes())
                .unwrap()
                .for_each(|v| v.write_compact(&mut bytes).unwrap())
                .unwrap();
            bytes
        };
        assert_eq!(snapshot(&expression), snapshot(&interpreted));
    }
    let mut at = 0;
    measure_allocations("jit/varying_branch", 500, smoke, Some(0), || {
        expression
            .evaluate(black_box(inputs[at].as_bytes()))
            .unwrap()
            .for_each(|v| {
                black_box(v);
            })
            .unwrap();
        at = (at + 1) % inputs.len();
    });
    workload("jit/tiny", ARITHMETIC, br#"{"x":7,"y":3}"#, smoke);
    for width in [8, 32, 128, 1024, 16384] {
        let rows = (0..width)
            .map(|i| format!(r#"{{"x":{},"y":{}}}"#, i % 16 + 1, i % 7 + 1))
            .collect::<Vec<_>>()
            .join(",");
        let input = format!(r#"{{"rows":[{rows}]}}"#);
        workload(
            "jit/fold",
            &format!("$sum(rows.({DENSE}))"),
            input.as_bytes(),
            smoke,
        );
        workload(
            "jit/filter_map_fold",
            &format!(
                "$sum($map($filter(rows,function($r){{($r.x*2+$r.y)>12 and $r.y<6}}),function($r){{{}}}))",
                ARITHMETIC.replace('x', "$r.x").replace('y', "$r.y")
            ),
            input.as_bytes(),
            smoke,
        );
        let select = jx::compile("rows").unwrap();
        measure_allocations("jit/rust_fold", input.len(), smoke, Some(0), || {
            let mut total = 0.0;
            select
                .evaluate(black_box(input.as_bytes()))
                .unwrap()
                .for_each(|v| {
                    for row in v.as_raw().unwrap().as_str().split('{').skip(1) {
                        let (x, y) = fields(row.split('}').next().unwrap());
                        total += dense(x, y);
                    }
                })
                .unwrap();
            black_box(total);
        });
        let values = (0..width)
            .map(|i| (i % 16 + 1).to_string())
            .collect::<Vec<_>>()
            .join(",");
        let input = format!(r#"{{"rows":[{values}]}}"#);
        workload(
            "jit/numeric_fold",
            "$sum(rows.(($*$+$+1)/($+1)+($*$-$)/($+2)))",
            input.as_bytes(),
            smoke,
        );
    }
}
