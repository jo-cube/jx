pub(crate) mod borrow;
mod retention;
use crate::{
    Error, ErrorKind, Value,
    evaluate::Operand,
    expression::{Kind, Node},
    function::{Arguments, FunctionKind},
    sequence::Context,
    value::type_error,
};
pub(crate) use retention::{Retention, same};
use std::{borrow::Cow, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Eval,
    Random,
    Shuffle,
}
impl Builtin {
    pub fn arity(self) -> usize {
        match self {
            Self::Eval => 2,
            Self::Random => 0,
            Self::Shuffle => 1,
        }
    }
    pub fn partial_values<'e, 'i>(
        self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        if self == Self::Eval
            && args
                .first()
                .and_then(Option::as_ref)
                .is_some_and(|v| !matches!(v, Value::Undefined) && v.string_body().is_none())
        {
            return Err(Error::new(
                ErrorKind::EvalSyntax,
                offset,
                "D3120: dynamic source must be a string",
            ));
        }
        if self == Self::Shuffle
            && args
                .first()
                .and_then(Option::as_ref)
                .is_some_and(|v| !matches!(v, Value::Undefined) && !v.is_array())
        {
            return Err(Error::new(
                ErrorKind::UnsupportedExpression,
                offset,
                "untyped native shuffle coercions are deferred",
            ));
        }
        self.values(args, context, offset)
    }
    pub fn values<'e, 'i>(
        self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        if let Some(scope) = &context.scope {
            scope.arguments(args, offset)?;
        }
        match self {
            Self::Eval => evaluate(args, context, offset),
            Self::Random if args.is_empty() => Ok(Operand::One(Value::Number(
                context
                    .scope
                    .as_ref()
                    .expect("effects runtime")
                    .random()
                    .draw(),
            ))),
            Self::Shuffle if args.len() == 1 => shuffle(args[0].clone(), context, offset),
            _ => Err(type_error(offset)),
        }
    }
}
fn shuffle<'e, 'i>(
    value: Option<Value<'e, 'i>>,
    context: &Context<'e, 'i>,
    _offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    let Some(value) = value.filter(|v| !matches!(v, Value::Undefined)) else {
        return Ok(Operand::Missing);
    };
    if !value.is_array() {
        return Ok(Operand::One(Value::array(vec![value], false)));
    }
    let mut items = value.elements();
    let Some(first) = items.next() else {
        return Ok(Operand::One(value));
    };
    let Some(second) = items.next() else {
        return Ok(Operand::One(value));
    };
    let random = context.scope.as_ref().expect("effects runtime").random();
    let mut result = Vec::new();
    for item in [first, second].into_iter().chain(items) {
        let index = result.len();
        let at = (random.draw() * (index + 1) as f64) as usize;
        result.push(item);
        result.swap(index, at);
    }
    Ok(Operand::One(Value::array(result, false)))
}

#[derive(Clone, Debug)]
pub(crate) struct Call {
    pub args: Box<[Node]>,
    pub program: Option<std::sync::Arc<PreparedProgram>>,
}
impl Call {
    pub fn prepare(args: Box<[Node]>) -> Self {
        let program = PreparedProgram::new(&args);
        Self { args, program }
    }

