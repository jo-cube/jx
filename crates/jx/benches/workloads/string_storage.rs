// Construction-only controls, not JSONata evaluation. The inline type stays in
// the benchmark; production retains shared string identity and its smaller Value.
use super::measure_allocations;
use std::{hint::black_box, rc::Rc};

#[derive(Clone)]
enum Compact {
    Inline([u8; 22], u8),
    Shared(Rc<str>),
}
impl Compact {
    fn new(left: &str, right: &str) -> Self {
        let len = left.len() + right.len() + 2;
        if len > 22 {
            return Self::Shared(shared(left, right));
        }
        let mut bytes = [0; 22];
        encode(&mut bytes[..len], left, right);
        Self::Inline(bytes, len as u8)
    }
    fn text(&self) -> &str {
        match self {
            Self::Inline(bytes, len) => std::str::from_utf8(&bytes[..usize::from(*len)]).unwrap(),
            Self::Shared(text) => text,
        }
    }
}
fn encode(bytes: &mut [u8], left: &str, right: &str) {
    bytes[0] = b'"';
    bytes[1..1 + left.len()].copy_from_slice(left.as_bytes());
    bytes[1 + left.len()..1 + left.len() + right.len()].copy_from_slice(right.as_bytes());
    let last = bytes.len() - 1;
    bytes[last] = b'"';
}
fn shared(left: &str, right: &str) -> Rc<str> {
    let mut text = String::with_capacity(left.len() + right.len() + 2);
    text.push('"');
    text.push_str(left);
    text.push_str(right);
    text.push('"');
    text.into()
}
fn stack(left: &str, right: &str) -> Rc<str> {
    let len = left.len() + right.len() + 2;
    if len > 64 {
        return shared(left, right);
    }
    let mut bytes = [0; 64];
    encode(&mut bytes[..len], left, right);
    std::str::from_utf8(&bytes[..len]).unwrap().into()
}
pub(super) fn run(smoke: bool) {
    eprintln!(
        "string storage control: shared {} B, inline {} B, current Value {} B",
        std::mem::size_of::<Rc<str>>(),
        std::mem::size_of::<Compact>(),
        std::mem::size_of::<jx::Value>()
    );
    for len in [12, 22, 62, 63, 128] {
        let left = "x".repeat(len - 2);
        let right = "yz";
        let expected = format!("\"{left}{right}\"");
        assert_eq!(Compact::new(&left, right).text(), expected);
        for (label, constructor, limit) in [
            ("shared", shared as fn(&str, &str) -> Rc<str>, 2),
            (
                "stack",
                stack as fn(&str, &str) -> Rc<str>,
                if len + 2 <= 64 { 1 } else { 2 },
            ),
        ] {
            measure_allocations(
                &format!("rust/string_{label}"),
                len,
                smoke,
                Some(limit),
                || {
                    let text = constructor(black_box(&left), black_box(right));
                    black_box((text.clone(), text.clone(), text.clone(), text));
                },
            );
        }
        measure_allocations(
            "rust/string_inline",
            len,
            smoke,
            Some(if len + 2 <= 22 { 0 } else { 2 }),
            || {
                let text = Compact::new(black_box(&left), black_box(right));
                black_box((text.clone(), text.clone(), text.clone(), text));
            },
        );
    }
}
