//! Complete pure iteration regions derived from navigation IR. Acquisition and
//! execution use the common kernels; source and mapper bodies keep their IR owners.
//! Only filtered regions retain a composed scalar row.
use super::*;
use crate::sequence::{Halt, Output, Walk};

#[derive(Clone, Debug)]
pub(super) struct Region {
    source_depth: usize,
    eager: bool,
    // Only filtered regions compose a row; plain maps use the navigation body.
    pub(super) row: Option<Program>,
}
fn scalar(p: &Program) -> bool {
    scalar::supported(p)
        && !p.scoped
        && p.code.iter().all(|i| {
            !matches!(
                i.kind,
                Operation::Outer(..) | Operation::Argument(_) | Operation::Accept(_)
            )
        })
}
// A transparent nested producer carries a cardinality boundary. Its depth is
// recorded once, so execution locates the source without repeating this proof.
fn source(step: &navigation::Step) -> Option<(&navigation::Step, usize, bool)> {
    if step.effects {
        return None;
    }
    if scalar(&step.body) {
        return Some((step, 0, false));
    }
    if !step.predicates.is_empty() {
        return None;
    }
    let (producer, eager) = match step.body.code.as_ref() {
        [i] if i.out == step.body.result => (i, false),
        [i, j]
            if j.out == step.body.result
                && matches!(j.kind, Operation::Materialize(r) if r == i.out) =>
        {
            (i, true)
        }
        _ => return None,
    };
    let Operation::Navigate(n) = &producer.kind else {
        return None;
    };
    if n.array_focus || n.variable || n.steps.len() != 1 {
        return None;
    }
    let (source, depth, inner) = source(&n.steps[0])?;
    Some((source, depth + 1, eager || inner))
}
pub(super) fn plan(n: &navigation::Navigation) -> Option<Box<Region>> {
    if n.array_focus || n.variable {
        return None;
    }
    let [first, mapper] = n.steps.as_ref() else {
        return None;
    };
    if mapper.effects
        || mapper.lookup
        || !mapper.predicates.is_empty()
        || !facts::non_array_result(&mapper.body)
    {
        return None;
    }
    let (source, source_depth, eager) = source(first)?;
    if source.predicates.is_empty() && source_depth == 0 && !mapper.body.scoped {
        return Some(Box::new(Region {
            source_depth,
            eager,
            row: None,
        }));
    }
    if !scalar(&mapper.body) {
        return None;
    }
    let [predicate] = source.predicates.as_ref() else {
        return None;
    };
    if predicate.effects
        || predicate.position.is_some()
        || !scalar(&predicate.body)
        || !facts::truth_predicate(&predicate.body)
    {
        return None;
    }
    let mut row = mapper.body.clone();
    optimize::compose_row(&mut row, predicate.body.clone(), source.body.offset);
    acquisition::plan_row(&mut row, true, false);
    lower::prepare(&mut row);
    Some(Box::new(Region {
        source_depth,
        eager,
        row: Some(row),
    }))
}
impl Region {
    pub(super) fn walk<'e, 'i>(
        &'e self,
        navigation: &'e navigation::Navigation,
        a: run::Activation<'_, 'e, 'i, '_>,
        out: &mut Output<'_, 'e, 'i>,
    ) -> Walk {
        let mut source = &navigation.steps[0];
        for _ in 0..self.source_depth {
            let Operation::Navigate(n) = &source.body.code[0].kind else {
                unreachable!("compiled source locator")
            };
            source = &n.steps[0];
        }
        let mut frame = frame::Frame::new();
        let value = source.body.run(a, &mut frame)?;
        let Some(row) = &self.row else {
            let mapper = &navigation.steps[1].body;
            let mut child = frame::Frame::repeated();
            // Finish upstream mapping before a downstream evaluation failure;
            // consumer cancellation still stops immediately.
            let mut downstream = None;
            a.input
                .each(&value, &mapper.row_schema, |input, focus, _| -> Walk {
                    let result = mapper
                        .run(
                            run::Activation {
                                input,
                                focus: &focus,
                                wrapped: false,
                                ..a
                            },
                            &mut child,
                        )?
                        .into_value(input);
                    if let Some(value) = result
                        && downstream.is_none()
                    {
                        match out(value) {
                            Err(Halt::Evaluation(error)) => downstream = Some(error),
                            result => result?,
                        }
                    }
                    Ok(())
                })?;
            return downstream.map_or(Ok(()), |error| Err(Halt::Evaluation(error)));
        };
        self.rows(
            a,
            &value,
            row,
            &source.predicates[0].body,
            &navigation.steps[1].body,
            out,
        )
    }
    fn rows<'e, 'i>(
        &'e self,
        a: run::Activation<'_, 'e, 'i, '_>,
        source: &run::Cell<'e, 'i>,
        row: &'e Program,
        predicate: &'e Program,
        mapper: &'e Program,
        out: &mut Output<'_, 'e, 'i>,
    ) -> Walk {
        use run::{Activation, Cell};
        let missing = source.value(a.input).is_none();
        if missing && self.eager {
            return Ok(());
        }
        let undefined = Cell::Value(Value::Undefined);
        let source = if missing { &undefined } else { source };
        let mut frame = frame::Frame::repeated();
        let mut test_frame = frame::Frame::repeated();
        let mut map_frame = frame::Frame::repeated();
        let mut mapper_error = None;
        let mut downstream = None;
        let mut first = None;
        let mut defined = false;
        let mut deliver = |value| {
            if downstream.is_some() {
                return Ok(());
            }
            match out(value) {
                Err(Halt::Evaluation(error)) => {
                    downstream = Some(error);
                    Ok(())
                }
                other => other,
            }
        };
        // Ambiguous first arrays/undefined stay borrowed until cardinality is
        // known. Ordinary scalar/object candidates never leave the acquired row.
        let mut map = |input: &run::Input<'_, 'i, '_>, focus: Cell<'e, 'i>| {
            mapper
                .run(
                    Activation {
                        input,
                        focus: &focus,
                        wrapped: false,
                        ..a
                    },
                    &mut map_frame,
                )
                .map(|cell| cell.into_value(input))
        };
        a.input
            .each(source, &row.row_schema, |input, focus, _| -> Walk {
                let value = focus.value(input).unwrap_or(Value::Undefined);
                let ambiguous = value.is_array() || matches!(value, Value::Undefined);
                if first.is_none() && (defined || !ambiguous || !self.eager) {
                    if row.run_row(
                        Activation {
                            input,
                            focus: &focus,
                            wrapped: false,
                            ..a
                        },
                        &mut frame,
                        &mut mapper_error,
                        false,
                    )? {
                        defined = true;
                        if let Some(value) = frame.take_result(row.result).into_value(input) {
                            deliver(value)?;
                        }
                    }
                    return Ok(());
                }
                // Rare cardinality edges execute the original predicate and mapper
                // programs once each; no speculative language work is replayed.
                let selected = predicate
                    .run(
                        Activation {
                            input,
                            focus: &focus,
                            wrapped: false,
                            ..a
                        },
                        &mut test_frame,
                    )?
                    .truth(input, predicate.offset)?;
                if !selected || mapper_error.is_some() {
                    return Ok(());
                }
                if let Some(value) = first.take() {
                    match map(a.input, a.input.restore(Some(value))) {
                        Ok(Some(value)) => deliver(value)?,
                        Ok(None) => {}
                        Err(error) => mapper_error = Some(error),
                    }
                }
                if !defined && ambiguous {
                    first = Some(value);
                } else if mapper_error.is_none() {
                    match map(input, focus) {
                        Ok(Some(value)) => deliver(value)?,
                        Ok(None) => {}
                        Err(error) => mapper_error = Some(error),
                    }
                }
                defined = true;
                Ok(())
            })?;
        if let Some(error) = mapper_error {
            return Err(Halt::Evaluation(error));
        }
        if let Some(value) = first {
            // A sole selected array is the next mapping source. Undefined alone
            // normalizes to missing; multiple undefined positions remain values.
            if !matches!(value, Value::Undefined) {
                a.input.each(
                    &Cell::Value(value),
                    &mapper.row_schema,
                    |input, focus, _| -> Walk {
                        if let Some(value) = map(input, focus)? {
                            deliver(value)?;
                        }
                        Ok(())
                    },
                )?;
            }
        }
        downstream.map_or(Ok(()), |e| Err(Halt::Evaluation(e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn strip(p: &mut Program) {
        for i in &mut p.code {
            if let Operation::Navigate(n) = i.kind.producer_mut() {
                n.region = None;
            }
            i.kind.children_mut(&mut strip);
        }
    }
    #[test]
    fn missing_source_keeps_general_predicate_semantics() {
        let p = crate::CompileOptions::default()
            .compile_program("$sum((items[1/$.n>0]).(v+1))")
            .unwrap();
        assert!(
            p.code
                .iter()
                .any(|i| matches!(i.kind.producer(), Operation::Navigate(n) if n.region.is_some()))
        );
        let mut general = p.clone();
        strip(&mut general);
        let mut a = Acquisition::new([&p]);
        let d = a.prepare(br#"{"n":0}"#).unwrap();
        let mut output = b"prefix".to_vec();
        let mut expected = output.clone();
        assert_eq!(
            format!("{:?}", p.write_json(&d, &mut output)),
            format!("{:?}", general.write_json(&d, &mut expected))
        );
        assert_eq!(output, expected);
    }
    #[test]
    fn physical_selection_matches_general_delivery_on_unexpected_shapes() {
        for source in [
            "$sum((items[v>0]).(v*w+v))",
            "(items[v>0]).(v*w+v)",
            "$count((items[v>0]).(v*w+v))",
            "$sum((items[true]).(v+1))",
            "$sum((items[v>0][w>0]).(v/w))",
        ] {
            let p = crate::CompileOptions::default()
                .compile_program(source)
                .unwrap();
            let mut general = p.clone();
            strip(&mut general);
            for items in [
                "[]",
                "[{}]",
                "null",
                "1",
                "[null]",
                "[[]]",
                "[[{}]]",
                "[[{\"v\":1,\"w\":2}]]",
                "[[{\"v\":1}],[{\"v\":2}]]",
                "[{},{}]",
                "[{\"v\":1,\"w\":2}]",
                "[{\"v\":1,\"w\":2},{\"v\":2,\"w\":3}]",
                "[{\"v\":1,\"w\":null},{\"v\":false}]",
                "[{\"v\":1,\"w\":0},{\"v\":2,\"w\":0}]",
            ] {
                for input in [
                    format!("{{\"items\":{items}}}"),
                    format!("[{{\"items\":{items}}}]"),
                ] {
                    let mut a = Acquisition::new([&p]);
                    let d = a.prepare(input.as_bytes()).unwrap();
                    let mut actual = b"prefix".to_vec();
                    let mut expected = actual.clone();
                    let result = p.write_json(&d, &mut actual);
                    let reference = general.write_json(&d, &mut expected);
                    assert_eq!(
                        format!("{result:?}"),
                        format!("{reference:?}"),
                        "{source} {input}"
                    );
                    assert_eq!(actual, expected, "{source} {input}");
                }
            }
        }
    }
    #[test]
    fn ordinary_maps_require_proven_non_array_results() {
        fn has_region(p: &Program) -> bool {
            p.code.iter().any(|i| {
                let mut found =
                    matches!(i.kind.producer(), Operation::Navigate(n) if n.region.is_some());
                i.kind.children(&mut |p| found |= has_region(p));
                found
            })
        }
        for (source, eligible) in [
            ("{'v':items.(n+1)}", true),
            ("{'v':items.$lowercase(name)}", true),
            ("{'v':items.{'v':n+1}}", true),
            ("{'v':items.[n]}", false),
            ("{'v':items.(flag ? n : name)}", false),
            ("{'v':items.($x:=n;$x+1)}", false),
            ("{'v':items.($error('stop');1)}", true),
            ("{'v':items.($random();1)}", false),
            ("{'v':items.{'f':$sum,'n':n+1}}", false),
            ("{'v':items.(n+1)[true]}", false),
        ] {
            let p = crate::CompileOptions::default()
                .compile_program(source)
                .unwrap();
            assert_eq!(has_region(&p), eligible, "{source}");
        }
    }
    #[test]
    fn unfiltered_regions_match_general_contracts() {
        for source in [
            "{'v':items.(nested.v+nested.v)}",
            "items.(nested.v+nested.v)",
            "items.{'f':$sum,'n':1/n}",
            "{'v':items.{'f':$sum,'n':1/n}}",
            "{'v':items.$lowercase(name)}",
            "{'v':items.{'name':$lowercase(name),'v':nested.v+nested.v}}",
            "{'v':items.(1/n)}",
            "{'v':items.(n>1 ? $number(name) : 1)}",
            "{'v':items.$substring(name,1,2)}",
            "{'v':items.[nested.v]}",
        ] {
            let p = crate::CompileOptions::default()
                .compile_program(source)
                .unwrap();
            let mut general = p.clone();
            strip(&mut general);
            for items in [
                r#"[{"n":1},{"n":0}]"#,
                "[]",
                "[{}]",
                "null",
                "1",
                "[null]",
                "[[]]",
                "[[{}]]",
                "[[{\"name\":\"ΟΣ\",\"nested\":{\"v\":1}}]]",
                "[{\"name\":\"Café\",\"nested\":{\"v\":2},\"n\":1}]",
                "[{\"name\":\"A\",\"nested\":{\"v\":1},\"n\":0},{\"name\":null,\"nested\":{\"v\":2},\"n\":2}]",
                "[{\"name\":\"A\",\"nested\":{\"v\":1},\"n\":1},{\"name\":null,\"nested\":{\"v\":false},\"n\":2}]",
                "[{\"name\":\"\\ud800\\u039f\\u03a3\",\"nested\":{\"v\":1}}]",
                "[{\"nested\":{\"v\":1},\"nested\":{}}]",
                "[{\"nested\":{\"v\":1},\"nested\":{\"v\":3}}]",
            ] {
                for input in [
                    format!("{{\"items\":{items}}}"),
                    format!("[{{\"items\":{items}}}]"),
                    "{}".to_owned(),
                ] {
                    let mut acquisition = Acquisition::new([&p]);
                    let doc = acquisition.prepare(input.as_bytes()).unwrap();
                    let mut actual = b"prefix".to_vec();
                    let mut expected = actual.clone();
                    assert_eq!(
                        format!("{:?}", p.write_json(&doc, &mut actual)),
                        format!("{:?}", general.write_json(&doc, &mut expected)),
                        "{source} {input}"
                    );
                    assert_eq!(actual, expected, "{source} {input}");
                    assert_eq!(
                        format!(
                            "{:?}",
                            p.evaluate_bytes(input.as_bytes())
                                .and_then(|e| e.collect_owned())
                        ),
                        format!(
                            "{:?}",
                            general
                                .evaluate_bytes(input.as_bytes())
                                .and_then(|e| e.collect_owned())
                        ),
                        "{source} {input}"
                    );
                    let cancel = |program: &Program| {
                        let mut count = 0;
                        let result = program.try_for_each(&doc, |_| {
                            count += 1;
                            Err("consumer")
                        });
                        (count, result)
                    };
                    assert_eq!(cancel(&p), cancel(&general), "{source} {input}");
                }
            }
        }
    }
    #[test]
    fn unfiltered_maps_preserve_cardinality_and_duplicate_fields() {
        let p = crate::CompileOptions::default()
            .compile_program("{'v':items.(nested.v+nested.v)}")
            .unwrap();
        let mut acquisition = Acquisition::new([&p]);
        for (input, expected) in [
            (r#"{}"#, r#"{}"#),
            (r#"{"items":null}"#, r#"{}"#),
            (r#"{"items":1}"#, r#"{}"#),
            (r#"{"items":[]}"#, r#"{}"#),
            (r#"{"items":[null,{}]}"#, r#"{}"#),
            (r#"{"items":{"nested":{"v":2}}}"#, r#"{"v":4}"#),
            (r#"{"items":[{"nested":{"v":2}}]}"#, r#"{"v":4}"#),
            (r#"{"items":[[{"nested":{"v":2}}]]}"#, r#"{"v":4}"#),
            (
                r#"{"items":[{"nested":{"v":1}},{},{"nested":{"v":3}}]}"#,
                r#"{"v":[2,6]}"#,
            ),
            (
                r#"{"items":[{"nested":{"v":1},"nested":{}},{"nested":{"v":2},"nested":{"v":3}}]}"#,
                r#"{"v":6}"#,
            ),
            (r#"[{"items":[{"nested":{"v":2}}]}]"#, r#"{"v":4}"#),
        ] {
            let doc = acquisition.prepare(input.as_bytes()).unwrap();
            let mut output = b"prefix".to_vec();
            assert!(p.write_json(&doc, &mut output).unwrap(), "{input}");
            assert_eq!(&output[6..], expected.as_bytes(), "{input}");
        }
    }

    #[test]
    fn unfiltered_map_values_outlive_row_and_workspace_scratch() {
        let p = crate::CompileOptions::default()
            .compile_program("{'v':items.{'name':name,'nested':nested,'twice':nested.v+nested.v}}")
            .unwrap();
        let mut workspace = Workspace::default();
        let first = p
            .evaluate_with_workspace(
                Some(br#"{"items":[{"name":"A","nested":{"v":1}},{"name":"B","nested":{"v":2}}]}"#),
                Default::default(),
                &mut workspace,
            )
            .unwrap()
            .single()
            .unwrap()
            .unwrap();
        for input in [br#"{"items":[{"name":"C"}]}"#.as_slice(), br#"{}"#] {
            p.evaluate_with_workspace(Some(input), Default::default(), &mut workspace)
                .unwrap()
                .collect_owned()
                .unwrap();
        }
        let mut output = Vec::new();
        first.write_compact(&mut output).unwrap();
        assert_eq!(output, br#"{"v":[{"name":"A","nested":{"v":1},"twice":2},{"name":"B","nested":{"v":2},"twice":4}]}"#);
    }

    #[test]
    fn unfiltered_map_errors_preserve_precedence_and_output_prefix() {
        for source in [
            "{'v':items.(n+1)}",
            // Upstream type failure wins over an earlier downstream range failure.
            "(items.(n+1)).((1/($-2))+0)",
            "items.{'f':$sum,'n':n+1}",
            "{'v':items.{'f':$sum,'n':n+1}}",
        ] {
            let p = crate::CompileOptions::default()
                .compile_program(source)
                .unwrap();
            let mut acquisition = Acquisition::new([&p]);
            let doc = acquisition
                .prepare(br#"{"items":[{"n":1},{"n":null}]}"#)
                .unwrap();
            let mut output = b"prefix".to_vec();
            let error = p.write_json(&doc, &mut output).unwrap_err();
            assert!(
                matches!(error, crate::ConsumeError::Evaluation(e) if e.kind == crate::ErrorKind::TypeError),
                "{source}"
            );
            assert_eq!(output, b"prefix", "{source}");
        }
    }
}
