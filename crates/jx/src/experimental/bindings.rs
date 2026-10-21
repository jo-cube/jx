//! Compilation-time bindings from semantic field reads to acquired span slots.
//! Expected base/scope guards make these hints safe for branches and later
//! programs. There is no record-value cache or evaluator fallback.
use super::*;
use std::num::NonZeroUsize;
#[derive(Clone, Copy, Debug)]
pub(super) struct Read {
    pub base: usize,
    pub parent: usize,
    pub child: NonZeroUsize,
}
#[derive(Debug)]
pub(crate) struct Bound {
    pub key: Arc<()>,
    pub(super) fields: Box<[Option<Binding>]>,
}
#[derive(Clone, Copy)]
pub(super) struct Reads<'a> {
    pub fields: &'a [Option<Binding>],
    pub row: Option<&'a acquisition::Schema>,
}
pub(super) type Binding = Read;
impl Read {
    pub fn at(&self, base: usize) -> Option<Self> {
        (self.base == base).then_some(*self)
    }
}
impl<'a> Reads<'a> {
    pub fn get(self, out: usize) -> Option<(&'a Binding, Option<&'a acquisition::Schema>)> {
        self.fields[out].as_ref().map(|r| (r, self.row))
    }
}
struct Origins<'a> {
    root: Option<usize>,
    focus: Option<usize>,
    argument: Option<usize>,
    outer: &'a [Option<usize>],
}
pub(super) fn bind(
    p: &Program,
    schema: &acquisition::Schema,
    root: bool,
    focus: bool,
    argument: bool,
) -> Box<[Option<Binding>]> {
    let origin = |known: bool| known.then_some(0);
    analyze(
        p,
        schema,
        Origins {
            root: origin(root),
            focus: origin(focus),
            argument: origin(argument),
            outer: &[],
        },
        None,
        false,
    )
}
// Row layouts cross non-iterating focus regions with the same origin facts as
// root layouts. Iterating children retain their independent acquisition scopes.
pub(super) fn bind_rows(p: &mut Program, root: bool, focus: bool, argument: bool) {
    if !p
        .code
        .iter()
        .any(|i| matches!(i.kind, Operation::Construct(_)))
    {
        p.row_reads = bind(p, &p.row_schema, root, focus, argument);
        return;
    }
    let schema = Arc::clone(&p.row_schema);
    let mut bound = Vec::new();
    child_bindings(
        p,
        &schema,
        Origins {
            root: root.then_some(0),
            focus: focus.then_some(0),
            argument: argument.then_some(0),
            outer: &[],
        },
        &mut bound,
        true,
    );
    fn install(
        p: &mut Program,
        schema: &Arc<acquisition::Schema>,
        bound: &mut impl Iterator<Item = Bound>,
    ) {
        let table = bound.next().expect("region binding preorder");
        debug_assert!(Arc::ptr_eq(&p.key, &table.key));
        p.row_schema = Arc::clone(schema);
        p.row_reads = table.fields;
        for i in &mut p.code {
            if let Operation::Construct(body) = &mut i.kind {
                install(body, schema, bound);
            }
        }
    }
    let mut tables = bound.into_iter();
    install(p, &schema, &mut tables);
    debug_assert!(tables.next().is_none());
}
fn child_bindings(
    p: &Program,
    schema: &acquisition::Schema,
    origins: Origins<'_>,
    bound: &mut Vec<Bound>,
    regions: bool,
) {
    // Parent entries come first: a top-level activation normally binds once.
    let at = bound.len();
    bound.push(Bound {
        key: Arc::clone(&p.key),
        fields: Box::new([]),
    });
    bound[at].fields = analyze(p, schema, origins, Some(bound), regions);
}
pub(super) fn bind_tree(programs: &[&Program], schema: &acquisition::Schema) -> Vec<Bound> {
    let mut bound = Vec::new();
    for p in programs {
        child_bindings(
            p,
            schema,
            Origins {
                root: Some(0),
                focus: Some(0),
                argument: None,
                outer: &[],
            },
            &mut bound,
            false,
        );
    }
    bound
}
fn analyze(
    p: &Program,
    schema: &acquisition::Schema,
    origins: Origins<'_>,
    mut children: Option<&mut Vec<Bound>>,
    regions: bool,
) -> Box<[Option<Binding>]> {
    let Origins {
        root,
        focus,
        argument,
        outer,
    } = origins;
    let mut nodes = vec![None; p.registers];
    let mut functions: Vec<Option<(&call::Definition, Option<usize>)>> = vec![None; p.registers];
    let mut fields = (0..p.registers).map(|_| None).collect::<Vec<_>>();
    let mut definitions = vec![0; p.registers];
    for i in &p.code {
        definitions[i.out] += 1;
    }
    for i in &p.code {
        functions[i.out] = if definitions[i.out] == 1 {
            match &i.kind {
                Operation::Function(d) => Some((d.as_ref(), focus)),
                Operation::Copy(r) | Operation::Materialize(r) => functions[*r],
                _ => None,
            }
        } else {
            None
        };
        if let Some(bound) = children.as_deref_mut() {
            let mut child = |program: &Program, focus: Option<usize>, argument: Option<usize>| {
                child_bindings(
                    program,
                    schema,
                    Origins {
                        root,
                        focus,
                        argument,
                        outer: &nodes,
                    },
                    bound,
                    regions,
                );
            };
            match i.kind.producer() {
                Operation::Construct(body) => child(body, focus, None),
                _ if regions => {}
                Operation::Function(d) => child(&d.body, focus, None),
                Operation::Group(..)
                | Operation::Order(..)
                | Operation::Navigate(_)
                | Operation::Select(..) => i.kind.children(&mut |p| child(p, None, None)),
                Operation::Path(path) => {
                    child(&path.body, nodes[path.source], None);
                    for p in &path.predicates {
                        child(p, nodes[path.source], None);
                    }
                }
                Operation::Each { body, source, .. } => child(body, focus, nodes[*source]),
                Operation::Fold {
                    body,
                    predicate,
                    source,
                    callback,
                    ..
                } => {
                    child(
                        body,
                        if *callback { focus } else { nodes[*source] },
                        nodes[*source],
                    );
                    if let Some(p) = predicate {
                        child(p, nodes[*source], None);
                    }
                }
                _ => {}
            }
        }
        let node = match i.kind.producer() {
            Operation::Invoke(target, args, ..) => {
                functions[*target].and_then(|(d, definition_focus)| {
                    let result = d.demands.result.as_ref()?;
                    let mut at = match result.scope {
                        0 => root,
                        1 => definition_focus,
                        n => args.get(n - 2).and_then(|r| nodes[*r]),
                    }?;
                    for field in &result.path {
                        at = schema.child(at, field)?;
                    }
                    Some(at)
                })
            }
            Operation::Root => root,
            Operation::Focus => focus,
            Operation::Argument(0) => argument,
            Operation::Outer(r, _) => outer.get(*r).copied().flatten(),
            Operation::Copy(r) | Operation::Materialize(r) => nodes[*r],
            Operation::Field(base, path, _, _) => {
                fields[i.out] = nodes[*base].and_then(|base| {
                    let mut at = Some(base);
                    let mut parent = base;
                    for name in &path.fields {
                        at = at.and_then(|id| {
                            parent = id;
                            schema.child(id, name)
                        });
                    }
                    at.map(|child| Read {
                        base,
                        parent,
                        child: NonZeroUsize::new(child).expect("nonempty path"),
                    })
                });
                fields[i.out].map(|r| r.child.get())
            }
            _ => None,
        };
        nodes[i.out] = if definitions[i.out] > 1 || matches!(i.kind, Operation::Reduce(..)) {
            None
        } else {
            node
        };
    }
    fields.into()
}
