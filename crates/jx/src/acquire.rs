use crate::{
    Error, Evaluation, Value,
    builtin::Builtin,
    evaluate::{Operand, pure},
    expression::{Kind, Node, Op, Path},
    json::{Captured, Captures, Demand},
    sequence::Context,
};

/// Acquisition only: the tree and its existing plans still execute the region.
#[derive(Clone, Debug)]
pub(crate) struct Region {
    demand: Demand,
    paths: Box<[Path]>,
}
pub(crate) fn paths(node: &Node, minimum_loads: usize) -> Option<Box<[Path]>> {
    let mut paths = Vec::new();
    let mut loads = 0;
    fn collect(node: &Node, paths: &mut Vec<Path>, loads: &mut usize) -> Option<()> {
        if node.effects || node.clock || node.tail_call {
            return None;
        }
        match &node.kind {
            Kind::Path(path) if !path.fields.is_empty() && !path.rooted => {
                *loads += 1;
                if !paths.iter().any(|p| p.fields == path.fields) {
                    if paths.len() == crate::json::CAPTURE_SLOTS {
                        return None;
                    }
                    paths.push(path.clone());
                }
            }
            Kind::Plan(plan) if plan.scalar() => collect(&plan.source, paths, loads)?,
            Kind::Group(n) | Kind::Negate(n) => collect(n, paths, loads)?,
            Kind::Binary(op, a, b) if !matches!(op, Op::Chain) => {
                collect(a, paths, loads)?;
                collect(b, paths, loads)?;
            }
            Kind::Conditional(test, yes, no) => {
                collect(test, paths, loads)?;
                collect(yes, paths, loads)?;
                if let Some(no) = no {
                    collect(no, paths, loads)?;
                }
            }
            Kind::Builtin(Builtin::Lookup, args)
                if args.len() == 2 && immutable_lookup(&args[0]) =>
            {
                for arg in args {
                    collect(arg, paths, loads)?;
                }
            }
            Kind::Builtin(builtin, args)
                if (builtin.is_conversion() || *builtin == Builtin::Exists) && !args.is_empty() =>
            {
                for arg in args {
                    collect(arg, paths, loads)?;
                }
            }
            Kind::Array(items, _) => {
                for item in items {
                    collect(item, paths, loads)?;
                }
            }
            Kind::Object(pairs) => {
                for (key, value) in pairs {
                    if !matches!(&key.kind, Kind::String(_) | Kind::Prepared(_)) {
                        return None;
                    }
                    collect(value, paths, loads)?;
                }
            }
            Kind::StaticLookup(_, key) => collect(key, paths, loads)?,
            Kind::Number(_)
            | Kind::Boolean(_)
            | Kind::Null
            | Kind::String(_)
            | Kind::Missing
            | Kind::Prepared(_) => {}
            _ => return None,
        }
        Some(())
    }
    collect(node, &mut paths, &mut loads)?;
    (loads >= minimum_loads).then(|| paths.into_boxed_slice())
}

// Only lookup chains rooted in compiled immutable data qualify. Keys are checked
// separately by the ordinary demand analysis; no lookup is performed here.
fn immutable_lookup(node: &Node) -> bool {
    match &node.kind {
        Kind::StaticLookup(..) => true,
        Kind::Group(child) => immutable_lookup(child),
        Kind::Plan(plan) if plan.scalar() => immutable_lookup(&plan.source),
        Kind::Builtin(Builtin::Lookup, args) => {
            matches!(args.as_ref(), [object, _] if immutable_lookup(object))
        }
        _ => false,
    }
}

impl Region {
    pub(crate) fn prepare(node: &Node) -> Option<Box<Self>> {
        let mut region = Self {
            demand: Demand::default(),
            paths: Box::new([]),
        };
        // Plans already acquire their own inputs; simple paths/calls stay lightweight.
        if matches!(node.kind, Kind::Plan(_)) {
            return None;
        }
        let paths = paths(node, 3)?;
        for (slot, path) in paths.iter().enumerate() {
            region.demand.insert(&path.fields, slot);
        }
        region.paths = paths;
        Some(Box::new(region))
    }

    // This frame exists once per root evaluation, never per callback or tree node.
    #[inline(never)]
    pub(crate) fn evaluate<'e, 'i>(
        &'e self,
        root: &'e Node,
        input: &'i [u8],
    ) -> Result<Evaluation<'e, 'i>, Error> {
        let mut captures = Captures::default();
        let raw = crate::json::capture(input, &self.demand, &mut captures)?;
        self.captured(root, raw, &captures)
    }

    #[inline(never)]
    pub(crate) fn evaluate_validated<'e, 'i>(
        &'e self,
        root: &'e Node,
        input: crate::RawJson<'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        let mut captures = Captures::default();
        input.capture(&self.demand, &mut captures);
        self.captured(root, input, &captures)
    }

