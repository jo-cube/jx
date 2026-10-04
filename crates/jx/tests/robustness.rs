#[path = "support/robustness.rs"]
mod common;
use jx::{CompileOptions, ErrorKind, EvaluationOptions, HostFunction, Limits, Value};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

struct Random(u64);
impl Random {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as usize
    }
}
#[test]
fn seeded_parser_scanner_and_serialization_mutations() {
    let seeds: &[&[u8]] = &[
        r#"{"a":2,"b":3,"flag":true,"rows":[{"n":1},{"n":2}],"date":"1970年01月01日","picture":"[Y]年[M]月[D]日"}"#.as_bytes(),
        r#"{"a":{"b":"\ud800"},"a":{"b":"😀"},"ignored":[true,null,"\u0041"]}"#.as_bytes(),
        b"function($x)<a<n>:n>{$sum($x)}(rows.n)", b"a#$i^(n)[`@`.n>0].$i",
        b"($f:=function($x){$x=0?0:$f($x-1)};$f(4))", b"$formatNumber(a,picture)",
        b"($f:=$eval('function($n){function(){$n}}');$f(a)())", b"/([a-z]+)\\1/i",
    ];
    let mut random = Random(0x29_c0ffee);
    let iterations = std::env::var("JX_FUZZ_ITERATIONS")
        .map(|n| n.parse().unwrap())
        .unwrap_or(4096);
    for seed in seeds {
        common::exercise(seed);
    }
    for _ in 0..iterations {
        let mut input = seeds[random.next() % seeds.len()].to_vec();
        for _ in 0..1 + random.next() % 4 {
            let at = random.next() % (input.len() + 1);
            match random.next() % 3 {
                0 if at < input.len() => {
                    input.remove(at);
                }
                1 if at < input.len() => {
                    input[at] = random.next() as u8;
                }
                _ => input.insert(at, random.next() as u8),
            }
        }
        common::exercise(&input);
    }
}

#[test]
fn fallback_and_shared_expression_ownership_survive_adversarial_shapes() {
    let atoms = [
        "null",
        "false",
        "true",
        "-0",
        "1e999",
        "2",
        "[]",
        "[1]",
        "[1,2]",
        "{}",
        "\"é\\ud800\"",
    ];
    for source in [&common::EXPRESSIONS[3..6], &common::EXPRESSIONS[7..8]].concat() {
        let interpreted = jx::compile(source).unwrap();
        let mut accelerated = interpreted.clone();
        #[cfg(feature = "jit")]
        accelerated.enable_native();
        #[cfg(not(feature = "jit"))]
        let _ = &mut accelerated;
        let accelerated_copy = accelerated.clone();
        drop(accelerated);
        for a in atoms {
            for b in atoms {
                let input =
                    format!(r#"{{"a":{a},"b":{b},"flag":{a},"rows":[{{"n":{a}}},{{"n":{b}}}]}}"#);
                assert_eq!(
                    common::snapshot(&interpreted, input.as_bytes()),
                    common::snapshot(&accelerated_copy, input.as_bytes()),
                    "{source}: {input}"
                );
            }
        }
        let expr = accelerated_copy.clone();
        std::thread::spawn(move || common::snapshot(&expr, b"{\"a\":2,\"b\":3}"))
            .join()
            .unwrap();
    }
}

#[test]
fn host_failure_deadline_and_cancellation_do_not_leak_into_later_evaluations() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let token = jx::Cancellation::default();
    let cancel = token.clone();
    let host = HostFunction::new(1, move |args, context| {
        counter.fetch_add(1, Ordering::Relaxed);
        if args
            .first()
            .and_then(Option::as_ref)
            .and_then(Value::as_number)
            == Some(2.0)
        {
            cancel.cancel();
            context.checkpoint()?;
        }
        Ok(args.first().cloned().flatten())
    });
    let mut expr = CompileOptions::default()
        .binding("host")
        .compile("$map([1..100],function($n){$host($n)})")
        .unwrap();
    #[cfg(feature = "jit")]
    expr.enable_native();
    #[cfg(not(feature = "jit"))]
    let _ = &mut expr;
    let options = EvaluationOptions {
        bindings: vec![("host", host.value())],
        cancellation: Some(token),
        ..Default::default()
    };
    let error = expr.evaluate_with(Some(b"null"), options).unwrap_err();
    assert_eq!(error.kind, ErrorKind::HostError);
    assert_eq!(error.cause().unwrap().kind, ErrorKind::Cancelled);
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    let host = HostFunction::new(1, |args, _| Ok(args.first().cloned().flatten()));
    let options = EvaluationOptions {
        bindings: vec![("host", host.value())],
        deadline: Some(Instant::now()),
        ..Default::default()
    };
    assert_eq!(
        expr.evaluate_with(Some(b"null"), options).unwrap_err().kind,
        ErrorKind::EvaluationLimit
    );
    for source in [
        "($f:=$eval('function($n){function(){$n}}');$g:=$f(2);[$g(),$error('stop')])",
        "$eval('($f:=function($n){$f($n+1)};$f(0))')",
    ] {
        let expr = jx::compile(source).unwrap();
        for _ in 0..8 {
            let options = EvaluationOptions {
                limits: Some(Limits {
                    max_work: 50,
                    ..Default::default()
                }),
                ..Default::default()
            };
            assert!(expr.evaluate_with(None, options).is_err());
        }
    }
    let options = EvaluationOptions {
        bindings: vec![("host", host.value())],
        ..Default::default()
    };
    let mut count = 0;
    expr.evaluate_with(Some(b"null"), options)
        .unwrap()
        .for_each(|_| count += 1)
        .unwrap();
    assert_eq!(count, 100);
}

