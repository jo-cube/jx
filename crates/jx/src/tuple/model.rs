//! Correlation is an explicit sequence property, independent of executable bodies.
use super::*;
pub(crate) enum Form<'a, B> {
    Value,
    Route(&'a [Step<B>], bool),
    Scope(&'a [B]),
    Keep(&'a B),
    Filter(&'a B, &'a [B]),
    Order(&'a B, &'a [(B, bool)]),
}
pub(crate) trait Expression: Sized {
    fn form(&self) -> Form<'_, Self>;
    fn offset(&self) -> usize;
    fn boolean(&self) -> bool;
    fn effects(&self) -> bool;
    fn position(&self) -> Option<f64>;
    fn absolute(&self) -> bool;
    fn sorted_tuple(&self) -> bool {
        matches!(self.form(), Form::Order(base, _) if correlated(base))
    }
}
// Only body execution and the uncorrelated prefix differ during migration.
// Correlation, cardinality, ordering, scopes and error precedence have one kernel.
pub(crate) trait Evaluator<'e, 'i, B: Expression + 'e> {
    fn parent(&self, _body: &'e B, _value: &Value<'e, 'i>) -> Option<Value<'e, 'i>> {
        None
    }

    fn run(&self, body: &'e B, context: &Context<'e, 'i>) -> Result<Operand<'e, 'i>, Error>;
    fn filtered(
        &self,
        body: &'e B,
        tests: &'e [B],
        context: &Context<'e, 'i>,
        output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
    ) -> Walk;
    fn prefix(
        &self,
        steps: &'e [Step<B>],
        focus: bool,
        context: &Context<'e, 'i>,
        output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
    ) -> Walk;
}
impl Expression for Node {
    fn form(&self) -> Form<'_, Self> {
        match &self.kind {
            Kind::Tuples(s, f) => Form::Route(s, *f),
            Kind::Keep(n, _) if correlated(&**n) => Form::Keep(n),
            Kind::Filter(n, p) if correlated(&**n) => Form::Filter(n, p),
            Kind::Sort(n, t) if correlated(&**n) => Form::Order(n, t),
            Kind::Group(n) if crate::provenance::captured(n) => {
                Form::Scope(std::slice::from_ref(n))
            }
            Kind::Block(n) if n.last().is_some_and(crate::provenance::captured) => Form::Scope(n),
            _ => Form::Value,
        }
    }
    fn offset(&self) -> usize {
        self.offset
    }
    fn boolean(&self) -> bool {
        super::stages::boolean(self)
    }
    fn effects(&self) -> bool {
        self.effects
    }
    fn position(&self) -> Option<f64> {
        if let Kind::Number(n) = self.kind {
            Some(n)
        } else {
            None
        }
    }
    fn absolute(&self) -> bool {
        matches!(&self.kind, Kind::Variable(_) | Kind::Sort(..))
            || matches!(&self.kind, Kind::Prepared(p) if p.data.is_shared())
            || matches!(&self.kind, Kind::Path(p) if p.rooted && p.fields.is_empty())
    }
}
pub(super) fn materialize<'e, 'i, B: Expression + 'e>(
    node: &'e B,
    context: &Context<'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Result<Option<Value<'e, 'i>>, Error> {
    match evaluator.run(node, context)? {
        Operand::Missing => Ok(None),
        Operand::One(value) => Ok(Some(value)),
        Operand::Many(stream) => crate::retain::collect(|emit| {
            stream.visit(|v| {
                emit(v);
                Ok(())
            })
        }),
    }
}
