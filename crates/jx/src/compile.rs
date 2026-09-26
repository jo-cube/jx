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
    if let Kind::Binary(Op::Coalesce, test, no) = &mut node.kind {
        // Its call argument is also the selected branch; keep that call boundary.
        crate::analysis::children(test, &mut |child| {
            constant &= prepare(child);
        });
        constant &= eligible(test);
        constant &= prepare(no);
    } else {
        crate::analysis::children(node, &mut |child| {
            constant &= prepare(child);
        });
    }
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
        if let Ok(value) = crate::retain::materialize(node, &context)
            && let Some(data) = Data::capture(&value.unwrap_or(Value::Undefined))
        {
            let prepared = Prepared {
                data,
                array_syntax: node.is_array_constructor(),
            };
            node.kind = Kind::Prepared(Box::new(prepared));
            node.effects = false;
        } else {
            constant = false;
        }
    }
    if let Kind::Builtin(Builtin::Lookup, args) = &mut node.kind
        && args.len() == 2
        && matches!(&args[0].kind,Kind::Prepared(p) if matches!(p.data,Data::Object {..}))
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
    if node.effects {
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
        Kind::Builtin(builtin, args) => match builtin {
            Builtin::Deferred(_) => false,
            Builtin::Lookup => args.len() == 2,
            _ => !args.is_empty(),
        },
        _ => false,
    }
}
