//! Conservative outcome kinds over semantic instructions. A path predicate may
//! use truth filtering only when its result cannot denote numeric positions.
//! Unknown fields/captures and arrays remain unproved, including singleton arrays.
use super::*;
use crate::builtin::library::Library;
const MISSING: u16 = 1;
const NUMBER: u16 = 2;
const BOOLEAN: u16 = 4;
const STRING: u16 = 8;
const NULL: u16 = 16;
const OBJECT: u16 = 32;
const ARRAY: u16 = 64;
const UNKNOWN: u16 = u16::MAX;
pub(super) fn truth_predicate(p: &Program) -> bool {
    let kinds = infer(&p.code, p.registers);
    let result = kinds[p.result];
    result != 0 && result & !(MISSING | BOOLEAN | STRING | NULL | OBJECT) == 0
}
pub(super) fn infer(code: &[Instruction], registers: usize) -> Vec<u16> {
    let mut kinds = vec![0; registers];
    for i in code {
        let kind = match &i.kind {
            Operation::Literal(l) => match l {
                Literal::Missing => MISSING,
                Literal::Null => NULL,
                Literal::Boolean(_) => BOOLEAN,
                Literal::Number(_) => NUMBER,
                Literal::String(_) => STRING,
                Literal::Data(d) => match &***d {
                    Data::Missing => MISSING,
                    Data::Null => NULL,
                    Data::Boolean(_) => BOOLEAN,
                    Data::Number(_) => NUMBER,
                    Data::String(_) => STRING,
                    Data::Object { .. } => OBJECT,
                    Data::Array(..) => ARRAY,
                },
            },
            Operation::Copy(r) | Operation::Materialize(r) | Operation::Bind(_, r) => kinds[*r],
            Operation::Outer(_, kind) => *kind,
            Operation::Negate(_) | Operation::Fold { .. } | Operation::Reduce(..) => {
                NUMBER | MISSING
            }
            Operation::Truth(_) => BOOLEAN,
            Operation::Binary(op, ..) => match op {
                Op::Equal | Op::NotEqual | Op::In => BOOLEAN,
                Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => BOOLEAN | MISSING,
                Op::Concat => STRING,
                Op::Add | Op::Subtract | Op::Multiply | Op::Divide | Op::Remainder => {
                    NUMBER | MISSING
                }
                _ => UNKNOWN,
            },
            Operation::Builtin(b, ..) => match b {
                Builtin::Boolean | Builtin::Not | Builtin::Exists => BOOLEAN,
                Builtin::Aggregate(_) => NUMBER | MISSING,
                Builtin::Library(l) => match l {
                    Library::Contains => BOOLEAN | MISSING,
                    Library::String
                    | Library::Uppercase
                    | Library::Lowercase
                    | Library::Trim
                    | Library::Substring
                    | Library::Before
                    | Library::After
                    | Library::Type
                    | Library::Join => STRING | MISSING,
                    Library::Number
                    | Library::Length
                    | Library::Abs
                    | Library::Floor
                    | Library::Ceil
                    | Library::Sqrt
                    | Library::Power => NUMBER | MISSING,
                    _ => UNKNOWN,
                },
                _ => UNKNOWN,
            },
            Operation::Object(_) | Operation::Construct(_) | Operation::Group(..) => OBJECT,
            Operation::Array(..) => ARRAY,
            Operation::Jump(_) | Operation::JumpFalse(..) => MISSING,
            _ => UNKNOWN,
        };
        // Conditional copies join here. An unknown arm cannot become a proof.
        kinds[i.out] |= kind;
    }
    kinds
}

pub(super) fn non_array_result(p: &Program) -> bool {
    let kind = infer(&p.code, p.registers)[p.result];
    kind != 0 && kind & ARRAY == 0
}
