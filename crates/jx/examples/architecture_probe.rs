use jx::{CompileOptions, OwnedValue};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn result(value: Result<jx::Evaluation<'_, '_>, jx::Error>) -> Value {
    match value.and_then(|v| v.collect_owned()) {
        Ok(items) => {
            json!({"items_json":format!("[{}]",items.iter().map(|v|{let mut b=Vec::new();v.as_value().write_compact(&mut b).unwrap();String::from_utf8(b).unwrap()}).collect::<Vec<_>>().join(","))})
        }
        Err(e) => json!({"error":format!("{:?}",e.kind),"offset":e.offset,"message":e.message}),
    }
}
fn complete_reference(evaluation: Result<jx::Evaluation<'_, '_>, jx::Error>) -> Value {
    let mut output = Vec::new();
    let mut first = None;
    let mut many = false;
    let result = evaluation
        .map_err(jx::ConsumeError::Evaluation)
        .and_then(|e| {
            e.try_for_each(|value| {
                if many {
                    output.push(b',');
                } else if let Some(first) = first.take() {
                    many = true;
                    output.push(b'[');
                    jx::Value::write_compact(&first, &mut output)?;
                    output.push(b',');
                } else {
                    first = Some(value);
                    return Ok(());
                }
                value.write_compact(&mut output)
            })
        });
    match result {
        Ok(()) => {
            let present = many || first.is_some();
            if many {
                output.push(b']');
            } else if let Some(value) = first {
                value.write_compact(&mut output).unwrap();
            }
            json!({"present":present,"json":String::from_utf8(output).unwrap()})
        }
        Err(jx::ConsumeError::Evaluation(e)) => {
            json!({"error":format!("{:?}",e.kind),"offset":e.offset,"message":e.message})
        }
        Err(jx::ConsumeError::Consumer(e)) => json!({"serialization":e.to_string()}),
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let source = request["expr"].as_str().unwrap();
        let input = request["input"].as_str().unwrap().as_bytes();
        let mut options = CompileOptions::default();
        if let Some(v) = request.get("vars") {
            options = options.constant_binding(
                "vars",
                OwnedValue::from_json(serde_json::to_string(v).unwrap().as_bytes()).unwrap(),
            );
        }
        if let Some(bindings) = request["bindings"].as_object() {
            for (name, value) in bindings {
                options = options.constant_binding(
                    name.clone(),
                    OwnedValue::from_json(serde_json::to_string(value).unwrap().as_bytes())
                        .unwrap(),
                );
            }
        }
        let compiled = options.compile_program(source);
        if request["coverage_only"] == true
            && let Err(e) = &compiled
        {
            println!(
                "{}",
                json!({"unsupported":e.to_string(),"kind":format!("{:?}",e.kind)})
            );
            continue;
        }
        let expression = match options.compile(source) {
            Ok(e) => e,
            Err(e) => {
                println!("{}", json!({"compile_error":format!("{:?}",e.kind)}));
                continue;
            }
        };
        if request["absent_input"] == true {
            let original = result(expression.evaluate_with(None, Default::default()));
            let answer = match compiled {
                Err(e) => json!({"unsupported":e.to_string(),"engine":original}),
                Ok(p) => json!({
                    "engine":original,
                    "prototype":[result(p.evaluate_without_input())],
                    "complete":complete_reference(p.evaluate_without_input()),
                    "engine_complete":complete_reference(expression.evaluate_with(None, Default::default()))
                }),
            };
            println!("{answer}");
            continue;
        }
        let original = result(expression.evaluate(input));
        let answer = match compiled {
            Err(e) => json!({"unsupported":e.to_string(),"engine":original}),
            Ok(p) => {
                let mut results = vec![result(p.evaluate_bytes(input))];
                let mut a = jx::execution::Acquisition::new([&p]);
                let mut items = Vec::new();
                let result = a
                    .prepare(input)
                    .map_err(jx::ConsumeError::Evaluation)
                    .and_then(|d| {
                        p.try_for_each(&d, |v| {
                            let mut bytes = Vec::new();
                            v.to_owned()
                                .unwrap()
                                .as_value()
                                .write_compact(&mut bytes)
                                .unwrap();
                            items.push(String::from_utf8(bytes).unwrap());
                            Ok::<_, ()>(())
                        })
                    });
                results.push(match result {
                    Ok(()) => json!({"items_json":format!("[{}]",items.join(","))}),
                    Err(jx::ConsumeError::Evaluation(e)) => json!({"error":format!("{:?}",e.kind),"offset":e.offset,"message":e.message}),
                    Err(jx::ConsumeError::Consumer(())) => unreachable!(),
                });
                let mut bytes = b"prefix".to_vec();
                let complete = a
                    .prepare(input)
                    .map_err(jx::ConsumeError::Evaluation)
                    .and_then(|d| p.write_json(&d, &mut bytes));
                let complete = match complete {
                    Ok(present) => {
                        json!({"present":present,"json":String::from_utf8(bytes[6..].to_vec()).unwrap()})
                    }
                    Err(jx::ConsumeError::Evaluation(e)) => {
                        assert_eq!(bytes, b"prefix");
                        json!({"error":format!("{:?}",e.kind),"offset":e.offset,"message":e.message})
                    }
                    Err(jx::ConsumeError::Consumer(e)) => panic!("{source}: {e}"),
                };
                json!({"engine":original,"prototype":results,"complete":complete,"engine_complete":complete_reference(expression.evaluate(input))})
            }
        };
        println!("{answer}");
    }
}
