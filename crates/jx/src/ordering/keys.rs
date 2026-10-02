use crate::{Error, Value, expression::Node};
use std::cmp::Ordering;

type Column<'e, 'i> = Vec<Option<Option<Value<'e, 'i>>>>;

pub(crate) struct Keys<'e, 'i> {
    // Only replay-safe terms retain results. Allocate a column on its first
    // comparison so short-circuited secondary keys remain entirely unevaluated.
    columns: Vec<Option<Column<'e, 'i>>>,
    length: usize,
}
impl<'e, 'i> Keys<'e, 'i> {
    pub fn new(length: usize, terms: &[(Node, bool)]) -> Self {
        Self {
            columns: if length > 2 && terms.iter().any(|(term, _)| !term.effects) {
                (0..terms.len()).map(|_| None).collect()
            } else {
                Vec::new()
            },
            length,
        }
    }
    fn get(
        &mut self,
        index: usize,
        term_index: usize,
        term: &'e Node,
        evaluate: &mut impl FnMut(usize, &'e Node) -> Result<Option<Value<'e, 'i>>, Error>,
    ) -> Result<Option<Value<'e, 'i>>, Error> {
        if term.effects || self.columns.is_empty() {
            return evaluate(index, term);
        }
        let column = self.columns[term_index].get_or_insert_with(|| vec![None; self.length]);
        if let Some(value) = &column[index] {
            return Ok(value.clone());
        }
        let value = evaluate(index, term)?.map(|value| value.atomic());
        column[index] = Some(value.clone());
        Ok(value)
    }
    pub fn compare(
        &mut self,
        left: usize,
        right: usize,
        terms: &'e [(Node, bool)],
        offset: usize,
        mut evaluate: impl FnMut(usize, &'e Node) -> Result<Option<Value<'e, 'i>>, Error>,
    ) -> Result<Ordering, Error> {
        for (i, (term, descending)) in terms.iter().enumerate() {
            let a = self.get(left, i, term, &mut evaluate)?;
            let b = self.get(right, i, term, &mut evaluate)?;
            let missing = a.is_none() || b.is_none();
            let order = super::values(a, b, offset)?;
            if order != Ordering::Equal {
                return Ok(if *descending && !missing {
                    order.reverse()
                } else {
                    order
                });
            }
        }
        Ok(Ordering::Equal)
    }
}
