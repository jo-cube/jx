use super::*;

#[derive(Default)]
pub(super) struct Lower {
    pub(super) instructions: Vec<Instruction>,
    paths: Vec<Path>,
    loaded: Vec<(u8, u8)>,
    lookups: Vec<(Box<Data>, Path)>,
    pub(super) operations: usize,
}
impl Lower {
    pub(super) fn emit(&mut self, instruction: Instruction) -> Option<u8> {
        if self.instructions.len() == SLOTS {
            return None;
        }
        let at = self.instructions.len() as u8;
        self.instructions.push(instruction);
        Some(at)
    }
    pub(super) fn finish(self, result: u8) -> Program {
        let capture: Box<[(Box<str>, u8)]> = if self.paths.len() >= 2
            && self.lookups.is_empty()
            && self.paths.iter().all(|p| p.fields.len() == 1)
        {
            self.instructions
                .iter()
                .enumerate()
                .filter_map(|(slot, instruction)| match instruction {
                    Instruction::Load(path) => {
                        Some((self.paths[usize::from(*path)].fields[0].clone(), slot as u8))
                    }
                    _ => None,
                })
                .collect()
        } else {
            Box::new([])
        };
        Program {
            capture,
            instructions: self.instructions.into_boxed_slice(),
            paths: self.paths.into_boxed_slice(),
            lookups: self.lookups.into_boxed_slice(),
            result,
        }
    }
    // Only loads dominating both branches can be reused after the join.
    fn branch(
        &mut self,
        test: u8,
        yes: impl FnOnce(&mut Self) -> Option<u8>,
        no: impl FnOnce(&mut Self) -> Option<u8>,
    ) -> Option<u8> {
        let loaded = self.loaded.clone();
        let branch = self.emit(Instruction::Branch(test, false, 0))?;
        let yes = yes(self)?;
        let result = self.emit(Instruction::Copy(yes))?;
        let jump = self.emit(Instruction::Jump(0))?;
        self.instructions[usize::from(branch)] =
            Instruction::Branch(test, false, self.instructions.len() as u8);
        self.loaded = loaded.clone();
        let no = no(self)?;
        self.emit(Instruction::Merge(no, result))?;
        self.instructions[usize::from(jump)] = Instruction::Jump(self.instructions.len() as u8);
        self.loaded = loaded;
        self.operations += 1;
        Some(result)
    }
    pub(super) fn node(&mut self, node: &Node) -> Option<u8> {
        let instruction = match &node.kind {
            Kind::Path(path) => {
                let index = self
                    .paths
                    .iter()
                    .position(|p| p.fields == path.fields && p.rooted == path.rooted)
                    .unwrap_or_else(|| {
                        self.paths.push(path.clone());
                        self.paths.len() - 1
                    }) as u8;
                if let Some((_, slot)) = self.loaded.iter().find(|(p, _)| *p == index) {
                    return Some(*slot);
                }
                let slot = self.emit(Instruction::Load(index))?;
                self.loaded.push((index, slot));
                return Some(slot);
            }
            Kind::StaticLookup(data, key) => {
                let Kind::Path(path) = &key.kind else {
                    return None;
                };
                let index = self.lookups.len() as u8;
                self.lookups.push((data.clone(), path.clone()));
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
                let right = self.node(right)?;
                let right = self.emit(Instruction::Truth(right))?;
                self.emit(Instruction::Merge(right, result))?;
                self.instructions[usize::from(branch)] = Instruction::Branch(
                    result,
                    matches!(op, Op::Or),
                    self.instructions.len() as u8,
                );
                self.loaded = loaded;
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
        Kind::Path(_) => false,
        Kind::Group(child) => computed(child),
        Kind::Conditional(_, yes, no) => computed(yes) && no.as_ref().is_none_or(|n| computed(n)),
        _ => true,
    }
}
