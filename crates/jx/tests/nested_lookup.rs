use jx::{CompileOptions, Error, Evaluation, EvaluationOptions, InputPlan, OwnedValue};

fn config() -> OwnedValue {
    OwnedValue::from_json(
        br#"{"map":{"x":{"y":{"z":7},"array":[1,2]},"rows":[{"y":1},{"y":[2,3]},{}],"scalar":1,"null":null,"undefined":{"undefined":9}},"direct":{"y":8}}"#,
    )
    .unwrap()
}

fn outcome(result: Result<Evaluation<'_, '_>, Error>) -> Result<Vec<Vec<u8>>, Error> {
    let mut items = Vec::new();
    result?.for_each(|value| {
        let mut bytes = Vec::new();
        value.write_compact(&mut bytes).unwrap();
        items.push(bytes);
    })?;
    Ok(items)
}

#[test]
fn nested_captures_match_runtime_lookup_values_cardinality_and_diagnostics() {
    let binding = config();
    let options = CompileOptions::default().constant_binding("config", binding.clone());
    let runtime = CompileOptions::default().binding("config");
    for source in [
        "$lookup($lookup($config.map,a),b)",
        "$lookup($lookup($config.map,a.key),b.key)",
        "$lookup($lookup(($lookup($config.map,a)),b),c)",
        "{'v':$lookup($lookup($config.map,a),b),'input':c}",
        "$lookup($lookup($config.map,a),b) ?? $lookup($config.direct,c)",
        "flag ? $lookup($config.direct,b) : $lookup($lookup($config.map,a),b)",
        "$lookup($lookup($config.map,a&c),b)",
        "$lookup($lookup($config.map,a),b) = $lookup($lookup($config.map,a),b)",
    ] {
        let expression = options.compile(source).unwrap();
        let original = runtime.compile(source).unwrap();
        let plan = InputPlan::new([&expression]);
        for input in [
            r#"{"a":"x","b":"y","c":"z","flag":false}"#,
            r#"{"a":"absent","b":"y","c":"y","flag":false}"#,
            r#"{"a":"x","b":"absent","c":"y","flag":false}"#,
            r#"{"a":"rows","b":"y","c":"y","flag":false}"#,
            r#"{"a":"x","b":"array","c":"y","flag":false}"#,
            r#"{"a":"scalar","b":"y","c":"y"}"#,
            r#"{"a":"null","b":"y","c":"y"}"#,
            r#"{"a":"x","b":1,"c":"y","flag":true}"#,
            r#"{"a":1,"b":2,"c":"y"}"#,
            r#"{"a":["x"],"b":"y","c":"z"}"#,
            r#"{"a":"x","b":["y","array"],"c":"z"}"#,
            r#"{"a":{"key":"x"},"b":null}"#,
            r#"{"a":{"key":"x"},"b":{"key":"y"}}"#,
            r#"{"a":[{"key":"x"}],"b":{"key":"y"}}"#,
            r#"{}"#,
            r#"{"a":"absent","\u0061":"x","b":"absent","b":"y","c":"z"}"#,
            r#"[{"a":"x","b":"y","c":"z"},{"a":"rows","b":"y","c":"z"}]"#,
        ] {
            let expected = outcome(original.evaluate_with(
                Some(input.as_bytes()),
                EvaluationOptions {
                    bindings: vec![("config", binding.as_value())],
                    ..Default::default()
                },
            ));
            let prepared = plan.prepare(input.as_bytes()).unwrap();
            assert_eq!(outcome(prepared.evaluate(0)), expected, "{source}: {input}");
            assert_eq!(outcome(expression.evaluate(input.as_bytes())), expected);
            let prepared = plan.prepare_validated(jx::validate(input.as_bytes()).unwrap());
            assert_eq!(outcome(prepared.evaluate(0)), expected);
        }
    }
}

