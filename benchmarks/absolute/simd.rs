//! Handwritten, schema-restricted Rust over a fully validated reusable SIMD tape.
//! Benchmark reference only: no JSONata lowering, evaluator, or production integration.
#![allow(unsafe_code, dead_code)]
mod fixtures;
mod handwritten;
#[path = "simd_metrics.rs"]
mod metrics;
use fixtures::{Case, Kind};
use simd_json::{Node, StaticNode, Tape};
use std::{borrow::Cow, collections::HashMap, hint::black_box};
fn next(nodes: &[Node<'_>], at: usize) -> usize {
    match nodes[at] {
        Node::Object { count, .. } | Node::Array { count, .. } => at + count + 1,
        _ => at + 1,
    }
}
fn field(nodes: &[Node<'_>], object: usize, key: &str) -> Option<usize> {
    let Node::Object { len, .. } = nodes[object] else {
        return None;
    };
    let mut at = object + 1;
    for _ in 0..len {
        let Node::String(name) = nodes[at] else {
            panic!("key")
        };
        let value = at + 1;
        if name == key {
            return Some(value);
        }
        at = next(nodes, value)
    }
    None
}
fn num(nodes: &[Node<'_>], at: usize) -> f64 {
    match nodes[at] {
        Node::Static(StaticNode::I64(n)) => n as f64,
        Node::Static(StaticNode::U64(n)) => n as f64,
        Node::Static(StaticNode::F64(n)) => n,
        _ => panic!("numeric shape"),
    }
}
fn text<'a>(nodes: &[Node<'a>], at: usize) -> &'a str {
    match nodes[at] {
        Node::String(s) => s,
        _ => panic!("string shape"),
    }
}
fn row<'a>(nodes: &[Node<'a>], root: usize, width: usize) -> handwritten::Row<'a> {
    let mut row = handwritten::Row::default();
    let Node::Object { len, .. } = nodes[root] else {
        panic!("row object")
    };
    let mut at = root + 1;
    for _ in 0..len {
        let key = text(nodes, at);
        let value = at + 1;
        match key {
            "id" => row.id = Cow::Borrowed(text(nodes, value)),
            "name" => row.name = Cow::Borrowed(text(nodes, value)),
            "env" => row.env = Cow::Borrowed(text(nodes, value)),
            "deleted" => row.deleted = matches!(nodes[value], Node::Static(StaticNode::Bool(true))),
            "profile" => {
                row.region = Cow::Borrowed(text(nodes, field(nodes, value, "region").unwrap()))
            }
            "metrics" => {
                let Node::Object { len, .. } = nodes[value] else {
                    panic!("metrics")
                };
                let mut m = value + 1;
                for _ in 0..len {
                    let k = text(nodes, m);
                    if let Some(i) = k
                        .strip_prefix('v')
                        .and_then(|s| s.parse::<usize>().ok())
                        .filter(|i| *i < width)
                    {
                        row.metrics[i] = num(nodes, m + 1)
                    }
                    m = next(nodes, m + 1);
                }
            }
            _ => {}
        }
        at = next(nodes, value)
    }
    row
}
fn render(case: &Case, nodes: &[Node<'_>], labels: &HashMap<String, String>, out: &mut Vec<u8>) {
    if case.kind == Kind::Validate {
        return;
    }
    if matches!(case.kind, Kind::Fold | Kind::Filtered | Kind::Callback) {
        let root = field(nodes, 0, "items").unwrap();
        let Node::Array { len, .. } = nodes[root] else {
            panic!("items")
        };
        let mut at = root + 1;
        let mut sum = 0.;
        let mut count = 0;
        for _ in 0..len {
            let m = field(nodes, at, "metrics").unwrap();
            let a = num(nodes, field(nodes, m, "v0").unwrap());
            let b = num(nodes, field(nodes, m, "v1").unwrap());
            if case.kind == Kind::Fold || a > 0. {
                if case.kind == Kind::Callback {
                    if count == 0 {
                        out.push(b'[')
                    } else {
                        out.push(b',')
                    };
                    out.extend_from_slice(b"{\"id\":");
                    serde_json::to_writer(&mut *out, text(nodes, field(nodes, at, "id").unwrap()))
                        .unwrap();
                    out.extend_from_slice(b",\"x\":");
                    handwritten::number(out, a * b + a);
                    out.push(b'}');
                } else {
                    sum += a * b + a;
                }
                count += 1;
            }
            at = next(nodes, at);
        }
        if case.kind == Kind::Callback {
            if count == 1 {
                out.remove(0);
            } else if count > 1 {
                out.push(b']')
            }
        } else if count > 0 {
            handwritten::number(out, sum)
        }
    } else if case.kind == Kind::Nested {
        let depth = case
            .name
            .rsplit('/')
            .next()
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let mut at = 0;
        for _ in 0..depth {
            at = field(nodes, at, "n").unwrap()
        }
        at = field(nodes, at, "a").unwrap();
        let x = num(nodes, field(nodes, at, "x").unwrap());
        let y = num(nodes, field(nodes, at, "y").unwrap());
        handwritten::number(out, x * y + x);
    } else {
        handwritten::render(case, &row(nodes, 0, case.width.max(2)), labels, out)
    }
}
fn check(case: &Case, options: &jx::CompileOptions, labels: &HashMap<String, String>) {
    let expr = options.compile(&case.source).unwrap();
    let program = options.compile_program(&case.source).unwrap();
    let mut acquire = jx::execution::Acquisition::new([&program]);
    for input in case.inputs.iter().flatten() {
        let mut bytes = input.clone();
        let tape = simd_json::to_tape(&mut bytes).unwrap();
        let mut a = vec![];
        render(case, &tape.0, labels, &mut a);
        if case.kind == Kind::Validate {
            continue;
        }
        let mut b = vec![];
        program
            .write_json(&acquire.prepare(input).unwrap(), &mut b)
            .unwrap();
        let mut n = 0;
        let mut c = vec![];
        expr.evaluate(input)
            .unwrap()
            .for_each(|v| {
                if n == 1 {
                    c.insert(0, b'[')
                }
                if n > 0 {
                    c.push(b',')
                }
                v.write_compact(&mut c).unwrap();
                n += 1;
            })
            .unwrap();
        if n > 1 {
            c.push(b']')
        }
        let json = |s: &[u8]| {
            if s.is_empty() {
                None
            } else {
                Some(serde_json::from_slice::<serde_json::Value>(s).unwrap())
            }
        };
        assert_eq!(json(&a), json(&b), "SIMD {}", case.name);
        assert_eq!(json(&b), json(&c));
        assert_eq!(a.len(), b.len());
    }
}
fn run(case: &Case, labels: &HashMap<String, String>, mode: &str) {
    let expected = case
        .inputs
        .iter()
        .map(|input| {
            if let Some(input) = input {
                let mut out = vec![];
                handwritten::run(case, input, labels, &mut out, true);
                let tag = if case.worker && case.kind == Kind::Project {
                    let r = handwritten::parse(input, case.width, true);
                    if r.env != "production" {
                        1
                    } else if r.deleted {
                        2
                    } else {
                        3
                    }
                } else {
                    3
                };
                (tag, if tag == 3 { out } else { vec![] })
            } else {
                (2, vec![])
            }
        })
        .collect::<Vec<_>>();
    let before = metrics::live_bytes();
    let mut buffers = simd_json::Buffers::default();
    let mut tape = Some(Tape::null());
    let mut input = Vec::with_capacity(case.inputs.iter().flatten().map(Vec::len).max().unwrap());
    let mut output = Vec::with_capacity(if case.worker { 0 } else { 4096 });
    let mut scratch = Vec::new();
    let mut i = 0;
    metrics::measure(
        &case.name,
        mode,
        if case.worker { "worker" } else { "full" },
        case.inputs.iter().flatten().map(Vec::len).sum::<usize>() as f64 / case.inputs.len() as f64,
        || {
            let record = &case.inputs[i % case.inputs.len()];
            i += 1;
            input.clear();
            output.clear();
            let Some(record) = record else {
                black_box(2usize);
                return 0;
            };
            input.extend_from_slice(record);
            let mut t = tape.take().unwrap().reset();
            let parse_input = if case.worker && mode == "simd-copy" {
                scratch.clear();
                scratch.extend_from_slice(&input);
                &mut scratch
            } else {
                &mut input
            };
            simd_json::fill_tape(parse_input, &mut buffers, &mut t).unwrap();
            let project = if case.worker && case.kind == Kind::Project {
                let r = row(&t.0, 0, case.width);
                if r.env != "production" {
                    1
                } else if r.deleted {
                    2
                } else {
                    handwritten::render(case, &r, labels, &mut output);
                    3
                }
            } else {
                render(case, &t.0, labels, &mut output);
                3
            };
            tape = Some(t.reset());
            if i <= expected.len() {
                assert_eq!(project, expected[i - 1].0);
                let got = if output.is_empty() {
                    None
                } else {
                    Some(serde_json::from_slice::<serde_json::Value>(&output).unwrap())
                };
                let want = if expected[i - 1].1.is_empty() {
                    None
                } else {
                    Some(serde_json::from_slice::<serde_json::Value>(&expected[i - 1].1).unwrap())
                };
                assert_eq!(got, want);
            }
            black_box(project);
            if project != 3 {
                return 0;
            }
            if case.worker {
                // Same owned input/output replacement and 16 KiB recycling cap as jkq.
                input.clear();
                if input.capacity() <= 16384 {
                    std::mem::swap(&mut output, &mut input)
                } else {
                    input = std::mem::take(&mut output)
                }
                let n = input.len();
                black_box(&input);
                n
            } else {
                black_box(&output);
                output.len()
            }
        },
    );
    let retained = metrics::live_bytes() - before;
    let tape_b = tape.as_ref().unwrap().0.capacity() * std::mem::size_of::<Node<'_>>();
    eprintln!(
        "retained,{},{},{},{},{},{},{}",
        case.name,
        mode,
        retained,
        input.capacity() + output.capacity(),
        scratch.capacity(),
        tape_b,
        retained - (input.capacity() + output.capacity() + scratch.capacity()) as i64
    );
}
fn main() {
    eprintln!("SIMD reference");
    if std::env::var_os("ABS_POLICIES").is_some() {
        let mut probes = vec![
            b"{\"unused\":\"\xff\"}".to_vec(),
            b"{\"unused\":\"\\uD800\"}".to_vec(),
            b"1e9999".to_vec(),
            b"{} false".to_vec(),
            b"{\"a\":[1,]}".to_vec(),
            b"{\"a\":1,\"a\":2}".to_vec(),
        ];
        probes.push(format!("{}0{}", "[".repeat(200), "]".repeat(200)).into_bytes());
        for (i, original) in probes.iter().enumerate() {
            let mut bytes = original.clone();
            println!(
                "{}",
                serde_json::json!({"probe":i,"jx":jx::validate(original).is_ok(),"simd":simd_json::to_tape(&mut bytes).is_ok()})
            );
        }
        return;
    }

    let labels = (0..4096)
        .map(|i| (format!("k{i}"), format!("label-{i}")))
        .collect::<HashMap<_, _>>();
    let vars = serde_json::to_vec(&serde_json::json!({"labels":labels})).unwrap();
    let options = jx::CompileOptions::default()
        .constant_binding("vars", jx::OwnedValue::from_json(&vars).unwrap());
    println!(
        "case,mode,phase,sample,records,wall_seconds,cpu_seconds,input_bytes,output_bytes,allocations,allocated_bytes,peak_heap,probe_output_bytes"
    );
    let mode = std::env::var("ABS_MODE").unwrap_or_else(|_| "simd".into());
    assert!(["simd", "simd-copy"].contains(&mode.as_str()));
    for case in fixtures::cases() {
        check(&case, &options, &labels);
        if std::env::var_os("ABS_CHECK").is_some() {
            eprintln!("checked,{}", case.name)
        } else {
            run(&case, &labels, &mode)
        }
    }
}
