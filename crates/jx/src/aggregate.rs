use crate::{Error, Value, evaluate::Operand, expression::Aggregate, value::type_error};

impl Aggregate {
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
            Aggregate::Average => self.number.map(|n| n / self.count as f64),
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
        self.number_value(number);
    }

    pub(crate) fn push_number(&mut self, number: f64) {
        self.count += 1;
        if !matches!(self.aggregate, Aggregate::Count) {
            self.number_value(number);
        }
    }
    fn number_value(&mut self, number: f64) {
        self.number = Some(match self.aggregate {
            Aggregate::Sum | Aggregate::Average => self.number.unwrap_or(0.0) + number,
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
