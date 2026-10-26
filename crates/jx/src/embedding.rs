use crate::{Error, ErrorKind, Evaluation, Expression, Value, sequence::Context};
use std::{rc::Rc, sync::Arc};

/// Declare external lexical names before compilation so specialization respects
/// possible builtin shadowing. Supply per-record values through evaluate_with or
/// immutable owned data through constant_binding.
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
    constants: Vec<(Box<str>, Arc<crate::OwnedValue>)>,
}
impl CompileOptions {
    pub fn binding(mut self, name: impl Into<Box<str>>) -> Self {
        self.bindings.push(name.into());
        self
    }
    /// Bind immutable owned data before optimization. Compile options and
    /// expressions share its storage; pass an Arc to share across options.
    /// Evaluation needs no binding setup.
    /// Local assignments/parameters may shadow the name. Runtime injection cannot
    /// replace it, and a name cannot also be declared with [`Self::binding`].
    ///
    /// ```
    /// let config = jx::OwnedValue::from_json(br#"{"scale":3}"#)?;
    /// let expression = jx::CompileOptions::default()
    ///     .constant_binding("config", config)
    ///     .compile("price * $config.scale")?;
    /// assert_eq!(expression.evaluate(br#"{"price":2}"#)?.single()?.unwrap().as_number(), Some(6.0));
    /// # Ok::<(), jx::Error>(())
    /// ```
    pub fn constant_binding(
        mut self,
        name: impl Into<Box<str>>,
        value: impl Into<Arc<crate::OwnedValue>>,
    ) -> Self {
        self.constants.push((name.into(), value.into()));
        self
    }
    pub fn compile(&self, source: &str) -> Result<Expression, Error> {
        Ok(Expression {
            execution: Arc::new(self.compile_program(source)?),
        })
    }
    /// Compile immutable common-IR code. Unsupported syntax fails explicitly.
    pub fn compile_program(&self, source: &str) -> Result<crate::execution::Program, Error> {
        self.validate_names()?;
        let parsed = crate::parse::semantic(source, &self.bindings, &self.constants)
            .map_err(Error::compilation)?;
        crate::execution::Program::compile_parsed(&parsed).map_err(|e| {
            Error::new(ErrorKind::UnsupportedExpression, e.offset, e.reason).compilation()
        })
    }
    fn validate_names(&self) -> Result<(), Error> {
        for name in self
            .bindings
            .iter()
            .chain(self.constants.iter().map(|(name, _)| name))
        {
            if name.is_empty() || name.starts_with('$') || name.starts_with('\0') {
                return Err(Error::new(
                    ErrorKind::BindingError,
                    0,
                    "binding names omit '$' and must be nonempty",
                )
                .compilation());
            }
        }
        for (index, (name, _)) in self.constants.iter().enumerate() {
            if self.bindings.contains(name)
                || self.constants[..index].iter().any(|(n, _)| n == name)
            {
                return Err(Error::new(
                    ErrorKind::BindingError,
                    0,
                    "constant binding names must be unique and cannot also be runtime bindings",
                )
                .compilation());
            }
        }
        Ok(())
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
        self.execution.evaluate_with(input, options)
    }
    pub fn evaluate_validated_with(
        &'e self,
        input: Option<crate::RawJson<'i>>,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.execution.evaluate_validated_with(input, options)
    }
}
pub(crate) fn validate_options(
    declared: &[Box<str>],
    options: &EvaluationOptions<'_, '_>,
) -> Result<bool, Error> {
    for (name, value) in &options.bindings {
        if !declared.iter().any(|n| n.as_ref() == *name) {
            return Err(Error::new(
                ErrorKind::BindingError,
                0,
                "external binding was not declared at compilation",
            ));
        }
        external(value)?;
    }
    options.focus.as_ref().map_or(Ok(false), external)
}

// JSONata closures carry arena indices: accepting one from another evaluation
// would be unsound semantically even though Rust's bytes still live. Host values
// have independent ownership and can be safely injected into any evaluation.
fn external(value: &Value<'_, '_>) -> Result<bool, Error> {
    match value {
        _ if value.as_raw().is_some() || matches!(value, Value::Constant(_)) => Ok(false),
        Value::Function(f) if !matches!(f.kind, crate::function::FunctionKind::Host(_)) => {
            Err(Error::new(
                ErrorKind::BindingError,
                0,
                "only host functions can cross evaluation boundaries",
            ))
        }
        Value::Function(_) => Ok(true),
        _ if value.is_array() => {
            let mut functions = false;
            for v in value.elements() {
                functions |= external(&v)?;
            }
            Ok(functions)
        }
        _ if value.is_object() => {
            let mut functions = false;
            for (_, v) in value.members() {
                functions |= external(&v)?;
            }
            Ok(functions)
        }
        _ => Ok(false),
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