#[test]
fn nested_keys_keep_order_and_untaken_branches_remain_lazy() {
    let options = CompileOptions::default().constant_binding("config", config());
    let expressions = [
        "$lookup($lookup($config.map,a),$number(b))",
        "flag ? $lookup($config.direct,c) : $lookup($lookup($config.map,a),$number(b))",
        "$lookup($config.direct,c) ?? $lookup($lookup($config.map,a),$number(b))",
    ]
    .map(|source| options.compile(source).unwrap());
    let plan = InputPlan::new(&expressions);
    let prepared = plan
        .prepare(br#"{"a":1,"b":"bad","c":"y","flag":true}"#)
        .unwrap();
    let error = outcome(prepared.evaluate(0)).unwrap_err();
    assert_eq!(error.kind, jx::ErrorKind::TypeError);
    assert_eq!(error.offset, 8); // The inner lookup fails before the outer key conversion.
    for index in [1, 2] {
        assert_eq!(outcome(prepared.evaluate(index)).unwrap(), [b"8".to_vec()]);
    }
    let prepared = plan.prepare(br#"{"a":"absent","b":"bad"}"#).unwrap();
    assert_eq!(
        outcome(prepared.evaluate(0)),
        outcome(expressions[0].evaluate(br#"{"a":"absent","b":"bad"}"#))
    );
    assert!(outcome(prepared.evaluate(0)).is_err()); // Missing object still evaluates its key.
}

#[test]
fn effects_shadowing_and_assignments_keep_independent_fallback_execution() {
    let options = CompileOptions::default().constant_binding("config", config());
    let expressions = [
        "flag ? $lookup($lookup($config.map,a),b) : $error('untaken')",
        "$lookup($lookup($config.map,a),b) ?? $error('missing')",
        "($lookup:=function($object,$key){$key}; $lookup($lookup($config.map,a),b))",
        "($exists:=function($value){false}; $lookup($lookup($config.map,a),b) ?? 9)",
        "($config:={'map':{'x':{'y':10}}}; $lookup($lookup($config.map,a),b))",
        "($x:=$lookup($lookup($config.map,a),b); $x)",
        "$x",
        "$lookup($lookup($config.map,a),b)",
    ]
    .map(|source| options.compile(source).unwrap());
    let plan = InputPlan::new(&expressions);
    let input = br#"{"a":"x","b":"y","flag":true}"#;
    let prepared = plan.prepare(input).unwrap();
    for _ in 0..2 {
        for (index, expression) in expressions.iter().enumerate().rev() {
            assert_eq!(
                outcome(prepared.evaluate(index)),
                outcome(expression.evaluate(input))
            );
        }
    }
    assert_eq!(outcome(prepared.evaluate(3)).unwrap(), [b"9".to_vec()]);
    assert!(outcome(prepared.evaluate(6)).unwrap().is_empty());
}

#[test]
fn nested_results_outlive_captures_and_input_is_fully_validated() {
    let expression = CompileOptions::default()
        .constant_binding("config", config())
        .compile("{'v':$lookup($lookup($config.map,a),b),'raw':c}")
        .unwrap();
    let input = br#"{"a":"x","b":"y","c": { "untouched": 1 }}"#;
    let value = {
        let plan = InputPlan::new([&expression]);
        plan.prepare(input)
            .unwrap()
            .evaluate(0)
            .unwrap()
            .single()
            .unwrap()
            .unwrap()
    };
    assert_eq!(
        value.get("raw").unwrap().as_raw().unwrap().as_str(),
        r#"{ "untouched": 1 }"#
    );
    assert_eq!(
        value.get("v").unwrap().get("z").unwrap().as_number(),
        Some(7.)
    );
    assert!(value.to_owned().is_ok());
    let plan = InputPlan::new([&expression]);
    for input in [
        b"{\"a\":\"x\",\"b\":\"y\",\"unused\":[1,]}".as_slice(),
        b"{\"a\":\"x\",\"b\":\"y\"} trailing",
    ] {
        let error = match plan.prepare(input) {
            Ok(_) => panic!("invalid input accepted"),
            Err(error) => error,
        };
        assert_eq!(error, jx::validate(input).unwrap_err());
    }
}