    pub fn needs_clock(&self) -> bool {
        self.program
            .as_ref()
            .is_none_or(|p| p.body().is_some_and(crate::execution::Program::needs_clock))
    }
}
fn source<'a>(value: &'a Value<'_, '_>, offset: usize) -> Result<Cow<'a, str>, Error> {
    let body = value.string_body().ok_or_else(|| type_error(offset))?;
    if !body.as_bytes().contains(&b'\\') {
        return Ok(Cow::Borrowed(body));
    }
    char::decode_utf16(crate::json::string::units(body))
        .collect::<Result<String, _>>()
        .map(Cow::Owned)
        .map_err(|_| {
            Error::new(
                ErrorKind::EvalSyntax,
                offset,
                "D3120: isolated surrogate in dynamic source",
            )
        })
}
fn wrapped(mut error: Error, kind: ErrorKind, offset: usize) -> Error {
    if error.source == crate::Source::Expression {
        error.source = crate::Source::DynamicExpression;
    }
    let wrapped = Error::custom(
        kind,
        offset,
        format!(
            "{}: {}",
            if kind == ErrorKind::EvalSyntax {
                "D3120"
            } else {
                "D3121"
            },
            error
        ),
    );
    wrapped.with_cause(error)
}
// Argument validation and focus substitution are shared by cached and dynamic code.
fn evaluation_focus<'e, 'i>(
    args: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Option<(Value<'e, 'i>, bool)>, Error> {
    if !(1..=2).contains(&args.len()) {
        return Err(type_error(offset));
    }
    let Some(text) = args[0].as_ref().filter(|v| !matches!(v, Value::Undefined)) else {
        return Ok(None);
    };
    if text.string_body().is_none() {
        return Err(type_error(offset));
    }
    let replacement = args
        .get(1)
        .cloned()
        .flatten()
        .filter(|v| !matches!(v, Value::Undefined));
    let wrapped_focus = replacement
        .as_ref()
        .map_or(context.wrapped, |v| v.is_array() && !v.is_sequence());
    let focus = replacement.unwrap_or_else(|| context.value.clone());
    Ok(Some((focus, wrapped_focus)))
}

/// Immutable code attached to a literal or immutable-source call site.
/// Stores code or a lazy syntax error, never environments or evaluated values.

#[derive(Debug, Clone)]
pub(crate) struct PreparedProgram {
    program: Result<crate::execution::Program, Error>,
    syntax_error: bool,
}

impl PreparedProgram {
    pub(crate) fn new(args: &[Node]) -> Option<std::sync::Arc<Self>> {
        let node = args.first()?;
        let value = match &node.kind {
            Kind::String(s) => Value::StringLiteral(crate::RawJson(s)),
            Kind::Prepared(p) if matches!(*p.data, crate::constant::Data::String(_)) => {
                p.data.value()
            }
            _ => return None,
        };
        let (program, syntax_error) = match source(&value, node.offset)
            .and_then(|text| crate::parse::dynamic_semantic(&text))
        {
            Ok(e) => (crate::execution::Program::compile_dynamic(&e), false),
            Err(e) => (Err(e), true),
        };
        Some(std::sync::Arc::new(Self {
            program,
            syntax_error,
        }))
    }
    pub(crate) fn body(&self) -> Option<&crate::execution::Program> {
        self.program.as_ref().ok()
    }
    pub(crate) fn evaluate<'e, 'i>(
        &'e self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        input: &crate::execution::input::Input<'_, 'i, '_>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        let scope = context.scope.as_ref().expect("dynamic eval runtime");
        scope.checkpoint(offset)?;
        scope.arguments(args, offset)?;
        let Some((value, wrapped_focus)) = evaluation_focus(args, context, offset)? else {
            return Ok(Operand::Missing);
        };
        let program = self.program.as_ref().map_err(|e| {
            if self.syntax_error {
                wrapped(e.clone(), ErrorKind::EvalSyntax, offset)
            } else {
                e.clone()
            }
        })?;
        let local = Context {
            value,
            wrapped: wrapped_focus,
            scope: context.scope.clone(),
        };
        // Code outlives evaluation. The ordinary arena retains closures/writes;
        // only genuinely dynamic short-lived code needs a lifetime bridge.
        scope
            .call(offset, program.call.depth, || {
                program.evaluate_region_in(&local, Some(input))
            })
            .map(|v| v.map_or(Operand::Missing, Operand::One))
            .map_err(|e| {
                if e.kind == ErrorKind::UnsupportedExpression
                    && e.source == crate::Source::DynamicExpression
                {
                    e
                } else {
                    wrapped(e, ErrorKind::EvalError, offset)
                }
            })
    }
}

fn evaluate<'e, 'i>(
    args: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    let Some((focus, wrapped_focus)) = evaluation_focus(args, context, offset)? else {
        return Ok(Operand::Missing);
    };
    let text = args[0].as_ref().expect("source validated");

    // Dynamic code has a shorter source lifetime. The existing scope bridge
    // retains escaping IR definitions and preserves imported value identity.
    // No AST body can execute in this domain, including nested $eval calls.
    let source = source(text, offset)?;
    let expression = crate::parse::dynamic_semantic(&source)
        .map_err(|e| wrapped(e, ErrorKind::EvalSyntax, offset))?;
    let program = crate::execution::Program::compile_dynamic(&expression)?;
    let scope = context.scope.as_ref().unwrap();
    let mut definitions = scope.dynamic_definitions();
    borrow::collect(&focus, &mut definitions);
    scope
        .bridge(
            borrow::value(&focus, &definitions),
            wrapped_focus,
            |local| {
                let scope = local.scope.as_ref().unwrap();
                scope.loan_dynamic(&definitions);
                scope.call(offset, program.call.depth, || {
                    program.evaluate_region(local)
                })
            },
        )
        .map(|v| v.map_or(Operand::Missing, Operand::One))
        .map_err(|e| {
            // Lowering rejection is an execution-domain boundary, not a
            // JSONata failure. Keep it explicit through nested $eval calls.
            if e.kind == ErrorKind::UnsupportedExpression
                && e.source == crate::Source::DynamicExpression
            {
                e
            } else {
                wrapped(e, ErrorKind::EvalError, offset)
            }
        })
}

#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "Definitions are already shared behind Rc; boxing adds another allocation."
)]
pub(crate) enum Definition {
    Program(crate::execution::call::Definition),

