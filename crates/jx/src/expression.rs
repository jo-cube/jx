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
    // Reads/writes lexical state or creates/calls functions; unsafe to replay.
    pub effects: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Path(Path),
    Route(Box<[Step]>, bool), // A leading array constructor fixes the input focus.
    Filter(Box<Node>, Box<[Node]>),
    Group(Box<Node>),
    Builtin(crate::builtin::Builtin, Box<[Node]>),
    Variable(Box<str>),
    Bind(Box<str>, Box<Node>),
    Block(Box<[Node]>),
    Conditional(Box<Node>, Box<Node>, Option<Box<Node>>),
    Lambda(Box<[Box<str>]>, Box<Node>),
    Call(Box<Node>, Box<[Node]>),
    Array(Box<[Node]>, bool),
    Object(Box<[(Node, Node)]>),
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

#[derive(Clone, Debug)]
pub(crate) struct Step {
    pub node: Node,
    pub predicates: Box<[Node]>,
    pub lookup: bool,
    pub effects: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Aggregate {
    Count,
    Sum,
    Min,
    Max,
}

impl Node {
    pub(crate) fn array_focus(&self) -> bool {
        match &self.kind {
            Kind::Array(_, preserve) | Kind::Route(_, preserve) => *preserve,
            Kind::Filter(base, _) | Kind::Group(base) => base.array_focus(),
            _ => false,
        }
    }
    pub(crate) fn is_array_constructor(&self) -> bool {
        match &self.kind {
            Kind::Array(..) => true,
            Kind::Filter(base, _) => base.is_array_constructor(),
            _ => false,
        }
    }
    pub(crate) fn preserve_array(&mut self) {
        match &mut self.kind {
            Kind::Array(_, preserve) => *preserve = true,
            Kind::Filter(base, _) => base.preserve_array(),
            _ => {}
        }
    }
}
