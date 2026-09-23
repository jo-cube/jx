#[derive(Clone, Debug)]
pub(crate) struct Path {
    pub fields: Box<[Box<str>]>,
    pub rooted: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub kind: Kind,
    pub offset: usize,
    pub depth: usize,
}

#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Path(Path),
    Number(f64),
    Boolean(bool),
    Null,
    String(Box<str>), // Valid JSON encoding, including escaped surrogate units.
    Missing,
    Negate(Box<Node>),
    Binary(Op, Box<Node>, Box<Node>),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Op {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

impl Op {
    pub fn precedence(self) -> u8 {
        match self {
            Self::Or => 25,
            Self::And => 30,
            Self::Equal
            | Self::NotEqual
            | Self::Less
            | Self::LessEqual
            | Self::Greater
            | Self::GreaterEqual => 40,
            Self::Add | Self::Subtract => 50,
            Self::Multiply | Self::Divide | Self::Remainder => 60,
        }
    }
}
