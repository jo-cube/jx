use jx::{ErrorKind, MAX_DEPTH, validate};
use std::io::{self, Write};

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
        "x&y",
        "{'x':x,'y':y}",
        "$map(payload.rows,function($r){$r.x&$r.y})",
        "payload.rows^(x).y",
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

#[test]
fn string_errors_preserve_exact_offsets_after_ordinary_runs() {
    let expressions = ["id", "id*id+id*id+id"].map(|source| jx::compile(source).unwrap());
    for length in (0..64).chain([127, 128, 129, 1023, 1024, 1025]) {
        let prefix = format!("\"é{}", "x".repeat(length));
        let at = prefix.len();
        let mut cases = vec![
            (r#"\q""#.to_owned(), 1, "invalid JSON string escape"),
            (
                r#"\u12x4""#.to_owned(),
                4,
                "expected four hexadecimal escape digits",
            ),
            (
                r#"\u12""#.to_owned(),
                4,
                "expected four hexadecimal escape digits",
            ),
            ("\\".to_owned(), 1, "invalid JSON string escape"),
            ("".to_owned(), 0, "unterminated JSON string"),
        ];
        for byte in 0..32 {
            cases.push((
                format!("{}\"", char::from(byte)),
                0,
                "unescaped control byte in string",
            ));
        }
        for (tail, relative, message) in cases {
            let input = format!("{prefix}{tail}");
            let error = validate(input.as_bytes()).unwrap_err();
            assert_eq!(error.kind, ErrorKind::InvalidJson, "{input:?}");
            assert_eq!(error.offset, at + relative, "{input:?}");
            assert_eq!(error.message, message, "{input:?}");
            for expression in &expressions {
                assert_eq!(expression.evaluate(input.as_bytes()).unwrap_err(), error);
            }
            if tail.ends_with('"') {
                let input = format!(r#"{{"v":{input},"padding":"{}"}}"#, "x".repeat(128));
                let error = validate(input.as_bytes()).unwrap_err();
                assert_eq!(error.offset, 5 + at + relative, "{input:?}");
                assert_eq!(error.message, message, "{input:?}");
                for expression in &expressions {
                    assert_eq!(expression.evaluate(input.as_bytes()).unwrap_err(), error);
                }
            }
        }
        for bytes in [
            b"\xff".as_slice(),
            b"\xc0\x80",
            b"\xed\xa0\x80",
            b"\xf4\x90\x80\x80",
            b"\xe2\x82",
        ] {
            let mut input = prefix.as_bytes().to_vec();
            input.extend_from_slice(bytes);
            input.push(b'"');
            let error = validate(&input).unwrap_err();
            assert_eq!(error.offset, at);
            assert_eq!(error.message, "invalid UTF-8");
            for expression in &expressions {
                assert_eq!(expression.evaluate(&input).unwrap_err(), error);
            }
        }
    }
}

#[test]
fn validated_tokens_preserve_exact_spelling() {
    for (input, expected) in [
        (
            r#"  { "a" : " x ", "a": "\u0061\/\ud800", "n": -0.00e+02, "v" : [ true , null ] }  "#,
            r#"{"a":" x ","a":"\u0061\/\ud800","n":-0.00e+02,"v":[true,null]}"#,
        ),
        (
            r#" "\"\\\b\f\n\r\t\u0000😀" "#,
            r#""\"\\\b\f\n\r\t\u0000😀""#,
        ),
        ("  -0.00e+02 \n", "-0.00e+02"),
        (" true ", "true"),
        (" false ", "false"),
        (" null ", "null"),
        (" 1e99999 ", "1e99999"),
        (" [ ] ", "[]"),
    ] {
        let mut out = vec![];
        jx::validate(input.as_bytes())
            .unwrap()
            .write_compact(&mut out)
            .unwrap();
        assert_eq!(out, expected.as_bytes());
    }
    for n in [0, 1, 6, 7, 8, 9, 31, 63, 64, 65, 127, 1024] {
        for suffix in ["", r#"\""#, r#"\\"#, r#"\ud800\udc00"#, " 😀 "] {
            let body = format!("{}{suffix}", "x".repeat(n));
            let input = format!(" {{ \"v\" : \"{body}\", \"a\" : [ \"{body}\" ] }} ");
            let expected = format!("{{\"v\":\"{body}\",\"a\":[\"{body}\"]}}");
            let mut out = vec![];
            jx::validate(input.as_bytes())
                .unwrap()
                .write_compact(&mut out)
                .unwrap();
            assert_eq!(out, expected.as_bytes());
        }
    }
}

struct FailAfter {
    bytes: Vec<u8>,
    remaining: usize,
}
impl Write for FailAfter {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if self.remaining == 0 {
            return Err(io::Error::other("stop"));
        }
        let n = b.len().min(self.remaining);
        self.bytes.extend_from_slice(&b[..n]);
        self.remaining -= n;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn writer_errors_leave_only_the_correct_prefix() {
    for (input, expected) in [
        (
            br#" { "a" : "a\"\\ long string with spaces", "b" : [ 1, 2 ] } "#.as_slice(),
            br#"{"a":"a\"\\ long string with spaces","b":[1,2]}"#.as_slice(),
        ),
        (
            br#" "a\"\\ scalar with spaces" "#,
            br#""a\"\\ scalar with spaces""#,
        ),
        (b" -0.00e+02 ", b"-0.00e+02"),
    ] {
        for limit in 0..expected.len() {
            let mut sink = FailAfter {
                bytes: vec![],
                remaining: limit,
            };
            assert!(
                jx::validate(input)
                    .unwrap()
                    .write_compact(&mut sink)
                    .is_err()
            );
            assert_eq!(sink.bytes, &expected[..limit]);
        }
    }
}
