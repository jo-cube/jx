use jx::{ErrorKind, MAX_DEPTH, validate};

#[test]
fn valid_json_syntax_and_utf8() {
    for input in [
        "null",
        "true",
        "false",
        "0",
        "-0",
        "1.23E-4",
        "1e99999",
        "[]",
        "{}",
        " \n\r\t [1,{},true,false,null] \t",
        r#""\"\\\/\b\f\n\r\t\u0000""#,
        r#""é中文😀""#,
        r#""\ud800""#,
        r#""\udc00""#,
        r#"{"a":1,"a":2}"#,
    ] {
        assert!(validate(input.as_bytes()).is_ok(), "{input}");
    }
}

#[test]
fn invalid_json_syntax() {
    for input in [
        "",
        " ",
        "01",
        "-01",
        "-",
        "+1",
        ".1",
        "1.",
        "1e",
        "1e+",
        "NaN",
        "Infinity",
        "nul",
        "True",
        "false0",
        "true false",
        "[",
        "{",
        "[1,]",
        "[,1]",
        "[1 2]",
        "{,}",
        r#"{"a":}"#,
        r#"{"a":1,}"#,
        r#"{'a':1}"#,
        r#"{"a" 1}"#,
        r#""\x01""#,
        r#""\u123""#,
        r#""\uZZZZ""#,
        "\"unclosed",
        "\"\n\"",
        "\"\0\"",
        "\"\\",
        "\u{feff}null",
        "null\u{a0}",
        "/*x*/null",
        "[1]x",
    ] {
        assert_eq!(
            validate(input.as_bytes()).unwrap_err().kind,
            ErrorKind::InvalidJson,
            "{input:?}"
        );
    }
}

#[test]
fn rejects_invalid_utf8_anywhere_even_in_unselected_fields() {
    for input in [
        &b"\"\xff\""[..],
        &b"\"\xc0\x80\""[..],
        &b"\"\xed\xa0\x80\""[..],
        &b"\"\xf4\x90\x80\x80\""[..],
        &b"\"\xe2\x82\""[..],
        &b"{\"x\":1,\"y\":\"\xff\"}"[..],
    ] {
        assert_eq!(
            jx::compile("x").unwrap().evaluate(input).unwrap_err().kind,
            ErrorKind::InvalidJson
        );
    }
}

#[test]
fn depth_limit_and_large_shallow_records() {
    for depth in [MAX_DEPTH - 1, MAX_DEPTH, MAX_DEPTH + 1] {
        let input = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let result = validate(input.as_bytes());
        if depth <= MAX_DEPTH {
            assert!(result.is_ok());
        } else {
            assert_eq!(result.unwrap_err().kind, ErrorKind::DepthLimit);
        }
    }
    let input = format!(r#"{{"payload":"{}","id":7}}"#, "x".repeat(1024 * 1024));
    let mut selected = None;
    jx::compile("id")
        .unwrap()
        .evaluate(input.as_bytes())
        .unwrap()
        .for_each(|value| {
            assert!(selected.replace(value.as_raw().unwrap()).is_none());
        })
        .unwrap();
    assert_eq!(selected.unwrap().as_str(), "7");
}

#[test]
fn deterministic_mutations_agree_with_serde_on_common_json_domain() {
    // Seed covers JSON grammar and escaping. Both parsers must inspect trailing
    // bytes. Lone surrogates and unbounded exponents have separate policy tests.
    let seed = br#"{"a":[0,-12.5e+2,true,false,null,"x\n\u0041"],"b":{}}"#;
    let alphabet = b"{}[],:\"\\-+0129.etfnul \n\t\x00\xff";
    for at in 0..seed.len() {
        for &byte in alphabet {
            let mut input = seed.to_vec();
            input[at] = byte;
            assert_eq!(
                validate(&input).is_ok(),
                serde_json::from_slice::<serde_json::Value>(&input).is_ok(),
                "{input:?}"
            );
        }
        assert_eq!(
            validate(&seed[..at]).is_ok(),
            serde_json::from_slice::<serde_json::Value>(&seed[..at]).is_ok()
        );
    }
}

#[test]
fn planned_capture_preserves_complete_validation_and_exact_diagnostics() {
    let expressions = [
        "x*x+y*y+x",
        "active ? payload.x*payload.x+payload.y : 0",
        r#"{"n":payload.x*payload.y+1}"#,
        "$sum(payload.rows[x>0].(x*y+1))",
        r#"$lookup({"a":3},key)*x+x*x+x"#,
    ]
    .map(|source| jx::compile(source).unwrap());
    let seeds: &[&[u8]] = &[
        br#"{"active":false,"x":2,"y":3,"payload":{"x":2,"y":3,"rows":[{"x":2,"y":3}]},"key":"a","ignored":[true,null,"\u0041"]}"#,
        br#"{"payload":{"x":1},"payload":{"y":2},"x":1,"\u0078":2}"#,
    ];
    let check = |input: &[u8]| {
        if let Err(expected) = validate(input) {
            for expression in &expressions {
                assert_eq!(
                    expression.evaluate(input).unwrap_err(),
                    expected,
                    "{input:?}"
                );
            }
        }
    };
    for seed in seeds {
        for at in 0..seed.len() {
            for byte in b"{}[],:\"\\-+0129.etfnul \n\t\x00\xff" {
                let mut input = seed.to_vec();
                input[at] = *byte;
                check(&input);
            }
            check(&seed[..at]);
        }
        let mut trailing = seed.to_vec();
        trailing.extend_from_slice(b" false");
        check(&trailing);
    }
    for depth in [MAX_DEPTH - 1, MAX_DEPTH, MAX_DEPTH + 1] {
        check(
            format!(
                r#"{{"active":false,"ignored":{}0{}}}"#,
                "[".repeat(depth),
                "]".repeat(depth)
            )
            .as_bytes(),
        );
    }
}
