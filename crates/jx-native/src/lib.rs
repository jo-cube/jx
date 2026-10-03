#![deny(unsafe_code)]
//! Internal native backend for `jx`; enable through the engine's `jit` feature.

#[cfg(not(any(
    all(
        target_arch = "x86_64",
        any(target_os = "linux", target_os = "macos", target_os = "windows")
    ),
    all(target_arch = "aarch64", any(target_os = "linux", target_os = "macos")),
)))]
compile_error!(
    "jx native support requires x86_64 Linux/macOS/Windows or aarch64 Linux/macOS; build jx without the jit feature on other targets"
);
mod compile;
#[allow(unsafe_code)]
mod executable;

pub use executable::Kernel;
/// Exact scratch-buffer bound shared with the plan's native adapter.
pub const SLOTS: usize = 32;

#[derive(Clone, Copy, Debug)]
pub enum Binary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
#[derive(Clone, Copy, Debug)]
pub enum Operation {
    Input,
    Number(f64),
    Boolean(bool),
    Missing,
    Negate(u8),
    Binary(Binary, u8, u8),
    Truth(u8),
    Copy(u8),
    Branch(u8, bool, u8),
    Jump(u8),
    Merge(u8, u8),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guards_and_shared_code_lifetime() {
        fn shareable<T: Send + Sync>() {}
        shareable::<Kernel>();
        let kernel = Kernel::compile(
            &[
                Operation::Input,
                Operation::Number(2.0),
                Operation::Binary(Binary::Multiply, 0, 1),
            ],
            2,
        )
        .unwrap();
        let mut numbers = [0.0; SLOTS];
        let mut tags = [0; SLOTS];
        numbers[0] = 3.0;
        tags[0] = 1;
        assert_eq!(kernel.run(&numbers, &tags), Some((6.0, 1)));
        tags[0] = 2;
        assert_eq!(kernel.run(&numbers, &tags), None);
        tags[0] = 1;
        numbers[0] = f64::INFINITY;
        assert_eq!(kernel.run(&numbers, &tags), None);
        tags[0] = 0;
        assert_eq!(kernel.run(&numbers, &tags).unwrap().1, 0);
        let copy = kernel.clone();
        drop(kernel);
        numbers[0] = 4.0;
        tags[0] = 1;
        std::thread::spawn(move || assert_eq!(copy.run(&numbers, &tags), Some((8.0, 1))))
            .join()
            .unwrap();
    }
    #[test]
    fn invalid_programs_fail_without_invocation() {
        assert!(Kernel::compile(&[], 0).is_err());
        assert!(Kernel::compile(&[Operation::Copy(0)], 0).is_err());
        assert!(Kernel::compile(&[Operation::Jump(0)], 0).is_err());
        assert!(Kernel::compile(&[Operation::Number(1.0)], 1).is_err());
        assert!(Kernel::compile(&[Operation::Missing; SLOTS + 1], 0).is_err());
    }
    #[test]
    fn invalid_operands_and_forward_branches_keep_the_executable_boundary_bounded() {
        for operand in [0, 1, 31, 32, 255] {
            for operation in [
                Operation::Negate(operand),
                Operation::Copy(operand),
                Operation::Truth(operand),
                Operation::Binary(Binary::Add, operand, 0),
                Operation::Merge(operand, 0),
            ] {
                let result = Kernel::compile(&[Operation::Input, operation], 1);
                assert_eq!(result.is_ok(), operand == 0, "{operation:?}");
            }
        }
        for target in [0, 1, 2, 3, 31, 32, 255] {
            let ops = [
                Operation::Input,
                Operation::Branch(0, true, target),
                Operation::Number(7.0),
            ];
            let result = Kernel::compile(&ops, 2);
            assert_eq!(result.is_ok(), matches!(target, 2 | 3));
            if let Ok(kernel) = result {
                for tag in [0, 1, 2, 3, 255] {
                    let mut numbers = [0.0; SLOTS];
                    let mut tags = [0; SLOTS];
                    numbers[0] = 1.0;
                    tags[0] = tag;
                    let _ = kernel.run(&numbers, &tags);
                }
            }
        }
    }
}
