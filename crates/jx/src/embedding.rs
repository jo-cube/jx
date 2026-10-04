use crate::{Error, ErrorKind, Evaluation, Expression, Value, evaluate, sequence::Context};
use std::{rc::Rc, sync::Arc};

/// Declare external lexical names before compilation so specialization respects
/// possible builtin shadowing. Per-record values arrive through evaluate_with.
///
/// ```
/// let expression = jx::CompileOptions::default().binding("scale")
///     .compile("price * $scale")?;
/// let value = expression.evaluate_with(Some(br#"{"price":2.5}"#), jx::EvaluationOptions {
///     bindings: vec![("scale", jx::Value::Number(3.0))],
///     ..Default::default()
/// })?.single()?.unwrap();
/// assert_eq!(value.as_number(), Some(7.5));
/// let owned = value.to_owned()?;
/// assert_eq!(owned.as_value().as_number(), Some(7.5));
/// # Ok::<(), jx::Error>(())
/// ```
///
/// Declare names without '$' before optimization. Declared functions are always
/// effectful; no host purity promise or mutable state enters compiled plans.
#[derive(Clone, Debug, Default)]
pub struct CompileOptions {
    bindings: Vec<Box<str>>,
}
impl CompileOptions {
    pub fn binding(mut self, name: impl Into<Box<str>>) -> Self {
        self.bindings.push(name.into());
        self
    }
    pub fn compile(&self, source: &str) -> Result<Expression, Error> {
        for name in &self.bindings {
            if name.is_empty() || name.starts_with('$') || name.starts_with('\0') {
                return Err(Error::new(
                    ErrorKind::BindingError,
                    0,
                    "binding names omit '$' and must be nonempty",
                )
                .compilation());
            }
        }
        crate::parse::configured(source, &self.bindings).map_err(Error::compilation)
    }
}

