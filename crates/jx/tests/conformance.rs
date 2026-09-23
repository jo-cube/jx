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
    let mut counts = [0; 2];
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
        let source = case["expr"].as_str().unwrap();
        let compiled = jx::compile(source);
        match row["status"].as_str().unwrap() {
            "supported" => {
                counts[0] += 1;
                if let Some(bindings) = case.get("bindings") {
                    assert!(
                        bindings.as_object().unwrap().is_empty(),
                        "unhandled bindings: {id}"
                    );
                }
                let data = match case.get("data") {
                    Some(data) => data.clone(),
                    None => read(
                        &root
                            .join("datasets")
                            .join(format!("{}.json", case["dataset"].as_str().unwrap())),
                    ),
                };
                let input = serde_json::to_vec(&data).unwrap();
                let mut values: Vec<Value> = Vec::new();
                compiled
                    .unwrap()
                    .evaluate(&input)
                    .unwrap()
                    .for_each(|value| {
                        values.push(serde_json::from_slice(value.as_bytes()).unwrap());
                    });
                if case.get("undefinedResult").is_some() {
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
                    assert_eq!(actual, case["result"], "{id}: {source}");
                }
            }
            "syntax" => {
                counts[1] += 1;
                assert_eq!(
                    compiled.unwrap_err().kind,
                    jx::ErrorKind::UnsupportedExpression,
                    "{id}"
                );
            }
            status => panic!("unrecognized status {status}: {id}"),
        }
    }
    let mut disk = BTreeSet::new();
    for group in fs::read_dir(root.join("groups")).unwrap() {
        for file in fs::read_dir(group.unwrap().path()).unwrap() {
            let path = file.unwrap().path();
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
        "JSONata {}: {} supported, {} deferred syntax; no skipped cases",
        manifest["revision"].as_str().unwrap(),
        counts[0],
        counts[1]
    );
}
