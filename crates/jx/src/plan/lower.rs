use super::*;

#[derive(Default)]
pub(super) struct Lower {
    pub(super) instructions: Vec<Instruction>,
    paths: Vec<Path>,
    loaded: Vec<(u8, u8)>,
    lookups: Vec<(Box<Data>, Path)>,
    pub(super) operations: usize,
    parameters: Vec<Box<str>>,
    path_sources: Vec<Option<u8>>,
    lookup_sources: Vec<Option<u8>>,
    available: u32,
}
impl Lower {
    pub(super) fn parameters(parameters: &[Box<str>]) -> Self {
        Self {
            parameters: parameters.to_vec(),
            ..Self::default()
        }
    }
    pub(super) fn set_parameters(&mut self, parameters: &[Box<str>]) {
        self.parameters = parameters.to_vec();
    }
    fn load(&mut self, path: Path, source: Option<u8>) -> Option<u8> {
        let index = self
            .paths
            .iter()
            .enumerate()
            .position(|(i, p)| {
                p.fields == path.fields
                    && p.rooted == path.rooted
                    && self.path_sources.get(i).copied().flatten() == source
            })
            .unwrap_or_else(|| {
                self.paths.push(path);
                if !self.parameters.is_empty() {
                    self.path_sources.push(source);
                }
                self.paths.len() - 1
            }) as u8;
        if let Some((_, slot)) = self.loaded.iter().find(|(p, _)| *p == index) {
            return Some(*slot);
        }
        let slot = self.emit(Instruction::Load(index))?;
        self.loaded.push((index, slot));
        Some(slot)
    }

