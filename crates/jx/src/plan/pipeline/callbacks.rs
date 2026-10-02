use super::*;

// This proof is narrower than global replay analysis: only literal callbacks without signatures
// and using their item parameter can disappear into the numeric loop.
pub(in crate::plan) fn lower(node: &Node) -> Option<Pipeline> {
    use crate::builtin::library::Library;
    let Kind::Builtin(Builtin::Aggregate(aggregate), args) = &node.kind else {
        return None;
    };
    let [argument] = args.as_ref() else {
        return None;
    };
    let Kind::Builtin(Builtin::Library(Library::Map), args) = &argument.kind else {
        return None;
    };
    let [source, mapped] = args.as_ref() else {
        return None;
    };
    fn lambda(node: &Node) -> Option<&crate::function::Definition> {
        match &node.kind {
            Kind::Lambda(d)
                if d.params.len() == 1
                    && !d.params[0].is_empty()
                    && d.signature.is_none()
                    && !d.tail
                    && !d.body.clock =>
            {
                Some(d.as_ref())
            }
            _ => None,
        }
    }
    let mapped = lambda(mapped)?;
    let (source, predicate) =
        if let Kind::Builtin(Builtin::Library(Library::Filter), args) = &source.kind {
            let [source, predicate] = args.as_ref() else {
                return None;
            };
            let predicate = lambda(predicate)?;
            if !boolean(&predicate.body) {
                return None;
            };
            (source, Some(predicate))
        } else {
            (source, None)
        };
    let Kind::Path(path) = &source.kind else {
        return None;
    };
    if path.rooted || path.fields.is_empty() {
        return None;
    };
    let mut lower = Lower::parameters(&mapped.params);
    let mut branches = Vec::new();
    if let Some(predicate) = predicate {
        lower.set_parameters(&predicate.params);
        let test = lower.node(&predicate.body)?;
        branches.push(lower.emit(Instruction::Branch(test, false, 0))?);
        lower.set_parameters(&mapped.params);
    }
    let value = lower.node(&mapped.body)?;
    let result = masked(&mut lower, value, branches)?;
    let mut program = lower.finish(result);
    if program
        .argument_demands
        .iter()
        .any(|(source, _)| *source != Some(0))
    {
        return None;
    };
    program.capture = program
        .argument_demands
        .into_vec()
        .pop()
        .map_or_else(Demand::default, |(_, d)| d);
    program.argument_demands = Box::new([]);
    program.inputs = Box::new([]);
    let mut demand = Demand::default();
    demand.insert(&path.fields, 0);
    Some(Pipeline {
        source: path.fields.clone(),
        demand,
        program,
        aggregate: *aggregate,
        offset: node.offset,
    })
}
