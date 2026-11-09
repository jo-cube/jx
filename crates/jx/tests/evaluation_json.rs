use jx::{ConsumeError, ErrorKind, EvaluationOptions, Value};

#[test]
fn absent_input_and_json_null_have_distinct_focus_and_root() {
    for (source, absent, null) in [
        ("$", b"".as_slice(), b"null".as_slice()),
        ("$$", b"", b"null"),
        ("$exists($$)", b"false", b"true"),
        ("$type($)", b"", br#""null""#),
    ] {
        let expression = jx::compile(source).unwrap();
        for (input, expected) in [(None, absent), (Some(b"null".as_slice()), null)] {
            let mut output = b"prefix".to_vec();
            assert_eq!(
                expression
                    .evaluate_with(input, EvaluationOptions::default())
                    .unwrap()
                    .write_json(&mut output)
                    .unwrap(),
                !expected.is_empty()
            );
            assert_eq!(&output[..6], b"prefix");
            assert_eq!(&output[6..], expected);
        }
    }
    let expression = jx::compile("{'focus':$.id,'rootExists':$exists($$),'root':$$}").unwrap();
    for (input, expected) in [
        (
            None,
            br#"{"focus":"replacement","rootExists":false}"#.as_slice(),
        ),
        (
            Some(b"null".as_slice()),
            br#"{"focus":"replacement","rootExists":true,"root":null}"#,
        ),
    ] {
        let mut output = Vec::new();
        expression
            .evaluate_with(
                input,
                EvaluationOptions {
                    focus: Some(Value::from_json(br#"{"id":"replacement"}"#).unwrap()),
                    ..Default::default()
                },
            )
            .unwrap()
            .write_json(&mut output)
            .unwrap();
        assert_eq!(output, expected);
    }
}

#[test]
fn complete_serialization_rolls_back_and_keeps_later_evaluation_errors() {
    let expression = jx::compile("items.(v=1?$sum:v+1)").unwrap();
    let mut output = b"prefix".to_vec();
    for (focus, evaluation_error) in [
        (br#"{"items":[{"v":1},{"v":2}]}"#.as_slice(), false),
        (br#"{"items":[{"v":1},{"v":2},{"v":null}]}"#, true),
        (br#"{"items":[{"v":2},{"v":3},{"v":null}]}"#, true),
    ] {
        let evaluation = expression
            .evaluate_with(
                None,
                EvaluationOptions {
                    focus: Some(Value::from_json(focus).unwrap()),
                    ..Default::default()
                },
            )
            .unwrap();
        let error = evaluation.write_json(&mut output).unwrap_err();
        match error {
            ConsumeError::Evaluation(error) => {
                assert!(evaluation_error);
                assert_eq!(error.kind, ErrorKind::TypeError);
            }
            ConsumeError::Consumer(error) => {
                assert!(!evaluation_error);
                assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
            }
        }
        assert_eq!(output, b"prefix");
    }
    assert!(
        expression
            .evaluate_with(
                None,
                EvaluationOptions {
                    focus: Some(Value::from_json(br#"{"items":[{"v":2},{"v":3}]}"#).unwrap()),
                    ..Default::default()
                }
            )
            .unwrap()
            .write_json(&mut output)
            .unwrap()
    );
    assert_eq!(output, b"prefix[3,4]");
}

#[test]
fn absent_input_serialization_rejects_nested_functions_and_honors_controls() {
    let expression = jx::compile("{'values':[1,function(){0}]}").unwrap();
    let mut output = b"prefix".to_vec();
    assert!(
        matches!(expression.evaluate_with(None, EvaluationOptions::default()).unwrap().write_json(&mut output), Err(ConsumeError::Consumer(error)) if error.kind() == std::io::ErrorKind::InvalidInput)
    );
    assert_eq!(output, b"prefix");

    let expression = jx::compile("items.v").unwrap();
    let cancellation = jx::Cancellation::default();
    let evaluation = expression
        .evaluate_with(
            None,
            EvaluationOptions {
                focus: Some(Value::from_json(br#"{"items":[{"v":1},{"v":2}]}"#).unwrap()),
                cancellation: Some(cancellation.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    cancellation.cancel();
    assert!(
        matches!(evaluation.write_json(&mut output), Err(ConsumeError::Evaluation(error)) if error.kind == ErrorKind::Cancelled)
    );
    assert_eq!(output, b"prefix");
}
