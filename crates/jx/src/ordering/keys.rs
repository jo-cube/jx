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
        Self::with_terms(
            length,
            terms.len(),
            terms.iter().any(|(term, _)| !term.effects),
        )
    }
    pub fn with_terms(length: usize, count: usize, replay_safe: bool) -> Self {
        Self {
            columns: if length > 2 && replay_safe {
                (0..count).map(|_| None).collect()
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
        effects: bool,
        evaluate: &mut impl FnMut(usize, usize) -> Result<Option<Value<'e, 'i>>, Error>,
    ) -> Result<Option<Value<'e, 'i>>, Error> {
        if effects || self.columns.is_empty() {
            return evaluate(index, term_index);
        }
        let column = self.columns[term_index].get_or_insert_with(|| vec![None; self.length]);
        if let Some(value) = &column[index] {
            return Ok(value.clone());
        }
        let value = evaluate(index, term_index)?.map(|value| value.atomic());
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
        self.compare_with(
            left,
            right,
            terms
                .iter()
                .map(|(term, descending)| (term.effects, *descending)),
            offset,
            |index, term| evaluate(index, &terms[term].0),
        )
    }
    pub fn compare_with(
        &mut self,
        left: usize,
        right: usize,
        terms: impl Iterator<Item = (bool, bool)>,
        offset: usize,
        mut evaluate: impl FnMut(usize, usize) -> Result<Option<Value<'e, 'i>>, Error>,
    ) -> Result<Ordering, Error> {
        for (i, (effects, descending)) in terms.enumerate() {
            let a = self.get(left, i, effects, &mut evaluate)?;
            let b = self.get(right, i, effects, &mut evaluate)?;
            let missing = a.is_none() || b.is_none();
            let order = super::values(a, b, offset)?;
            if order != Ordering::Equal {
                return Ok(if descending && !missing {
                    order.reverse()
                } else {
                    order
                });
            }
        }
        Ok(Ordering::Equal)
    }
}