    pub(super) fn emit(&mut self, instruction: Instruction) -> Option<u8> {
        let same = |other: &Instruction| match (&instruction, other) {
            (Instruction::Negate(a), Instruction::Negate(b)) => a == b,
            (Instruction::Binary(op, a, b), Instruction::Binary(q, c, d)) => {
                op == q && a == c && b == d
            }
            _ => false,
        };
        if let Some((slot, _)) = self
            .instructions
            .iter()
            .enumerate()
            .find(|(slot, i)| self.available & (1 << slot) != 0 && same(i))
        {
            return Some(slot as u8);
        }
        if self.instructions.len() == SLOTS {
            return None;
        }
        let at = self.instructions.len() as u8;
        if matches!(
            instruction,
            Instruction::Binary(..) | Instruction::Negate(_)
        ) {
            self.available |= 1 << at;
        }
        self.instructions.push(instruction);
        Some(at)
    }
    pub(super) fn finish(self, result: u8) -> Program {
        let mut capture = Demand::default();
        let mut inputs = if self.parameters.is_empty() {
            Vec::new()
        } else {
            vec![None; self.instructions.len()]
        };
        let mut argument_demands: Vec<(Option<u8>, Demand)> = Vec::new();
        for (slot, instruction) in self.instructions.iter().enumerate() {
            let (path, source) = match instruction {
                Instruction::Load(path) => (
                    &self.paths[usize::from(*path)],
                    self.path_sources.get(usize::from(*path)).copied().flatten(),
                ),
                Instruction::Lookup(index) => (
                    &self.lookups[usize::from(*index)].1,
                    self.lookup_sources
                        .get(usize::from(*index))
                        .copied()
                        .flatten(),
                ),
                _ => continue,
            };
            if self.parameters.is_empty() {
                capture.insert(&path.fields, slot);
            } else {
                inputs[slot] = source;
                let index = argument_demands
                    .iter()
                    .position(|(s, _)| *s == source)
                    .unwrap_or_else(|| {
                        argument_demands.push((source, Demand::default()));
                        argument_demands.len() - 1
                    });
                argument_demands[index].1.insert(&path.fields, slot);
            }
        }
        Program {
            capture,
            inputs: inputs.into_boxed_slice(),
            argument_demands: argument_demands.into_boxed_slice(),
            instructions: self.instructions.into_boxed_slice(),
            paths: self.paths.into_boxed_slice(),
            lookups: self.lookups.into_boxed_slice(),
            result,
        }
    }
    // The mask names reusable primitive results within the existing 32-slot bound.
    // Only dominating loads/results survive a branch join.
    fn branch(
        &mut self,
        test: u8,
        yes: impl FnOnce(&mut Self) -> Option<u8>,
        no: impl FnOnce(&mut Self) -> Option<u8>,
    ) -> Option<u8> {
        let loaded = self.loaded.clone();
        let available = self.available;
        let branch = self.emit(Instruction::Branch(test, false, 0))?;
        let yes = yes(self)?;
        let result = self.emit(Instruction::Copy(yes))?;
        let jump = self.emit(Instruction::Jump(0))?;
        self.instructions[usize::from(branch)] =
            Instruction::Branch(test, false, self.instructions.len() as u8);
        self.loaded = loaded.clone();
        self.available = available;
        let no = no(self)?;
        self.emit(Instruction::Merge(no, result))?;
        self.instructions[usize::from(jump)] = Instruction::Jump(self.instructions.len() as u8);
        self.loaded = loaded;
        self.available = available;
        self.operations += 1;
        Some(result)
    }
    pub(super) fn node(&mut self, node: &Node) -> Option<u8> {
        let instruction = match &node.kind {
            Kind::Path(path) => return self.load(path.clone(), None),
            Kind::Variable(_) | Kind::Route(_, false) if !self.parameters.is_empty() => {
                let (path, source) = callback::path(node, &self.parameters)?;
                return self.load(path, Some(source));
            }
            Kind::StaticLookup(data, key) => {
                let (path, source) = match &key.kind {
                    Kind::Path(path) => (path.clone(), None),
                    _ if !self.parameters.is_empty() => {
                        let (path, source) = callback::path(key, &self.parameters)?;
                        (path, Some(source))
                    }
                    _ => return None,
                };
                let index = self.lookups.len() as u8;
                self.lookups.push((data.clone(), path));
                if !self.parameters.is_empty() {
                    self.lookup_sources.push(source);
                }
                self.operations += 1;
                Instruction::Lookup(index)
            }
            Kind::Conditional(test, yes, no) => {
                let test = self.node(test)?;
                return self.branch(
                    test,
                    |this| this.node(yes),
                    |this| match no {
                        Some(no) => this.node(no),
                        None => this.emit(Instruction::Missing),
                    },
                );
            }
            Kind::Binary(op @ (Op::And | Op::Or), left, right) => {
                let left = self.node(left)?;
                let result = self.emit(Instruction::Truth(left))?;
                let branch = self.emit(Instruction::Branch(result, matches!(op, Op::Or), 0))?;
                let loaded = self.loaded.clone();
                let available = self.available;
                let right = self.node(right)?;
                let right = self.emit(Instruction::Truth(right))?;
                self.emit(Instruction::Merge(right, result))?;
                self.instructions[usize::from(branch)] = Instruction::Branch(
                    result,
                    matches!(op, Op::Or),
                    self.instructions.len() as u8,
                );
                self.loaded = loaded;
                self.available = available;
                self.operations += 1;
                return Some(result);
            }
            Kind::Boolean(b) => Instruction::Boolean(*b),
            Kind::Number(n) => Instruction::Number(*n),
            Kind::Missing => Instruction::Missing,
            Kind::Prepared(p) => match p.data {
                Data::Number(n) => Instruction::Number(n),
                Data::Boolean(b) => Instruction::Boolean(b),
                Data::Missing => Instruction::Missing,
                _ => return None,
            },
            Kind::Group(child) => return self.node(child),
            Kind::Negate(child) => {
                let a = self.node(child)?;
                self.operations += 1;
                Instruction::Negate(a)
            }
            Kind::Binary(
                op @ (Op::Add
                | Op::Subtract
                | Op::Multiply
                | Op::Divide
                | Op::Remainder
                | Op::Equal
                | Op::NotEqual
                | Op::Less
                | Op::LessEqual
                | Op::Greater
                | Op::GreaterEqual),
                a,
                b,
            ) => {
                let a = self.node(a)?;
                let b = self.node(b)?;
                self.operations += 1;
                Instruction::Binary(*op, a, b)
            }
            _ => return None,
        };
        self.emit(instruction)
    }
}

// Values returned directly from input must keep their borrowed representation.
// A primitive result is safe only after an operation has consumed that input.
pub(super) fn computed(node: &Node) -> bool {
    match &node.kind {
        Kind::Path(_) | Kind::Variable(_) | Kind::Route(..) => false,
        Kind::Group(child) => computed(child),
        Kind::Conditional(_, yes, no) => computed(yes) && no.as_ref().is_none_or(|n| computed(n)),
        _ => true,
    }
}
