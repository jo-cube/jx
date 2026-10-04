use super::*;

#[derive(Clone, Debug)]
pub(crate) struct Callback {
    pub(super) execution: Execution,
}
impl Callback {
    // Registers carry only primitives. Static object members may borrow this
    // short-lived program, so export those through the existing retention boundary.
    pub(crate) fn retained<'e, 'i>(
        &self,
        arguments: &[Option<Value<'e, 'i>>],
        focus: &Value<'e, 'i>,
        wrapped: bool,
    ) -> Option<Operand<'e, 'i>> {
        match self.run(arguments, focus, wrapped)? {
            Operand::Missing => Some(Operand::Missing),
            Operand::One(Value::Number(n)) => Some(Operand::One(Value::Number(n))),
            Operand::One(Value::Boolean(b)) => Some(Operand::One(Value::Boolean(b))),
            Operand::One(value) => Some(Operand::One(
                crate::dynamic::Retention::default().value(&value),
            )),
            Operand::Many(_) => unreachable!(),
        }
    }

    pub(crate) fn run<'e, 'i>(
        &'e self,
        arguments: &[Option<Value<'e, 'i>>],
        focus: &Value<'e, 'i>,
        wrapped: bool,
    ) -> Option<Operand<'e, 'i>> {
        if matches!(self.execution, Execution::Object(_)) && !wrapped && focus.is_array() {
            return None;
        }
        let program = match &self.execution {
            Execution::Scalar(p) => p,
            Execution::Object(o) => &o.program,
            Execution::Fold(_) => unreachable!(),
        };
        let value = |source: Option<u8>| match source {
            Some(index) => arguments.get(usize::from(index)).and_then(Option::as_ref),
            None => Some(focus),
        };
        let mut captured = Captures::default();
        for (source, demand) in &program.argument_demands {
            if let Some(Value::Raw(raw)) = value(*source) {
                raw.capture(demand, &mut captured);
            }
        }
        program.dispatch(
            |at, path| {
                let source = program.inputs[at];
                let Some(value) = value(source) else {
                    return Some(Operand::Missing);
                };
                if matches!(value, Value::Raw(_)) {
                    return captured_operand(captured.get(at));
                }
                path.select_context(&Context {
                    value: value.clone(),
                    wrapped: source.is_none() && wrapped,
                    scope: None,
                })
                .operand()
                .ok()
            },
            |slots| match &self.execution {
                Execution::Scalar(_) => slots[usize::from(program.result)].operand(),
                Execution::Object(o) => o.construct(slots),
                Execution::Fold(_) => unreachable!(),
            },
        )
    }
}
pub(super) fn lower(definition: &crate::function::Definition) -> Option<Callback> {
    if definition.tail
        || definition.params.is_empty()
        || definition.params.iter().any(|p| p.is_empty())
        || definition.body.clock
    {
        return None;
    }
    let body = &definition.body;
    let execution = if let Some(object) = object::lower_parameters(body, &definition.params) {
        Execution::Object(object)
    } else {
        if !lower::computed(body) {
            return None;
        }
        let mut lower = Lower::parameters(&definition.params);
        let result = lower.node(body)?;
        if lower.operations == 0 {
            return None;
        }
        Execution::Scalar(lower.finish(result))
    };
    Some(Callback { execution })
}
pub(super) fn path(node: &Node, parameters: &[Box<str>]) -> Option<(Path, u8)> {
    let (name, fields) = match &node.kind {
        Kind::Variable(name) => (name, Vec::new()),
        Kind::Route(steps, false) => {
            let (first, rest) = steps.split_first()?;
            let Kind::Variable(name) = &first.node.kind else {
                return None;
            };
            if steps
                .iter()
                .any(|s| !s.predicates.is_empty() || s.bindings.is_some())
            {
                return None;
            }
            let mut fields = Vec::new();
            for step in rest {
                let Kind::Path(path) = &step.node.kind else {
                    return None;
                };
                if path.rooted {
                    return None;
                }
                fields.extend(path.fields.iter().cloned());
            }
            (name, fields)
        }
        _ => return None,
    };
    let source = parameters
        .iter()
        .rposition(|p| p == name)?
        .try_into()
        .ok()?;
    Some((
        Path {
            fields: fields.into_boxed_slice(),
            rooted: false,
        },
        source,
    ))
}
