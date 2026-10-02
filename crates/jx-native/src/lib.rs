#![deny(unsafe_code)]
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
}
