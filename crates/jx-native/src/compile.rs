use crate::{Binary, Operation, SLOTS};
use cranelift_codegen::ir::{
    AbiParam, InstBuilder, MemFlagsData, Value, condcodes::FloatCC, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::JITModule;
use cranelift_module::{FuncId, Module};

pub(crate) fn compile(
    module: &mut JITModule,
    ops: &[Operation],
    result: u8,
) -> Result<(FuncId, usize), String> {
    if ops.is_empty() || ops.len() > SLOTS || usize::from(result) >= ops.len() {
        return Err("invalid register bound".into());
    }
    for (at, op) in ops.iter().enumerate() {
        let before = |r: u8| usize::from(r) < at;
        let forward = |r: u8| usize::from(r) > at && usize::from(r) <= ops.len();
        let valid = match *op {
            Operation::Negate(a) | Operation::Truth(a) | Operation::Copy(a) => before(a),
            Operation::Binary(_, a, b) | Operation::Merge(a, b) => before(a) && before(b),
            Operation::Branch(a, _, target) => before(a) && forward(target),
            Operation::Jump(target) => forward(target),
            _ => true,
        };
        if !valid {
            return Err("invalid operand or branch".into());
        }
    }
    let mut context = module.make_context();
    let pointer = module.target_config().pointer_type();
    for _ in 0..4 {
        context.func.signature.params.push(AbiParam::new(pointer));
    }
    context
        .func
        .signature
        .returns
        .push(AbiParam::new(types::I8));
    let id = module
        .declare_anonymous_function(&context.func.signature)
        .map_err(|e| e.to_string())?;
    let mut scratch = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut context.func, &mut scratch);
    let entry = b.create_block();
    let blocks = (0..=ops.len())
        .map(|_| b.create_block())
        .collect::<Vec<_>>();
    let failed = b.create_block();
    b.append_block_params_for_function_params(entry);
    b.switch_to_block(entry);
    let args = b.block_params(entry).to_vec();
    let numbers = (0..ops.len())
        .map(|_| b.declare_var(types::F64))
        .collect::<Vec<_>>();
    let tags = (0..ops.len())
        .map(|_| b.declare_var(types::I8))
        .collect::<Vec<_>>();
    let zero = b.ins().f64const(0.0);
    let missing = b.ins().iconst(types::I8, 0);
    for (&n, &t) in numbers.iter().zip(&tags) {
        b.def_var(n, zero);
        b.def_var(t, missing);
    }
    b.ins().jump(blocks[0], &[]);
    for (at, op) in ops.iter().enumerate() {
        b.switch_to_block(blocks[at]);
        let mut pair = None;
        match *op {
            Operation::Input => {
                let n = b
                    .ins()
                    .load(types::F64, MemFlagsData::new(), args[0], (at * 8) as i32);
                let t = b
                    .ins()
                    .load(types::I8, MemFlagsData::new(), args[1], at as i32);
                pair = Some((n, t));
            }
            Operation::Number(n) => {
                pair = Some((b.ins().f64const(n), b.ins().iconst(types::I8, 1)))
            }
            Operation::Boolean(n) => {
                pair = Some((
                    b.ins().f64const(if n { 1.0 } else { 0.0 }),
                    b.ins().iconst(types::I8, 2),
                ))
            }
            Operation::Missing => pair = Some((zero, missing)),
            Operation::Copy(a) => pair = Some(read(&mut b, &numbers, &tags, a)),
            Operation::Merge(a, target) => {
                let (n, t) = read(&mut b, &numbers, &tags, a);
                b.def_var(numbers[usize::from(target)], n);
                b.def_var(tags[usize::from(target)], t);
            }
            Operation::Negate(a) => {
                let (n, t) = read(&mut b, &numbers, &tags, a);
                guard_number(&mut b, n, t, failed);
                let n = b.ins().fneg(n);
                pair = Some((n, t));
            }
            Operation::Binary(op, a, c) => {
                let (a, ta) = read(&mut b, &numbers, &tags, a);
                let (c, tc) = read(&mut b, &numbers, &tags, c);
                guard_number(&mut b, a, ta, failed);
                guard_number(&mut b, c, tc, failed);
                let present = b.ins().band(ta, tc);
                let n = match op {
                    Binary::Add => b.ins().fadd(a, c),
                    Binary::Subtract => b.ins().fsub(a, c),
                    Binary::Multiply => b.ins().fmul(a, c),
                    Binary::Divide => b.ins().fdiv(a, c),
                    op => {
                        let cc = match op {
                            Binary::Equal => FloatCC::Equal,
                            Binary::NotEqual => FloatCC::NotEqual,
                            Binary::Less => FloatCC::LessThan,
                            Binary::LessEqual => FloatCC::LessThanOrEqual,
                            Binary::Greater => FloatCC::GreaterThan,
                            Binary::GreaterEqual => FloatCC::GreaterThanOrEqual,
                            _ => unreachable!(),
                        };
                        let test = b.ins().fcmp(cc, a, c);
                        let test = b.ins().band(test, present);
                        let one = b.ins().f64const(1.0);
                        b.ins().select(test, one, zero)
                    }
                };
                let t = if matches!(
                    op,
                    Binary::Add | Binary::Subtract | Binary::Multiply | Binary::Divide
                ) {
                    present
                } else if matches!(op, Binary::Equal | Binary::NotEqual) {
                    b.ins().iconst(types::I8, 2)
                } else {
                    let boolean = b.ins().iconst(types::I8, 2);
                    b.ins().select(present, boolean, missing)
                };
                pair = Some((n, t));
            }
            Operation::Truth(a) => {
                let (n, t) = read(&mut b, &numbers, &tags, a);
                let test = truth(&mut b, n, t, failed);
                let one = b.ins().f64const(1.0);
                let n = b.ins().select(test, one, zero);
                pair = Some((n, b.ins().iconst(types::I8, 2)));
            }
            Operation::Branch(a, wanted, target) => {
                let (n, t) = read(&mut b, &numbers, &tags, a);
                let mut test = truth(&mut b, n, t, failed);
                if !wanted {
                    test = b.ins().bxor_imm_u(test, 1)
                }
                b.ins()
                    .brif(test, blocks[usize::from(target)], &[], blocks[at + 1], &[]);
                continue;
            }
            Operation::Jump(target) => {
                b.ins().jump(blocks[usize::from(target)], &[]);
                continue;
            }
        }
        if let Some((n, t)) = pair {
            b.def_var(numbers[at], n);
            b.def_var(tags[at], t);
        }
        b.ins().jump(blocks[at + 1], &[]);
    }
    b.switch_to_block(blocks[ops.len()]);
    let (n, t) = read(&mut b, &numbers, &tags, result);
    b.ins().store(MemFlagsData::new(), n, args[2], 0);
    b.ins().store(MemFlagsData::new(), t, args[3], 0);
    let one = b.ins().iconst(types::I8, 1);
    b.ins().return_(&[one]);
    b.switch_to_block(failed);
    b.ins().return_(&[missing]);
    b.seal_all_blocks();
    b.finalize(module.target_config());
    module
        .define_function(id, &mut context)
        .map_err(|e| e.to_string())?;
    let size = context.compiled_code().unwrap().code_buffer().len();
    module.finalize_definitions().map_err(|e| e.to_string())?;
    Ok((id, size))
}
fn read(
    b: &mut FunctionBuilder<'_>,
    numbers: &[Variable],
    tags: &[Variable],
    a: u8,
) -> (Value, Value) {
    (
        b.use_var(numbers[usize::from(a)]),
        b.use_var(tags[usize::from(a)]),
    )
}
fn guard_number(
    b: &mut FunctionBuilder<'_>,
    n: Value,
    t: Value,
    failed: cranelift_codegen::ir::Block,
) {
    let numeric = b.ins().icmp_imm_u(
        cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThanOrEqual,
        t,
        1,
    );
    let abs = b.ins().fabs(n);
    let inf = b.ins().f64const(f64::INFINITY);
    let finite = b.ins().fcmp(FloatCC::LessThan, abs, inf);
    let missing = b
        .ins()
        .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::Equal, t, 0);
    let finite = b.ins().bor(finite, missing);
    let ok = b.ins().band(numeric, finite);
    let next = b.create_block();
    b.ins().brif(ok, next, &[], failed, &[]);
    b.switch_to_block(next);
}
fn truth(
    b: &mut FunctionBuilder<'_>,
    n: Value,
    t: Value,
    failed: cranelift_codegen::ir::Block,
) -> Value {
    let valid = b.ins().icmp_imm_u(
        cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThanOrEqual,
        t,
        2,
    );
    let abs = b.ins().fabs(n);
    let inf = b.ins().f64const(f64::INFINITY);
    let infinite = b.ins().fcmp(FloatCC::Equal, abs, inf);
    let finite = b.ins().bxor_imm_u(infinite, 1);
    let ok = b.ins().band(valid, finite);
    let next = b.create_block();
    b.ins().brif(ok, next, &[], failed, &[]);
    b.switch_to_block(next);
    let zero = b.ins().f64const(0.0);
    let nonzero = b.ins().fcmp(FloatCC::OrderedNotEqual, n, zero);
    let present = b
        .ins()
        .icmp_imm_u(cranelift_codegen::ir::condcodes::IntCC::NotEqual, t, 0);
    b.ins().band(nonzero, present)
}