    #[inline]
    fn captured<'e, 'i>(
        &'e self,
        root: &'e Node,
        raw: crate::RawJson<'i>,
        captures: &Captures<'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        let context = Context {
            value: Value::Raw(raw),
            wrapped: true,
            scope: None,
        };
        // Arrays need context/grouping and sequence normalization. Retry before any
        // expression runs, so no effects, errors or partially built output are replayed.
        let result = if raw.as_bytes()[0] == b'['
            || (0..self.paths.len()).any(|slot| captures.get(slot) == Captured::Deferred)
        {
            root.run(&context)?
        } else {
            Acquired {
                paths: &self.paths,
                captures,
            }
            .run(root, &context)?
        };
        Ok(Evaluation {
            result: crate::evaluate::results(result),
        })
    }
}
pub(crate) fn evaluate_captured<'e, 'i>(
    root: &'e Node,
    raw: crate::RawJson<'i>,
    paths: &[Path],
    captures: &Captures<'i>,
) -> Result<Evaluation<'e, 'i>, Error> {
    let context = Context {
        value: Value::Raw(raw),
        wrapped: true,
        scope: None,
    };
    let result = Acquired { paths, captures }.run(root, &context)?;
    Ok(Evaluation {
        result: crate::evaluate::results(result),
    })
}

// Supported nodes keep the same focus on non-array roots; constructor grouping
// therefore receives one original context. Captures never enter returned values.
struct Acquired<'a, 'i> {
    paths: &'a [Path],
    captures: &'a Captures<'i>,
}
impl<'i> Acquired<'_, 'i> {
    fn run<'e>(&self, node: &'e Node, context: &Context<'e, 'i>) -> Result<Operand<'e, 'i>, Error> {
        match &node.kind {
            Kind::Path(path) => Ok(self.path(path)),
            Kind::Plan(plan) => plan
                .run_acquired(|p| Some(self.path(p)))
                .map_or_else(|| self.run(&plan.source, context), Ok),
            Kind::StaticLookup(data, key) => {
                crate::lookup::constant(data, self.materialize(key, context)?, node.offset)
                    .map(|v| v.map_or(Operand::Missing, Operand::One))
            }
            Kind::Array(items, preserve) => crate::construct::array_with(
                items,
                *preserve,
                |n| self.materialize(n, context),
                |n, emit| self.visit(n, context, emit),
            )
            .map(Operand::One),
            Kind::Object(pairs) => {
                crate::construct::object_with(pairs, context, node.offset, |n, c| {
                    self.materialize(n, c)
                })
                .map(Operand::One)
            }
            Kind::Builtin(builtin, args) => {
                let mut values = crate::function::Arguments::new(args.len());
                for arg in args {
                    values.push(self.materialize(arg, context)?);
                }
                builtin
                    .values(values.as_slice(), context, node.offset)
                    .map(Operand::normalize)
            }
            Kind::Group(child) => self.run(child, context),
            Kind::Negate(child) => pure::negate(self.run(child, context)?, node.offset),
            Kind::Conditional(test, yes, no) => {
                pure::conditional(test, yes, no.as_deref(), node.offset, |n| {
                    self.run(n, context)
                })
            }
            Kind::Binary(op @ (Op::Default | Op::Coalesce), test, no) => {
                pure::fallback(op, test, no, node.offset, |n| self.run(n, context))
            }
            Kind::Binary(Op::Concat, left, right) => {
                pure::concat(left, right, node.offset, |n| self.materialize(n, context))
                    .map(Operand::One)
            }
            Kind::Binary(op, left, right) => {
                crate::evaluate::operators::binary(op, left, right, node.offset, |n| {
                    self.run(n, context)
                })
            }
            _ => node.run(context),
        }
    }
    fn materialize<'e>(
        &self,
        node: &'e Node,
        context: &Context<'e, 'i>,
    ) -> Result<Option<Value<'e, 'i>>, Error> {
        match self.run(node, context)? {
            Operand::Missing => Ok(None),
            Operand::One(value) => Ok(Some(value)),
            Operand::Many(_) => unreachable!("root capture falls back before sequence traversal"),
        }
    }
    fn visit<'e>(
        &self,
        node: &'e Node,
        context: &Context<'e, 'i>,
        emit: &mut dyn FnMut(Value<'e, 'i>),
    ) -> Result<(), Error> {
        match self.run(node, context)?.walk(&mut |value| {
            emit(value);
            Ok(())
        }) {
            Ok(()) => Ok(()),
            Err(crate::sequence::Halt::Evaluation(e)) => Err(e),
            Err(crate::sequence::Halt::Stop) => unreachable!("construction consumes its members"),
        }
    }
    fn path<'e>(&self, path: &Path) -> Operand<'e, 'i> {
        let slot = self
            .paths
            .iter()
            .position(|p| p.fields == path.fields)
            .expect("compiled root demand");
        match self.captures.get(slot) {
            Captured::Missing => Operand::Missing,
            Captured::Raw(raw) => Operand::One(Value::Raw(raw)),
            Captured::Deferred => unreachable!("deferred regions use the original tree"),
        }
    }
}

#[cfg(test)]
mod tests;
