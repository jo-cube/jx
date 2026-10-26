use super::*;
use crate::{Error, RawJson, evaluate::Operand, sequence::Context};

pub(super) use super::input::{Cell, Input};

#[derive(Clone, Copy)]
pub(super) struct Activation<'a, 'e, 'i, 't> {
    pub input: &'a Input<'a, 'i, 't>,
    pub focus: &'a Cell<'e, 'i>,
    pub wrapped: bool,
    pub calls: call::Depth,
    pub args: &'a [Cell<'e, 'i>],
    pub outer: frame::Registers<'a, 'e, 'i>,
    pub scope: Option<&'a crate::runtime::Scope<'e, 'i>>,
}
impl Program {
    pub(super) fn run<'e, 'i>(
        &'e self,
        activation: Activation<'_, 'e, 'i, '_>,
        frame: &mut frame::Frame<'e, 'i>,
    ) -> Result<Cell<'e, 'i>, Error> {
        // Reclaim row-local captures after the result has left its activation.
        // The shared arena's reachability rules retain returned closures and writes.
        if self.scoped
            && let Some(scope) = activation.scope
        {
            let input = activation.input;
            return scope
                .retained(|| {
                    self.run_value(activation, frame)
                        .map(|v| v.into_value(input))
                })
                .map(|v| v.map_or(Cell::Missing, Cell::Value));
        }
        self.run_value(activation, frame)
    }
    fn run_value<'e, 'i>(
        &'e self,
        activation: Activation<'_, 'e, 'i, '_>,
        frame: &mut frame::Frame<'e, 'i>,
    ) -> Result<Cell<'e, 'i>, Error> {
        if self.scalar.is_some() {
            return self.run_scalar(activation, frame);
        }
        match self.run_prefix(activation, frame, None)? {
            call::Exit::Done => Ok(frame.take_result(self.result)),
            _ => unreachable!("tail calls occur only in function bodies"),
        }
    }
    pub(super) fn run_prefix<'e, 'i>(
        &'e self,
        activation: Activation<'_, 'e, 'i, '_>,
        frame: &mut frame::Frame<'e, 'i>,
        stop: Option<usize>,
    ) -> Result<call::Exit<'e, 'i>, Error> {
        let Activation {
            input,
            focus,
            wrapped,
            args,
            outer,
            scope,
            calls,
        } = activation;
        let mut scopes = call::Scopes::new(scope);
        let controlled = scope.filter(|s| s.controlled());
        let reads = input.reads(self);
        frame.configure(&self.storage);
        let mut pc = 0;
        while pc < self.code.len() {
            if stop == Some(pc) {
                return Ok(call::Exit::Stop(pc, scopes));
            }
            let instruction = &self.code[pc];
            let offset = instruction.offset;
            if let Some(scope) = controlled {
                scope.checkpoint(offset)?;
            }
            let load = |value: Cell<'e, 'i>| {
                if self.atomic_loads[instruction.out] {
                    value.atomic(input)
                } else {
                    value
                }
            };
            let cell = match &instruction.kind {
                Operation::Focus => load(focus.read()),
                Operation::Root => load(if let Some(scope) = &scopes.current {
                    input.restore(scope.lookup("$"))
                } else {
                    input.root()
                }),
                Operation::Read(name) => load(
                    input.restore(
                        scopes
                            .current
                            .as_ref()
                            .expect("lexical scope")
                            .lookup(name)
                            .or_else(|| Builtin::named(name).map(crate::Function::builtin)),
                    ),
                ),
                Operation::Bind(name, reg) => {
                    let value = frame[*reg].value(input).unwrap_or(Value::Undefined);
                    scopes
                        .current
                        .as_ref()
                        .expect("lexical scope")
                        .bind(name, value.clone());
                    if matches!(value, Value::Undefined) {
                        Cell::Missing
                    } else {
                        Cell::Value(value)
                    }
                }
                Operation::EnterScope => {
                    scopes.enter();
                    Cell::Missing
                }
                Operation::LeaveScope => {
                    scopes.leave();
                    Cell::Missing
                }
                Operation::Function(definition) => {
                    Cell::Value(Value::Function(std::rc::Rc::new(crate::Function {
                        kind: crate::function::FunctionKind::Program {
                            definition,
                            focus: focus.value(input).unwrap_or(Value::Undefined),
                            wrapped,
                            frame: scopes.current.as_ref().expect("lexical scope").capture(),
                        },
                    })))
                }
                Operation::Transform(definition) => {
                    Cell::Value(Value::Function(std::rc::Rc::new(crate::Function {
                        kind: crate::function::FunctionKind::ProgramTransform {
                            definition,
                            frame: scopes.current.as_ref().expect("transform scope").capture(),
                        },
                    })))
                }
                Operation::Matcher(pattern) => Cell::Value(crate::matcher::literal(pattern)),
                Operation::Formatted(function, registers, prepared) => {
                    let mut values = crate::function::Arguments::new(registers.len());
                    for reg in registers {
                        values.push(frame[*reg].value(input));
                    }
                    let context = Context {
                        value: focus.value(input).unwrap_or(Value::Undefined),
                        wrapped,
                        scope: scopes.current.clone(),
                    };
                    Cell::from_operand(prepared.values(
                        *function,
                        values.as_slice(),
                        &context,
                        offset,
                    )?)?
                }
                Operation::FunctionReference(b) => Cell::Value(crate::Function::builtin(*b)),
                Operation::Invoke(target, registers, tail, native, prepared) => {
                    let mut arguments = crate::function::Arguments::new(registers.len());
                    for reg in registers {
                        arguments.push(frame[*reg].value(input));
                    }
                    let Some(Value::Function(function)) = frame[*target].value(input) else {
                        return Err(crate::value::type_error(offset));
                    };
                    if *tail {
                        return Ok(call::Exit::Call(crate::function::tail::Call {
                            function,
                            arguments,
                            offset,
                            validate: true,
                            null_focus: false,
                        }));
                    }
                    let context = Context {
                        value: focus.value(input).unwrap_or(Value::Undefined),
                        wrapped,
                        scope: scopes.current.clone(),
                    };
                    let value = if let Some(prepared) = prepared
                        && matches!(
                            function.kind,
                            crate::function::FunctionKind::Builtin(Builtin::Runtime(
                                crate::dynamic::Builtin::Eval
                            ))
                        ) {
                        prepared.evaluate(arguments.as_slice(), &context, input, offset)?
                    } else {
                        crate::function::invoke_in(
                            &function,
                            arguments.as_slice(),
                            &context,
                            offset,
                            true,
                            Some(input),
                        )?
                    };
                    let value = if *native {
                        Cell::from_native_operand(value)?
                    } else {
                        Cell::from_operand(value)?
                    };
                    match value {
                        Cell::Value(value @ Value::Raw(_)) => input.restore(Some(value)),
                        value => value,
                    }
                }
                Operation::Partial(target, args) => {
                    let arguments = args
                        .iter()
                        .map(|r| match r {
                            Some(r) => crate::function::composition::Argument::Value(
                                frame[*r].value(input),
                            ),
                            None => crate::function::composition::Argument::Hole,
                        })
                        .collect();
                    let Some(Value::Function(target)) = frame[*target].value(input) else {
                        return Err(crate::value::type_error(offset));
                    };
                    Cell::Value(crate::function::composition::bind_partial(
                        target, arguments, offset,
                    )?)
                }
                Operation::Chain(left, right, native) => {
                    let Some(Value::Function(second)) = frame[*right].value(input) else {
                        return Err(crate::value::type_error(offset));
                    };
                    if let Some(Value::Function(first)) = frame[*left].value(input) {
                        Cell::Value(Value::Function(std::rc::Rc::new(crate::Function {
                            kind: crate::function::FunctionKind::Chain(first, second),
                        })))
                    } else {
                        let context = Context {
                            value: Value::Null,
                            wrapped: false,
                            scope: scopes.current.clone(),
                        };
                        let value = crate::function::invoke(
                            &second,
                            &[frame[*left].value(input)],
                            &context,
                            offset,
                        )?;
                        if *native {
                            Cell::from_native_operand(value)?
                        } else {
                            Cell::from_operand(value)?
                        }
                    }
                }
                Operation::Argument(i) => load(args.get(*i).map_or(Cell::Missing, Cell::read)),
                Operation::Outer(i, _) => load(outer[*i].read()),
                Operation::Literal(l) => match l {
                    Literal::Missing => Cell::Missing,
                    Literal::Null => Cell::Value(Value::Null),
                    Literal::Boolean(b) => Cell::Value(Value::Boolean(*b)),
                    Literal::Number(n) => Cell::Value(Value::Number(*n)),
                    Literal::String(s) => Cell::Value(Value::StringLiteral(RawJson(s))),
                    Literal::Data(d) => match &***d {
                        Data::Missing => Cell::Missing,
                        _ => Cell::Value(d.value()),
                    },
                },
                Operation::Field(base, path, _, FieldMode::Retained)
                    if frame[*base].value(input).is_some_and(|v| v.is_array()) =>
                {
                    let mut values = Vec::new();
                    crate::path::lookup(
                        &frame[*base].value(input).unwrap(),
                        &path.fields[0],
                        &mut |value| {
                            values.push(value);
                            Ok::<_, Error>(())
                        },
                    )?;
                    if values.is_empty() {
                        Cell::Missing
                    } else {
                        Cell::Value(Value::kept(values))
                    }
                }
                Operation::Field(base, path, contextual, _) => load(
                    input
                        .select_field(
                            &frame[*base],
                            path,
                            *contextual && wrapped,
                            reads.and_then(|r| r.get(instruction.out)),
                        )
                        .retain()?,
                ),
                Operation::Copy(i) | Operation::Materialize(i) => load(frame[*i].clone()),
                Operation::Acquire(i, schema) => load(frame[*i].acquire(input, schema)),
                Operation::Accept(_) => unreachable!("acceptance is a scalar row stage"),
                Operation::Truth(i) => Cell::Value(Value::Boolean(frame[*i].truth(input, offset)?)),
                Operation::Negate(i) => Cell::from_operand(crate::evaluate::pure::negate(
                    frame[*i].operand(input),
                    offset,
                )?)?,
                Operation::Binary(op, left, right) => {
                    if matches!(
                        op,
                        Op::Add | Op::Subtract | Op::Multiply | Op::Divide | Op::Remainder
                    ) {
                        frame[*left].arithmetic(*op, &frame[*right], input, offset)?
                    } else {
                        let left = frame[*left].operand(input);
                        let right = frame[*right].operand(input);
                        let result = match op {
                            Op::Concat => Operand::One(crate::convert::concat(
                                crate::function::retained(left)?,
                                crate::function::retained(right)?,
                                offset,
                            )?),
                            Op::In => {
                                Operand::One(Value::Boolean(crate::compare::includes(left, right)?))
                            }
                            Op::Equal | Op::NotEqual => Operand::One(Value::Boolean(
                                crate::compare::equal(left, right, *op == Op::NotEqual)?,
                            )),
                            Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => {
                                crate::compare::order(left, right, *op, offset)?
                            }
                            _ => unreachable!("non-numeric IR operation"),
                        };
                        Cell::from_operand(result)?
                    }
                }
                Operation::Lookup(data, key) => load(
                    crate::lookup::constant(data, frame[*key].value(input), offset)?
                        .map_or(Cell::Missing, Cell::Value),
                ),
                Operation::Builtin(builtin, registers, native) => {
                    let mut values = crate::function::Arguments::new(registers.len());
                    for reg in registers {
                        values.push(frame[*reg].value(input));
                    }
                    let context = Context {
                        value: focus.value(input).unwrap_or(Value::Undefined),
                        wrapped,
                        scope: scopes.current.clone(),
                    };
                    if let Some(scope) = controlled {
                        scope.arguments(values.as_slice(), offset)?;
                    }
                    let value = builtin.values(values.as_slice(), &context, offset)?;
                    load(if *native {
                        Cell::from_native_operand(value)?
                    } else {
                        Cell::from_operand(value)?
                    })
                }
                Operation::Construct(body) => {
                    let focus = if !wrapped && focus.value(input).is_some_and(|v| v.is_array()) {
                        let value = focus.value(input).expect("array focus");
                        let mut items = value.elements();
                        input.restore(Some(crate::construct::group_value((
                            items.next(),
                            items.collect(),
                        ))))
                    } else {
                        // A focus boundary does not discard an acquired port.
                        // Preserve its view/ownership; only grouping changes value.
                        focus.clone()
                    };
                    frame.region(body.workspace, |outer, child| {
                        body.run(
                            Activation {
                                focus: &focus,
                                wrapped: false,
                                outer,
                                scope: scopes.current.as_ref(),
                                ..activation
                            },
                            child,
                        )
                    })?
                }
                Operation::Object(members) => {
                    let mut values = Vec::with_capacity(members.len());
                    for (key, reg) in members {
                        if let Some(value) = frame[*reg].value(input) {
                            if let Some(scope) = controlled {
                                scope.arguments(&[Some(value.clone())], offset)?;
                            }
                            values.push((Value::StringLiteral(RawJson(key)), value));
                        }
                    }
                    Cell::Value(Value::object(values))
                }
                Operation::Keep(reg, force) => match frame[*reg].value(input) {
                    None => Cell::Missing,
                    Some(value) if *force && (!value.is_array() || value.preserves_array()) => {
                        Cell::Value(Value::kept(vec![value]))
                    }
                    Some(value) if value.unpacks_sequence() => {
                        let items: Vec<_> = value.elements().collect();
                        if items.is_empty() {
                            Cell::Missing
                        } else {
                            Cell::Value(Value::kept(items))
                        }
                    }
                    Some(value) => Cell::Value(value),
                },
                Operation::Array(items, preserve) => {
                    let mut values = Vec::new();
                    for &(reg, syntax) in items {
                        if let Some(value) = frame[reg].value(input) {
                            if let Some(scope) = controlled {
                                scope.arguments(&[Some(value.clone())], offset)?;
                            }
                            if value.is_array() && !syntax {
                                values.extend(value.elements());
                            } else {
                                values.push(value);
                            }
                        }
                    }
                    Cell::Value(Value::array(values, *preserve))
                }
                Operation::JumpFalse(test, to) => {
                    if !frame[*test].truth(input, offset)? {
                        pc = *to;
                        continue;
                    }
                    Cell::Missing
                }
                Operation::Jump(to) => {
                    pc = *to;
                    continue;
                }
                Operation::CorrelatedGroup(region, pairs) => context::group(
                    region,
                    pairs,
                    Activation {
                        outer: frame.view(),
                        scope: scopes.current.as_ref(),
                        ..activation
                    },
                    offset,
                )?,
                Operation::Group(..) | Operation::Order(..) => grouping::execute(
                    &instruction.kind,
                    Activation {
                        outer: frame.view(),
                        scope: scopes.current.as_ref(),
                        ..activation
                    },
                    frame.view(),
                    offset,
                )?,
                Operation::Collect(_)
                | Operation::Correlate(_)
                | Operation::Navigate(_)
                | Operation::Select(..)
                | Operation::Range(..)
                | Operation::Traverse(_)
                | Operation::Path(_)
                | Operation::Each { .. }
                | Operation::Reduce(..)
                | Operation::Fold { .. } => load(super::iteration::execute(
                    instruction,
                    Activation {
                        input,
                        focus,
                        wrapped,
                        args: &[],
                        outer: frame.view(),
                        calls,
                        scope: scopes.current.as_ref(),
                    },
                    reads,
                )?),
            };
            frame[instruction.out] = cell;
            pc += 1;
        }
        Ok(call::Exit::Done)
    }
}
// Callback inputs are language values only when demanded by the compiled body.
// The mask includes argument definitions retained for nested lexical captures.
pub(super) struct Arguments<'e, 'i> {
    mask: u8,
    values: [Cell<'e, 'i>; 3],
}
impl<'e, 'i> Arguments<'e, 'i> {
    pub(super) fn new(input: &Input<'_, 'i, '_>, source: &Cell<'e, 'i>, body: &Program) -> Self {
        let mask = body.argument_mask;
        let whole = if mask & 4 == 0 {
            Cell::Missing
        } else if let Some(value) = source.value(input)
            && !value.is_array()
        {
            Cell::Value(Value::array(vec![value], false))
        } else {
            source.clone()
        };
        Self {
            mask,
            values: [Cell::Missing, Cell::Missing, whole],
        }
    }
    pub(super) fn at(&mut self, item: &Cell<'e, 'i>, index: usize) -> &[Cell<'e, 'i>] {
        if self.mask == 0 {
            return &[];
        }
        if self.mask & 1 != 0 {
            self.values[0] = item.clone();
        }
        if self.mask & 2 != 0 {
            self.values[1] = Cell::Value(Value::Number(index as f64));
        }
        &self.values
    }
}
