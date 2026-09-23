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
    let mut counts = [0; 3];
    for row in manifest["cases"].as_array().unwrap() {
        let file = row["file"].as_str().unwrap();
        assert!(
            seen.insert(file.to_owned()),
            "duplicate manifest case: {file}"
        );
        assert!(!row["reason"].as_str().unwrap().is_empty());
        let case = read(&root.join(file));
        let expression = case["expr"].as_str().unwrap();
        let input = read(
            &root
                .join("datasets")
                .join(format!("{}.json", case["dataset"].as_str().unwrap())),
        );
        assert!(
            case["bindings"].as_object().unwrap().is_empty(),
            "unhandled bindings: {file}"
        );
        let input = serde_json::to_vec(&input).unwrap();
        let compiled = jx::compile(expression);
        match row["status"].as_str().unwrap() {
            "supported" => {
                counts[0] += 1;
                let values: Vec<_> = compiled.unwrap().evaluate(&input).unwrap().collect();
                if case.get("undefinedResult").is_some() {
                    assert!(values.is_empty(), "{file}");
                } else {
                    assert_eq!(values.len(), 1, "{file}");
                    let actual: Value = serde_json::from_slice(values[0].as_bytes()).unwrap();
                    assert_eq!(actual, case["result"], "{file}: {expression}");
                }
            }
            "array" => {
                counts[1] += 1;
                assert_eq!(
                    compiled.unwrap().evaluate(&input).unwrap_err().kind,
                    jx::ErrorKind::ArrayTraversal,
                    "{file}"
                );
            }
            "syntax" => {
                counts[2] += 1;
                assert_eq!(
                    compiled.unwrap_err().kind,
                    jx::ErrorKind::UnsupportedExpression,
                    "{file}"
                );
            }
            status => panic!("unrecognized status {status}: {file}"),
        }
    }
    let mut disk = BTreeSet::new();
    for group in fs::read_dir(root.join("groups")).unwrap() {
        let group = group.unwrap();
        for file in fs::read_dir(group.path()).unwrap() {
            let path = file.unwrap().path();
            disk.insert(
                path.strip_prefix(&root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned(),
            );
        }
    }
    assert_eq!(
        seen, disk,
        "every imported case must be classified and executed"
    );
    println!(
        "JSONata {}: {} supported, {} deferred array mapping, {} deferred syntax; no skipped cases",
        manifest["revision"].as_str().unwrap(),
        counts[0],
        counts[1],
        counts[2]
    );
}
