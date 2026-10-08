use crate::{
    Error, Evaluation, Random, Value,
    expression::{Kind, Node},
    json::{Captured, Captures, Demand},
    sequence::Context,
};

pub(crate) fn prepare(node: &Node) -> Option<Demand> {
    let Kind::Call(target, arguments) = &node.kind else {
        return None;
    };
    if !matches!(target.kind, Kind::Lambda(_)) || arguments.len() > crate::json::CAPTURE_SLOTS {
        return None;
    }
    let mut demand = Demand::default();
    let mut fields = false;
    for (slot, argument) in arguments.iter().enumerate() {
        match &argument.kind {
            Kind::Path(path) => {
                fields |= !path.fields.is_empty();
                demand.insert(&path.fields, slot);
            }
            Kind::Number(_)
            | Kind::Boolean(_)
            | Kind::Null
            | Kind::String(_)
            | Kind::Missing
            | Kind::Prepared(_) => {}
            _ => return None,
        }
    }
    fields.then_some(demand)
}

// Only acquisition is specialized. Signatures, budgets, body execution, effects
// and call-result normalization still pass through the ordinary invocation path.
pub(crate) fn evaluate<'e, 'i>(
    node: &'e Node,
    demand: &Demand,
    input: &'i [u8],
    random: Option<&Random>,
) -> Result<Evaluation<'e, 'i>, Error> {
    let mut captured = Captures::default();
    let raw = crate::json::capture(input, demand, &mut captured)?;
    evaluate_captured(node, raw, &captured, random)
}

pub(crate) fn evaluate_validated<'e, 'i>(
    node: &'e Node,
    demand: &Demand,
    input: crate::RawJson<'i>,
    random: Option<&Random>,
) -> Result<Evaluation<'e, 'i>, Error> {
    let mut captured = Captures::default();
    input.capture(demand, &mut captured);
    evaluate_captured(node, input, &captured, random)
}

#[inline]
fn evaluate_captured<'e, 'i>(
    node: &'e Node,
    raw: crate::RawJson<'i>,
    captured: &Captures<'i>,
    random: Option<&Random>,
) -> Result<Evaluation<'e, 'i>, Error> {
    let Kind::Call(target, arguments) = &node.kind else {
        unreachable!()
    };
    let Kind::Lambda(definition) = &target.kind else {
        unreachable!()
    };
    let value = Value::Raw(raw);
    let context = Context {
        scope: Some(crate::runtime::Scope::with_random(
            value.clone(),
            node.clock,
            random,
        )),
        value,
        wrapped: true,
    };
    let mut values = super::Arguments::new(arguments.len());
    for (slot, argument) in arguments.iter().enumerate() {
        let value = if matches!(argument.kind, Kind::Path(_)) {
            match captured.get(slot) {
                Captured::Raw(raw) => Some(Value::Raw(raw)),
                Captured::Missing => None,
                // Intermediate arrays need the original path/sequence rules.
                Captured::Deferred => crate::retain::materialize(argument, &context)?,
            }
        } else {
            crate::retain::materialize(argument, &context)?
        };
        values.push(value);
    }
    let result = super::invoke_literal(definition, values.as_slice(), &context, node.offset)?;
    Ok(Evaluation {
        result: crate::evaluate::results(if node.tail_call {
            result
        } else {
            result.normalize()
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(result: Result<Evaluation<'_, '_>, Error>) -> Result<Vec<Vec<u8>>, Error> {
        let mut items = Vec::new();
        result?.for_each(|v| {
            let mut bytes = Vec::new();
            match v {
                Value::Number(n) => bytes.extend(n.to_bits().to_le_bytes()),
                v => v.write_compact(&mut bytes).unwrap(),
            }
            items.push(bytes);
        })?;
        Ok(items)
    }
    #[test]
    fn captured_calls_match_original_calls_and_exact_errors() {
        let atoms = [
            "null", "true", "false", "0", "-0", "2", "-3.5", "1e999", "1e-320", "[]", "[1]",
            "[1,2]", "{}", "\"x\"",
        ];
        for source in [
            "function($a,$b){$a+$b}(a,b)",
            "function($a,$b)<nn:n>{$a+$b}(a,b)",
            "function($a,$b){[$a,$b]}(a,b)",
            "function($a,$b){$a?$a:$b}(a.x,b.x)",
            "function($a,$b){[$a,$b]}(a,a)",
            "function($a,$b){$b}(a,1)",
        ] {
            let expr = crate::compile(source).unwrap();
            assert!(expr.acquisition.is_some(), "{source}");
            for a in atoms {
                for b in atoms {
                    let object = format!(r#"{{"a":{a},"b":{b}}}"#);
                    for input in [
                        object.clone(),
                        format!("[{object}]"),
                        format!("[{object},{object}]"),
                        format!(r#"{{"a":{a},"b":{b},"a":null}}"#),
                        format!(r#"{{"b":{b}}}"#),
                    ] {
                        assert_eq!(
                            outcome(expr.evaluate(input.as_bytes())),
                            outcome(crate::evaluate::scalar(
                                &expr.root,
                                input.as_bytes(),
                                None,
                                true
                            )),
                            "{source}: {input}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn effects_in_arguments_and_dynamic_targets_stay_on_the_tree() {
        for source in [
            "function($a){$a}(a+1)",
            "function($a){$a}($)",
            "function($a,$b){$a}(a,$random())",
            "function($a,$b){$a}(a,$error('stop'))",
            "($f:=function($x){$x};$f(a))",
        ] {
            assert!(
                crate::compile(source).unwrap().acquisition.is_none(),
                "{source}"
            );
        }
        let expr = crate::compile("function($a){$random()+$random()+$a}(a)").unwrap();
        assert!(expr.acquisition.is_some());
        let first = crate::Random::seeded(9);
        let second = crate::Random::seeded(9);
        for _ in 0..8 {
            assert_eq!(
                outcome(expr.evaluate_with_random(br#"{"a":2}"#, &first)),
                outcome(crate::evaluate::scalar(
                    &expr.root,
                    br#"{"a":2}"#,
                    Some(&second),
                    true
                ))
            );
        }
    }
}
