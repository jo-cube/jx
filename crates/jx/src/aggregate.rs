use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Aggregate, Node},
    sequence::{Context, Halt},
    value::type_error,
};

impl Aggregate {
    pub(crate) fn evaluate<'e, 'i>(
        self,
        args: &'e [Node],
        input: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        let [argument] = args else {
            // Arguments finish evaluating before the function checks its arity.
            for argument in args {
                argument.run(input)?;
            }
            return Err(type_error(offset));
        };
        let mut fold = Fold::new(self);
        let mut first = None;
        let mut many = false;
        let mut consume = |value| {
            if many {
                fold.push(value);
            } else if let Some(pending) = first.take() {
                many = true;
                fold.push(pending);
                fold.push(value);
            } else {
                first = Some(value);
            }
            Ok(())
        };
        let result = argument.consume(input, &mut consume);
        match result {
            Err(Halt::Evaluation(error)) => return Err(error),
            Err(Halt::Stop) => unreachable!("aggregate consumes its complete argument"),
            Ok(()) => {}
        }
        // Retain only the first item until cardinality is known. A sole raw array
        // is the argument array; arrays inside a multi-item sequence stay members.
        let defined = many || !matches!(first, None | Some(Value::Undefined));
        if let Some(value) = first {
            match value {
                Value::Undefined => {}
                value if value.is_array() => {
                    value.elements().for_each(|item| fold.push(item));
                }
                value => fold.push(value),
            }
        }
        fold.finish(defined, offset)
    }
    pub(crate) fn retained<'e, 'i>(
        self,
        value: Option<Value<'e, 'i>>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        let defined = value.is_some();
        let mut fold = Fold::new(self);
        if let Some(value) = value {
            if value.is_array() {
                value.elements().for_each(|item| fold.push(item));
            } else {
                fold.push(value);
            }
        }
        fold.finish(defined, offset)
    }
}

pub(crate) struct Fold {
    aggregate: Aggregate,
    count: usize,
    number: Option<f64>,
    invalid: bool,
}
impl Fold {
    pub(crate) fn new(aggregate: Aggregate) -> Self {
        Self {
            aggregate,
            count: 0,
            number: None,
            invalid: false,
        }
    }
    pub(crate) fn finish<'e, 'i>(
        self,
        defined: bool,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        if self.invalid {
            return Err(type_error(offset));
        }
        let number = match self.aggregate {
            Aggregate::Count => Some(self.count as f64),
            Aggregate::Sum if defined => Some(self.number.unwrap_or(0.0)),
            _ => self.number,
        };
        Ok(number.map_or(Operand::Missing, |n| Operand::One(Value::Number(n))))
    }
    pub(crate) fn push(&mut self, value: Value<'_, '_>) {
        self.count += 1;
        if matches!(self.aggregate, Aggregate::Count) {
            return;
        }
        let Value::Number(number) = value.atomic() else {
            // Preserve upstream evaluation errors over argument-type errors.
            self.invalid = true;
            return;
        };
        self.number = Some(match self.aggregate {
            Aggregate::Sum => self.number.unwrap_or(0.0) + number,
            Aggregate::Min | Aggregate::Max => match self.number {
                None => number,
                Some(previous) if previous.is_nan() || number.is_nan() => f64::NAN,
                Some(previous) => {
                    let smaller =
                        number < previous || (number == previous && number.is_sign_negative());
                    if smaller == matches!(self.aggregate, Aggregate::Min) {
                        number
                    } else {
                        previous
                    }
                }
            },
            Aggregate::Count => unreachable!(),
        });
    }
}
