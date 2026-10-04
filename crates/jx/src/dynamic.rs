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
            Self::Eval => evaluate(args, None, context, offset),
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
    program: Option<Result<crate::Expression, Error>>,
}
impl Call {
    pub fn prepare(args: Box<[Node]>) -> Self {
        let program = args.first().and_then(|n| match &n.kind {
            Kind::String(s) => Some(
                source(&Value::StringLiteral(crate::RawJson(s)), n.offset)
                    .and_then(|s| crate::parse::dynamic(&s)),
            ),
            Kind::Prepared(p) if matches!(p.data, crate::constant::Data::String(_)) => {
                Some(source(&p.data.value(), n.offset).and_then(|s| crate::parse::dynamic(&s)))
            }
            _ => None,
        });
        Self { args, program }
    }
    pub fn needs_runtime(&self) -> bool {
        self.program
            .as_ref()
            .is_none_or(|p| p.as_ref().is_ok_and(|p| p.runtime))
    }
    pub fn needs_clock(&self) -> bool {
        self.program
            .as_ref()
            .is_none_or(|p| p.as_ref().is_ok_and(|p| p.root.clock))
    }
    pub fn evaluate<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        let target = Self::target(context);
        let arguments = Arguments::evaluate(&self.args, context)?;
        let target = Self::resolve(target, offset)?;
        if let Some(function) = target {
            return crate::function::invoke(&function, arguments.as_slice(), context, offset);
        }
        self.values(arguments.as_slice(), context, offset)
    }
    pub fn target<'e, 'i>(context: &Context<'e, 'i>) -> Option<Value<'e, 'i>> {
        context.scope.as_ref().and_then(|s| s.lookup("eval"))
    }
    pub fn resolve<'e, 'i>(
        target: Option<Value<'e, 'i>>,
        offset: usize,
    ) -> Result<Option<Rc<crate::Function<'e, 'i>>>, Error> {
        match target {
            None => Ok(None),
            Some(Value::Function(function))
                if matches!(
                    function.kind,
                    FunctionKind::Builtin(crate::builtin::Builtin::Runtime(Builtin::Eval))
                ) =>
            {
                Ok(None)
            }
            Some(Value::Function(function)) => Ok(Some(function)),
            Some(_) => Err(type_error(offset)),
        }
    }
    pub fn values<'e, 'i>(
        &'e self,
        arguments: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        evaluate(arguments, self.program.as_ref(), context, offset)
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
fn evaluate<'e, 'i>(
    args: &[Option<Value<'e, 'i>>],
    prepared: Option<&'e Result<crate::Expression, Error>>,
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    if !(1..=2).contains(&args.len()) {
        return Err(type_error(offset));
    }
    let Some(text) = args[0].as_ref().filter(|v| !matches!(v, Value::Undefined)) else {
        return Ok(Operand::Missing);
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
    let result = if let Some(program) = prepared {
        let program = program
            .as_ref()
            .map_err(|e| wrapped(e.clone(), ErrorKind::EvalSyntax, offset))?;
        let local = Context {
            value: focus,
            wrapped: wrapped_focus,
            scope: context.scope.clone(),
        };
        if let Some(scope) = &context.scope {
            scope.call(offset, program.root.depth, || {
                crate::retain::materialize(&program.root, &local)
            })
        } else {
            crate::retain::materialize(&program.root, &local)
        }
    } else {
        let source = source(text, offset)?;
        let program = crate::parse::dynamic(&source)
            .map_err(|e| wrapped(e, ErrorKind::EvalSyntax, offset))?;
        let scope = context.scope.as_ref().expect("dynamic eval runtime");
        let mut definitions = scope.dynamic_definitions();
        borrow::collect(&focus, &mut definitions);
        scope.bridge(
            borrow::value(&focus, &definitions),
            wrapped_focus,
            |local| {
                local.scope.as_ref().unwrap().loan_dynamic(&definitions);
                local
                    .scope
                    .as_ref()
                    .unwrap()
                    .call(offset, program.root.depth, || {
                        crate::retain::materialize(&program.root, local)
                    })
            },
        )
    };
    result
        .map(|v| v.map_or(Operand::Missing, Operand::One))
        .map_err(|e| wrapped(e, ErrorKind::EvalError, offset))
}

#[derive(Debug)]
pub(crate) enum Definition {
    Lambda(crate::function::Definition),
    Transform(crate::transform::Definition),
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
            Definition::Lambda(d) => d.params.len(),
            Definition::Transform(_) => 1,
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
        if !scope.controlled()
            && let Definition::Lambda(definition) = self.definition.as_ref()
            && !definition.tail
            && let Some(plan) = &definition.plan
        {
            let validated;
            let arguments = if validate && let Some(signature) = &definition.signature {
                validated = signature.validate(args, &context.value, offset)?;
                validated.as_slice()
            } else {
                args
            };
            // Pure plans cannot read or write lexical frames. Retain only their
            // result; unsupported shapes still use the scope bridge below.
            let result = scope.call(offset, definition.body.depth, || {
                Ok(plan.retained(arguments, &self.focus, self.wrapped))
            })?;
            if let Some(result) = result {
                return Ok(result);
            }
        }
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
                Definition::Lambda(definition) => FunctionKind::Lambda {
                    definition,
                    focus: borrow::value(&self.focus, &definitions),
                    wrapped: self.wrapped,
                    frame: self.frame,
                },
                Definition::Transform(definition) => FunctionKind::Transform {
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
