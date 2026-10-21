use crate::{
    Value,
    builtin::Builtin,
    constant::{Data, Prepared},
    expression::{Kind, Node, Op},
    sequence::Context,
};

// Only context-independent, effect-free expressions are candidates. Evaluate
// with the existing semantics; failures stay in the tree as runtime failures.
pub(crate) fn prepare(node: &mut Node) -> bool {
    let mut constant = true;
    if let Kind::Keep(child, _) = &mut node.kind
        && matches!(
            child.kind,
            Kind::Builtin(..) | Kind::Call(..) | Kind::Binary(Op::Chain, ..)
        )
    {
        // Keep the call boundary: folding would normalize a singleton sequence
        // before [] can retain it. Its arguments may still be compiled constants.
        crate::analysis::children(child, &mut |arg| {
            prepare(arg);
        });
        specialize(child);
        constant = false;
    } else if let Kind::Binary(Op::Coalesce, test, no) = &mut node.kind {
        // Its call argument is also the selected branch; keep that call boundary.
        crate::analysis::children(test, &mut |child| {
            constant &= prepare(child);
        });
        specialize(test);
        constant &= eligible(test);
        constant &= prepare(no);
    } else {
        crate::analysis::children(node, &mut |child| {
            constant &= prepare(child);
        });
    }
    specialize(node);
    constant |= constant_route(node);
    constant &= eligible(node);
    if constant
        && !matches!(
            node.kind,
            Kind::Number(_)
                | Kind::String(_)
                | Kind::Boolean(_)
                | Kind::Null
                | Kind::Missing
                | Kind::Prepared(_)
                | Kind::BuiltinReference(_)
        )
    {
        let context = Context {
            value: Value::Undefined,
            wrapped: true,
            scope: None,
        };
        if let Some(data) = shared_object_route(node) {
            node.kind = Kind::Prepared(Box::new(Prepared {
                data,
                array_syntax: false,
            }));
            node.effects = false;
            node.clock = false;
        } else if let Ok(value) = crate::retain::materialize(node, &context)
            && let Some(data) = Data::capture(&value.unwrap_or(Value::Undefined))
        {
            let prepared = Prepared {
                data: data.into(),
                array_syntax: node.is_array_constructor(),
            };
            node.kind = Kind::Prepared(Box::new(prepared));
            node.effects = false;
            node.clock = false;
        } else {
            constant = false;
        }
    }
    if let Kind::Builtin(Builtin::Lookup, args) = &mut node.kind
        && args.len() == 2
        && matches!(&args[0].kind,Kind::Prepared(p) if matches!(*p.data,Data::Object {..}))
    {
        let mut args = std::mem::take(args).into_vec();
        let key = args.pop().unwrap();
        let Kind::Prepared(prepared) = args.pop().unwrap().kind else {
            unreachable!()
        };
        node.kind = Kind::StaticLookup(Box::new(prepared.data), Box::new(key));
    }
    constant
}
fn eligible(node: &Node) -> bool {
    if node.effects || node.clock || node.tail_call {
        return false;
    }
    match &node.kind {
        Kind::Number(_)
        | Kind::String(_)
        | Kind::Boolean(_)
        | Kind::Null
        | Kind::Missing
        | Kind::Prepared(_)
        | Kind::BuiltinReference(_)
        | Kind::Array(..)
        | Kind::Object(_)
        | Kind::Group(_)
        | Kind::Negate(_)
        | Kind::Binary(..)
        | Kind::Conditional(..) => true,
        Kind::Route(..) => constant_route(node),
        Kind::Builtin(builtin, args) => builtin.constant(args),
        Kind::Formatted(call) => call.constant(),
        _ => false,
    }
}

// A bound variable is an absolute path head even in unwrapped array contexts.
// Fold only its relative static navigation, using ordinary sequence semantics.
fn constant_route(node: &Node) -> bool {
    let Kind::Route(steps, _) = &node.kind else {
        return false;
    };
    steps
        .first()
        .is_some_and(|head| matches!(&head.node.kind, Kind::Prepared(p) if p.data.is_shared()))
        && steps
            .iter()
            .all(|step| step.bindings.is_none() && step.predicates.is_empty())
        && steps[1..]
            .iter()
            .all(|step| matches!(&step.node.kind, Kind::Path(path) if !path.rooted))
}

fn shared_object_route(node: &Node) -> Option<crate::constant::Storage> {
    if !constant_route(node) {
        return None;
    }
    let Kind::Route(steps, _) = &node.kind else {
        return None;
    };
    let Kind::Prepared(head) = &steps[0].node.kind else {
        return None;
    };
    let fields = steps[1..].iter().flat_map(|step| {
        let Kind::Path(path) = &step.node.kind else {
            unreachable!("static route")
        };
        path.fields.iter().map(AsRef::as_ref)
    });
    head.data.select_object(fields)
}

fn specialize(node: &mut Node) {
    let eval = match &node.kind {
        Kind::Builtin(Builtin::Runtime(crate::dynamic::Builtin::Eval), _) => true,
        Kind::Call(target, _) => {
            matches!(&target.kind, Kind::Variable(name) if name.as_ref() == "eval")
        }
        _ => false,
    };
    if eval {
        let args = match &mut node.kind {
            Kind::Builtin(_, args) | Kind::Call(_, args) => std::mem::take(args),
            _ => unreachable!(),
        };
        node.kind = Kind::Eval(Box::new(crate::dynamic::Call::prepare(args)));
    }
    if let Kind::Builtin(Builtin::Library(function), args) = &mut node.kind
        && let Some(call) = crate::format::Call::prepare(*function, args, node.offset)
    {
        node.kind = Kind::Formatted(Box::new(call));
    }
    let mut clock = crate::analysis::own_clock(&node.kind);
    crate::analysis::children(node, &mut |child| clock |= child.clock);
    node.clock = clock;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_immutable_object_storage_is_shared_across_expression_clones() {
        let config = std::sync::Arc::new(
            crate::OwnedValue::from_json(br#"{"labels":{"x":"value"}}"#).unwrap(),
        );
        let options = crate::CompileOptions::default().constant_binding("config", config.clone());
        let expression = options.compile("$lookup($config.labels,key)").unwrap();
        let expected = config
            .data
            .find(
                || crate::json::string::fingerprint("labels"),
                |key| crate::json::string::matches(key, "labels"),
            )
            .unwrap();
        for expression in [expression.clone(), expression] {
            let Kind::StaticLookup(data, _) = &expression.root.kind else {
                panic!("static lookup")
            };
            let actual: &Data = data;
            assert!(
                std::ptr::eq(actual, expected),
                "nested immutable table was copied"
            );
            assert_eq!(
                expression
                    .evaluate(br#"{"key":"x"}"#)
                    .unwrap()
                    .single()
                    .unwrap()
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .unwrap(),
                "value"
            );
        }
    }
}
