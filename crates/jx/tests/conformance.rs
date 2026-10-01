use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path};

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn pinned_upstream_groups_have_explicit_expected_outcomes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/conformance");
    let manifest = read(&root.join("manifest.json"));
    let mut seen = BTreeSet::new();
    let mut counts = [0; 4];
    for row in manifest["cases"].as_array().unwrap() {
        let file = row["file"].as_str().unwrap();
        let index = row["index"].as_u64().unwrap() as usize;
        let id = format!("{file}#{index}");
        assert!(seen.insert(id.clone()), "duplicate manifest case: {id}");
        assert!(!row["reason"].as_str().unwrap().is_empty());
        let spec = read(&root.join(file));
        let case = if spec.is_array() {
            &spec[index]
        } else {
            assert_eq!(index, 0);
            &spec
        };
        let expression_file;
        let source = if let Some(source) = case["expr"].as_str() {
            source
        } else {
            expression_file = fs::read_to_string(
                root.join(file)
                    .with_file_name(case["expr-file"].as_str().unwrap()),
            )
            .unwrap();
            &expression_file
        };
        let syntax_source = source;
        // Upstream host-provided JSON bindings become lexical declarations. This
        // exercises the same values without introducing an embedding API.
        let adapted;
        let source = if let Some(bindings) = case["bindings"].as_object().filter(|b| !b.is_empty())
        {
            adapted = format!(
                "({}{} )",
                bindings
                    .iter()
                    .map(|(name, value)| format!("${name}:={value};"))
                    .collect::<String>(),
                source
            );
            &adapted
        } else {
            source
        };
        // The byte-input API always has a JSON root. Upstream's absent host
        // input is represented by an undefined predicate candidate instead.
        let absent_input;
        let source = if case.get("data").is_none() && case["dataset"].is_null() {
            assert!(!source.contains("__jx_conformance"));
            absent_input = format!(
                "($$:=();$__jx_conformance_result:=();$__jx_conformance_missing[$__jx_conformance_result:=({source})];$__jx_conformance_result)"
            );
            &absent_input
        } else {
            source
        };
        let compiled = jx::compile(if row["phase"] == "compile" {
            syntax_source
        } else {
            source
        });
        match row["status"].as_str().unwrap() {
            "supported" => {
                counts[0] += 1;
                let input = input_bytes(&root, case, file, index);
                let mut values: Vec<Value> = Vec::new();
                compiled
                    .unwrap_or_else(|error| panic!("{id}: {source}: {error}"))
                    .evaluate(&input)
                    .unwrap_or_else(|error| panic!("{id}: {source}: {error}"))
                    .for_each(|value| {
                        let mut bytes = Vec::new();
                        value.write_compact(&mut bytes).unwrap();
                        values.push(serde_json::from_slice(&bytes).unwrap());
                    })
                    .unwrap_or_else(|error| panic!("{id}: {source}: {error}"));
                if case["undefinedResult"] == true {
                    assert!(values.is_empty(), "{id}");
                } else {
                    assert!(
                        !values.is_empty(),
                        "missing instead of a defined result: {id}"
                    );
                    let actual = if values.len() == 1 {
                        values.pop().unwrap()
                    } else {
                        Value::Array(values)
                    };
                    let mut expected = case["result"].clone();
                    let mut actual = actual;
                    if case["unordered"] == true {
                        actual
                            .as_array_mut()
                            .expect("unordered array")
                            .sort_by_cached_key(Value::to_string);
                        expected
                            .as_array_mut()
                            .expect("unordered array")
                            .sort_by_cached_key(Value::to_string);
                    }
                    assert!(
                        same_json(&actual, &expected),
                        "{id}: {source}: {actual} != {expected}"
                    );
                }
            }
            status @ ("error" | "limit" | "blocked") => {
                counts[match status {
                    "error" => 1,
                    "limit" => 2,
                    _ => 3,
                }] += 1;
                let error = match row["phase"].as_str().unwrap() {
                    "compile" => compiled.unwrap_err(),
                    "evaluate" => compiled
                        .unwrap()
                        .evaluate(&input_bytes(&root, case, file, index))
                        .and_then(|result| result.for_each(|_| {}))
                        .err()
                        .unwrap_or_else(|| panic!("expected error: {id}: {source}")),
                    phase => panic!("unknown error phase: {phase}"),
                };
                if let Some(message) = row["message"].as_str() {
                    assert_eq!(error.message, message, "{id}");
                }
                assert_eq!(
                    format!("{:?}", error.kind),
                    row["kind"].as_str().unwrap(),
                    "{id}: {source}"
                );
            }
            status => panic!("unrecognized status {status}: {id}"),
        }
    }
    assert_eq!(
        seen.len(),
        manifest["corpus_cases"].as_u64().unwrap() as usize
    );
    let mut disk = BTreeSet::new();
    for group in fs::read_dir(root.join("groups")).unwrap() {
        for file in fs::read_dir(group.unwrap().path()).unwrap() {
            let path = file.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "jsonata")
            {
                continue;
            }
            let spec = read(&path);
            let count = spec.as_array().map_or(1, Vec::len);
            for index in 0..count {
                disk.insert(format!(
                    "{}#{index}",
                    path.strip_prefix(&root).unwrap().to_str().unwrap()
                ));
            }
        }
    }
    assert_eq!(
        seen, disk,
        "every imported case must be classified and executed"
    );
    println!(
        "JSONata {}: {} supported results, {} supported errors, {} recursion limits, {} blocked compatibility cases; no skipped cases",
        manifest["revision"].as_str().unwrap(),
        counts[0],
        counts[1],
        counts[2],
        counts[3]
    );
}

// Keep source object order: $keys and iteration expose it semantically.
fn input_bytes(root: &Path, case: &Value, file: &str, index: usize) -> Vec<u8> {
    if let Some(dataset) = case["dataset"].as_str() {
        return fs::read(root.join("datasets").join(format!("{dataset}.json"))).unwrap();
    }
    let bytes = fs::read(root.join(file)).unwrap();
    type Raw = Box<serde_json::value::RawValue>;
    let raw = if bytes.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'[') {
        let mut cases: Vec<Raw> = serde_json::from_slice(&bytes).unwrap();
        cases.swap_remove(index)
    } else {
        serde_json::from_slice::<Raw>(&bytes).unwrap()
    };
    let fields: std::collections::BTreeMap<String, Raw> = serde_json::from_str(raw.get()).unwrap();
    fields
        .get("data")
        .map_or_else(|| b"null".to_vec(), |data| data.get().as_bytes().to_vec())
}

// JSONata uses binary64; fixture exponent notation must not change equality.
fn same_json(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => a.as_f64() == b.as_f64(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same_json(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(key, a)| b.get(key).is_some_and(|b| same_json(a, b)))
        }
        _ => left == right,
    }
}