    ProgramTransform(Box<crate::transform::Definition<crate::execution::Program>>),
}
#[derive(Debug)]
pub(crate) struct Callable<'e, 'i> {
    pub definition: Rc<Definition>,
    pub focus: Value<'e, 'i>,
    pub wrapped: bool,
    pub frame: usize,
}
impl<'e, 'i> Callable<'e, 'i> {
    pub fn arity(&self) -> usize {
        match self.definition.as_ref() {
            Definition::Program(d) => d.params.len(),

            Definition::ProgramTransform(_) => 1,
        }
    }
    pub fn invoke(
        &self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
        validate: bool,
    ) -> Result<Operand<'e, 'i>, Error> {
        let scope = context.scope.as_ref().expect("dynamic closure runtime");
        let mut definitions = scope.dynamic_definitions();
        if !definitions.iter().any(|d| Rc::ptr_eq(d, &self.definition)) {
            definitions.push(self.definition.clone());
        }
        borrow::collect(&self.focus, &mut definitions);
        for value in args.iter().flatten() {
            borrow::collect(value, &mut definitions);
        }
        let result = scope.bridge(context.value.clone(), context.wrapped, |local| {
            local.scope.as_ref().unwrap().loan_dynamic(&definitions);
            let mut arguments = Arguments::new(args.len());
            for v in args {
                arguments.push(v.as_ref().map(|v| borrow::value(v, &definitions)));
            }
            let kind = match self.definition.as_ref() {
                Definition::Program(definition) => FunctionKind::Program {
                    definition,
                    focus: borrow::value(&self.focus, &definitions),
                    wrapped: self.wrapped,
                    frame: self.frame,
                },

                Definition::ProgramTransform(definition) => FunctionKind::ProgramTransform {
                    definition,
                    frame: self.frame,
                },
            };
            let function = crate::Function { kind };
            crate::function::retained(crate::function::invoke_checked(
                &function,
                arguments.as_slice(),
                local,
                offset,
                validate,
            )?)
        })?;
        Ok(result.map_or(Operand::Missing, Operand::One))
    }
}
