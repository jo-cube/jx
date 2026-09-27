use super::*;

// The tree is the diagnostic fallback and must agree even on IEEE edge cases.
fn result(node: &Node, input: &[u8]) -> Result<Vec<(u8, u64)>, crate::Error> {
    let mut items = Vec::new();
    crate::evaluate::scalar(node, input)?.for_each(|value| {
        items.push(match value {
            Value::Number(n) => (0, n.to_bits()),
            Value::Boolean(b) => (1, u64::from(b)),
            _ => panic!("numeric expression returned {value:?}"),
        });
    })?;
    Ok(items)
}

#[test]
fn plan_and_tree_agree_on_values_and_exact_errors() {
    let atoms = [
        "null", "false", "0", "-0", "2", "-3.5", "1e308", "1e999", "1e-320", "[]", "[1]", "[1,2]",
        "{}", "\"x\"",
    ];
    for source in [
        "((x+y)*(x-y)+(x*x+y*y))/(x+1)-y*3",
        "-(x*x+x)-y",
        "(x+x+x)%y",
        "(x+x+x)/y",
        "(x+x+x)<y",
        "(x+x+x)>=y",
        "(x+x+x)/0<y",
        "(x-x+x-x)/0<y",
        "((x<y)<x)<y",
        "missing+missing+missing+x",
        "((x+x)+x)+(y/y+y)",
        "(x-x+x-x)/0",
        "x+x+$.x+$.x",
    ] {
        let expression = crate::compile(source).unwrap();
        let Kind::Plan(plan) = &expression.root.kind else {
            panic!("expected a numeric region: {source}");
        };
        for x in atoms {
            for y in atoms {
                let object = format!(r#"{{"x":{x},"y":{y}}}"#);
                for input in [
                    object.clone(),
                    format!("[{object}]"),
                    format!("[{object},{object}]"),
                    format!(r#"{{"y":{y}}}"#),
                ] {
                    assert_eq!(
                        result(&expression.root, input.as_bytes()),
                        result(&plan.source, input.as_bytes()),
                        "{source}: {input}"
                    );
                }
            }
        }
    }
}
