fn main() {
    println!(
        "Value: size={}, needs_drop={}",
        std::mem::size_of::<Value<'static, 'static>>(),
        std::mem::needs_drop::<Value<'static, 'static>>()
    );
    println!(
        "PathEvaluation: size={}, needs_drop={}",
        std::mem::size_of::<path::PathEvaluation<'static, 'static>>(),
        std::mem::needs_drop::<path::PathEvaluation<'static, 'static>>()
    );
    println!(
        "Stream: size={}, needs_drop={}",
        std::mem::size_of::<sequence::Stream<'static, 'static>>(),
        std::mem::needs_drop::<sequence::Stream<'static, 'static>>()
    );
    println!(
        "Operand: size={}, needs_drop={}",
        std::mem::size_of::<evaluate::Operand<'static, 'static>>(),
        std::mem::needs_drop::<evaluate::Operand<'static, 'static>>()
    );
    println!(
        "Context: size={}, needs_drop={}",
        std::mem::size_of::<sequence::Context<'static, 'static>>(),
        std::mem::needs_drop::<sequence::Context<'static, 'static>>()
    );
    println!(
        "Node: size={}, needs_drop={}",
        std::mem::size_of::<expression::Node>(),
        std::mem::needs_drop::<expression::Node>()
    );
}