/// Per-evaluation settings; values are cloned into the lexical frame, not replayed.
/// None input denotes missing; focus replaces '$' while '$$' retains the input.
#[derive(Default)]
pub struct EvaluationOptions<'e, 'i> {
    pub bindings: Vec<(&'e str, Value<'e, 'i>)>,
    pub focus: Option<Value<'e, 'i>>,
    pub random: Option<crate::Random>,
    pub limits: Option<crate::Limits>,
    pub cancellation: Option<crate::Cancellation>,
    pub deadline: Option<std::time::Instant>,
}
impl<'e, 'i> Expression {
    pub fn evaluate_with(
        &'e self,
        input: Option<&'i [u8]>,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        if options.bindings.is_empty()
            && options.focus.is_none()
            && options.limits.is_none()
            && options.cancellation.is_none()
            && options.deadline.is_none()
            && let Some(input) = input
        {
            return match options.random.as_ref() {
                Some(random) => self.evaluate_with_random(input, random),
                None => self.evaluate(input),
            };
        }
        // Validation precedes bindings, control failures, and host effects.
        let root = input
            .map(crate::validate)
            .transpose()?
            .map_or(Value::Undefined, Value::Raw);
        for (name, value) in &options.bindings {
            if !self.bindings.iter().any(|n| n.as_ref() == *name) {
                return Err(Error::new(
                    ErrorKind::BindingError,
                    0,
                    "external binding was not declared at compilation",
                ));
            }
            external(value)?;
        }
        if let Some(focus) = &options.focus {
            external(focus)?;
        }
        let control =
            crate::controls::Control::new(options.limits, options.cancellation, options.deadline);
        let scope = if self.runtime || !options.bindings.is_empty() || control.is_some() {
            let scope = crate::runtime::Scope::with_random(
                root.clone(),
                self.root.clock,
                options.random.as_ref(),
            )
            .configure(options.bindings, control.clone());
            scope.checkpoint(self.root.offset)?;
            Some(scope)
        } else {
            None
        };
        let context = Context {
            value: options.focus.unwrap_or(root),
            wrapped: true,
            scope,
        };
        let result = if let Some(stream) = self.root.stream(&context) {
            evaluate::results(crate::evaluate::Operand::Many(stream))
        } else {
            evaluate::results(self.root.run(&context)?)
        };
        Ok(Evaluation {
            result: match control {
                Some(c) => evaluate::Results::Controlled(Box::new((result, c))),
                None => result,
            },
        })
    }
}
// JSONata closures carry arena indices: accepting one from another evaluation
// would be unsound semantically even though Rust's bytes still live. Host values
// have independent ownership and can be safely injected into any evaluation.
fn external(value: &Value<'_, '_>) -> Result<(), Error> {
    match value {
        Value::Raw(_) | Value::Constant(_) => Ok(()),
        Value::Function(f) if !matches!(f.kind, crate::function::FunctionKind::Host(_)) => {
            Err(Error::new(
                ErrorKind::BindingError,
                0,
                "only host functions can cross evaluation boundaries",
            ))
        }
        _ if value.is_array() => {
            for v in value.elements() {
                external(&v)?;
            }
            Ok(())
        }
        _ if value.is_object() => {
            for (_, v) in value.members() {
                external(&v)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

type Implementation = dyn for<'c, 'e, 'i> Fn(
        &[Option<Value<'e, 'i>>],
        &HostContext<'c, 'e, 'i>,
    ) -> Result<Option<Value<'e, 'i>>, Error>
    + Send
    + Sync;
/// Synchronous, effectful host callable. Arguments/focus may be returned by clone;
/// newly created values own their storage. Panics follow normal Rust panic policy.
#[derive(Clone)]
pub struct HostFunction {
    pub(crate) arity: usize,
    implementation: Arc<Implementation>,
    signature: Option<Arc<crate::function::Signature>>,
}
impl std::fmt::Debug for HostFunction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostFunction")
            .field("arity", &self.arity)
            .finish_non_exhaustive()
    }
}
impl HostFunction {
    pub fn new(
        arity: usize,
        implementation: impl for<'c, 'e, 'i> Fn(
            &[Option<Value<'e, 'i>>],
            &HostContext<'c, 'e, 'i>,
        ) -> Result<Option<Value<'e, 'i>>, Error>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            arity,
            implementation: Arc::new(implementation),
            signature: None,
        }
    }
    /// Compile the same argument signature language used by JSONata lambdas.
    pub fn with_signature(mut self, signature: &str) -> Result<Self, Error> {
        if !signature.starts_with('<') || !signature.ends_with('>') {
            return Err(Error::new(
                ErrorKind::SignatureError,
                0,
                "signature must be enclosed in '<' and '>'",
            )
            .compilation());
        }
        self.signature = Some(Arc::new(
            crate::function::Signature::compile(signature, 0).map_err(Error::compilation)?,
        ));
        Ok(self)
    }
    pub fn value<'e, 'i>(&self) -> Value<'e, 'i> {
        Value::Function(Rc::new(crate::Function {
            kind: crate::function::FunctionKind::Host(self.clone()),
        }))
    }
    pub(crate) fn invoke<'e, 'i>(
        &self,
        arguments: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
        validate: bool,
    ) -> Result<crate::evaluate::Operand<'e, 'i>, Error> {
        if let Some(scope) = &context.scope {
            scope.arguments(arguments, offset)?;
        }
        let validated;
        let arguments = if validate && let Some(signature) = &self.signature {
            validated = signature.validate(arguments, &context.value, offset)?;
            validated.as_slice()
        } else {
            arguments
        };
        let call = HostContext { context, offset };
        let invoke = || {
            (self.implementation)(arguments, &call)
                .map(|v| {
                    v.map_or(
                        crate::evaluate::Operand::Missing,
                        crate::evaluate::Operand::One,
                    )
                })
                .map_err(|cause| {
                    Error::new(ErrorKind::HostError, offset, "host function failed")
                        .with_cause(cause)
                })
        };
        context
            .scope
            .as_ref()
            .expect("host call runtime")
            .call(offset, 0, invoke)
    }
}

/// Borrowed capability for one synchronous host invocation. Calls into JSONata
/// retain results inside the same lexical arena and share its resource controls.
pub struct HostContext<'call, 'e, 'i> {
    context: &'call Context<'e, 'i>,
    offset: usize,
}
impl<'e, 'i> HostContext<'_, 'e, 'i> {
    pub fn focus(&self) -> &Value<'e, 'i> {
        &self.context.value
    }
    pub fn root(&self) -> Value<'e, 'i> {
        self.context
            .scope
            .as_ref()
            .and_then(|s| s.lookup("$"))
            .unwrap_or(Value::Undefined)
    }
    pub fn checkpoint(&self) -> Result<(), Error> {
        self.context.scope.as_ref().unwrap().checkpoint(self.offset)
    }
    pub fn invoke(
        &self,
        function: &Value<'e, 'i>,
        arguments: &[Option<Value<'e, 'i>>],
    ) -> Result<Option<Value<'e, 'i>>, Error> {
        let Value::Function(function) = function else {
            return Err(crate::value::type_error(self.offset));
        };
        crate::function::retained(crate::function::invoke(
            function,
            arguments,
            self.context,
            self.offset,
        )?)
    }
}