#[test]
fn adversarial_dynamic_pictures_fail_without_panicking_or_losing_validation() {
    let expressions = [
        "$formatNumber(a,picture)",
        "$formatInteger(a,picture)",
        "$toMillis(date,picture)",
    ]
    .map(|s| jx::compile(s).unwrap());
    let mut random = Random(0x29_da7e);
    let alphabet: Vec<char> = "[YMDHmsf]01#.,;-NnÉ年😀 ".chars().collect();
    let seeds = [
        "[Y]-[M]-[D]",
        "[Y,999999999999999999999999]",
        "[M01][D01][Y0001]",
        "0.00;(-0.00)",
        "#,##0.00",
        "[Y]年[M]月[D]日",
    ];
    for _ in 0..1024 {
        let mut picture: Vec<_> = seeds[random.next() % seeds.len()].chars().collect();
        for _ in 0..1 + random.next() % 3 {
            let at = random.next() % (picture.len() + 1);
            if at < picture.len() {
                picture[at] = alphabet[random.next() % alphabet.len()];
            } else {
                picture.push(alphabet[random.next() % alphabet.len()]);
            }
        }
        let input = serde_json::json!({"a":1234.5,"date":"1970-01-01","picture":picture.into_iter().collect::<String>()}).to_string();
        for expression in &expressions {
            let options = EvaluationOptions {
                limits: Some(Limits {
                    max_output_bytes: 65536,
                    ..Default::default()
                }),
                ..Default::default()
            };
            if let Ok(result) = expression.evaluate_with(Some(input.as_bytes()), options) {
                let _ = result.for_each(|v| {
                    let mut output = Vec::new();
                    if v.write_compact(&mut output).is_ok() {
                        jx::validate(&output).unwrap();
                    }
                });
            }
        }
    }
}

#[test]
fn escaping_dynamic_literals_retain_their_program_after_the_call_returns() {
    let expression = jx::compile(common::EXPRESSIONS[13]).unwrap();
    for a in [
        serde_json::json!("é😀"),
        serde_json::json!([1, {"n":2}]),
        serde_json::json!({"key":"value"}),
        serde_json::Value::Null,
    ] {
        let input = serde_json::json!({"a":a}).to_string();
        let (items, error) = common::snapshot(&expression, input.as_bytes());
        assert_eq!(error, None);
        assert_eq!(items.len(), 1);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&items[0]).unwrap(),
            serde_json::json!({"value":a})
        );
    }
    let expression = jx::compile("$eval(source)").unwrap();
    for source in [
        "(",
        "$error('stop')",
        "[1,$error('stop')]",
        "function($x)<invalid>{$x}",
    ] {
        let input = serde_json::json!({"source":source}).to_string();
        let error = expression.evaluate(input.as_bytes()).unwrap_err();
        assert!(matches!(
            error.kind,
            ErrorKind::EvalSyntax | ErrorKind::EvalError
        ));
        assert!(error.cause().is_some());
    }
    let (items, error) = common::snapshot(&expression, br#"{"source":"{'ok':true}"}"#);
    assert_eq!(error, None);
    assert_eq!(items, [br#"{"ok":true}"#.to_vec()]);
}
