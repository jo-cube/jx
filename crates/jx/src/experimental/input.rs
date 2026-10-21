use super::*;
use crate::{Error, RawJson, evaluate::Operand, sequence::Context};
use std::num::NonZeroUsize;

// A row view and its immutable schema have different identity namespaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RowId(NonZeroUsize);
impl RowId {
    fn of(row: &acquisition::Prepared<'_, '_>) -> Self {
        Self(NonZeroUsize::new(std::ptr::from_ref(row).addr()).expect("non-null borrowed row"))
    }
}
#[derive(Clone, Debug)]
pub(super) enum Cell<'e, 'i> {
    Missing,
    Indexed(std::rc::Rc<Indexed<'e, 'i>>, usize),
    Capture(usize),
    // Opaque row-view identity, never dereferenced. Row spans live in Input; a
    // cell is usable only while that input activation contains its row view.
    RowCapture(RowId, usize),
    Value(Value<'e, 'i>),
}
#[derive(Debug)]
pub(super) struct Indexed<'e, 'i> {
    schema: &'e acquisition::Schema,
    raw: RawJson<'i>,
    spans: Box<[acquisition::Span]>,
}
impl<'i> Indexed<'_, 'i> {
    fn at(&self, id: usize) -> Option<RawJson<'i>> {
        let span = self.spans[id];
        (span.end > span.start).then(|| RawJson(&self.raw.as_str()[span.start..span.end]))
    }
}
pub(crate) enum Input<'a, 'i, 's> {
    Raw(Option<RawJson<'i>>),
    Captured(&'a acquisition::Prepared<'i, 's>),
    CapturedRow {
        parent: &'a Input<'a, 'i, 's>,
        row: &'a acquisition::Prepared<'i, 'a>,
    },
}
impl<'i> Input<'_, 'i, '_> {
    pub(super) fn snapshot(&self) -> acquisition::Snapshot<'i> {
        match self {
            Self::Raw(raw) => acquisition::Snapshot::Raw(*raw),
            Self::Captured(d) => {
                let (schema, bindings) = d.owner.expect("root preparation owns immutable metadata");
                acquisition::Snapshot::Captured {
                    text: d.text,
                    schema: Arc::clone(schema),
                    spans: d.spans.into(),
                    bindings: Arc::clone(bindings),
                }
            }
            Self::CapturedRow { .. } => {
                unreachable!("row activations do not escape into public evaluations")
            }
        }
    }
    // Recover a prepared port only by byte identity. Equal constructed values
    // are never substituted, and no record state is retained by a closure.
    pub(super) fn restore<'e>(&self, value: Option<Value<'e, 'i>>) -> Cell<'e, 'i> {
        let Some(value) = value else {
            return Cell::Missing;
        };
        if let Value::Raw(raw) = &value {
            let locate = |d: &acquisition::Prepared<'i, '_>| {
                d.spans.iter().enumerate().find_map(|(id, _)| {
                    d.at(id)
                        .filter(|v| {
                            v.as_bytes().as_ptr() == raw.as_bytes().as_ptr()
                                && v.as_bytes().len() == raw.as_bytes().len()
                        })
                        .map(|_| id)
                })
            };
            let mut active = self;
            while let Self::CapturedRow { parent, row } = active {
                if let Some(id) = locate(row) {
                    return Cell::RowCapture(RowId::of(row), id);
                }
                active = parent;
            }
            if let Some(d) = self.root_document()
                && let Some(id) = locate(d)
            {
                return Cell::Capture(id);
            }
        }
        if matches!(value, Value::Undefined) {
            Cell::Missing
        } else {
            Cell::Value(value)
        }
    }
    pub(super) fn root<'e>(&self) -> Cell<'e, 'i> {
        match self {
            Self::Raw(raw) => raw.map_or(Cell::Missing, |raw| Cell::Value(Value::Raw(raw))),
            _ => Cell::Capture(0),
        }
    }
    fn root_document(&self) -> Option<&acquisition::Prepared<'i, '_>> {
        match self {
            Self::Raw(_) => None,
            Self::Captured(d) => Some(d),
            Self::CapturedRow { parent, .. } => parent.root_document(),
        }
    }
    fn row_document(&self, row_id: RowId) -> Option<&acquisition::Prepared<'i, '_>> {
        match self {
            Self::CapturedRow { row, .. } if std::ptr::from_ref(*row).addr() == row_id.0.get() => {
                Some(row)
            }
            Self::CapturedRow { parent, .. } => parent.row_document(row_id),
            _ => None,
        }
    }
    pub(super) fn reads<'p>(&'p self, p: &'p Program) -> Option<bindings::Reads<'p>>
    where
        Self: 'p,
    {
        let mut active = self;
        while let Self::CapturedRow { parent, row } = active {
            if std::ptr::eq(row.schema, &*p.row_schema) {
                return Some(bindings::Reads {
                    fields: &p.row_reads,
                    row: Some(&p.row_schema),
                });
            }
            active = parent;
        }
        let key = &p.key;
        self.root_document()?
            .bindings
            .iter()
            .find(|b| Arc::ptr_eq(&b.key, key))
            .map(|b| bindings::Reads {
                fields: &b.fields,
                row: None,
            })
    }
    #[inline]
    pub(super) fn select_field<'e>(
        &self,
        base: &Cell<'e, 'i>,
        path: &'e crate::expression::Path,
        wrapped: bool,
        read: Option<(&bindings::Binding, Option<&acquisition::Schema>)>,
    ) -> FieldResult<'e, 'i> {
        let capture = match base {
            Cell::Capture(id) => self.root_document().map(|d| (d, *id, None)),
            Cell::RowCapture(schema, id) => {
                self.row_document(*schema).map(|d| (d, *id, Some(*schema)))
            }
            _ => None,
        };
        if let Some((doc, id, row)) = capture {
            let cell = |child| {
                if let Some(schema) = row {
                    Cell::RowCapture(schema, child)
                } else {
                    Cell::Capture(child)
                }
            };
            if let Some((read, scope)) = read
                && match (scope, row) {
                    (None, None) => true,
                    (Some(a), Some(_)) => std::ptr::eq(a, doc.schema),
                    _ => false,
                }
                && let Some(read) = read.at(id)
            {
                let child = read.child.get();
                if doc.at(child).is_some() {
                    return FieldResult::Cell(cell(child));
                }
                if doc.at(read.parent).is_some_and(|raw| !raw.is_array()) {
                    return FieldResult::Cell(Cell::Missing);
                }
            }
        }
        self.select_unbound(base, path, wrapped)
    }
    // Keep indexed/dynamic navigation out of the common bound-span kernel.
    // Every failed binding guard continues here with the same value, once.
    fn select_unbound<'e>(
        &self,
        base: &Cell<'e, 'i>,
        path: &'e crate::expression::Path,
        wrapped: bool,
    ) -> FieldResult<'e, 'i> {
        if let Cell::Indexed(doc, id) = base {
            let mut at = *id;
            let mut complete = true;
            for name in &path.fields {
                if doc.at(at).is_none() {
                    return FieldResult::Cell(Cell::Missing);
                }
                if doc.at(at).is_some_and(|raw| raw.is_array()) {
                    complete = false;
                    break;
                }
                let Some(child) = doc.schema.child(at, name) else {
                    complete = false;
                    break;
                };
                at = child;
            }
            if complete {
                return FieldResult::Cell(if doc.at(at).is_some() {
                    Cell::Indexed(std::rc::Rc::clone(doc), at)
                } else {
                    Cell::Missing
                });
            }
        }
        let capture = match base {
            Cell::Capture(id) => self.root_document().map(|d| (d, *id, None)),
            Cell::RowCapture(schema, id) => {
                self.row_document(*schema).map(|d| (d, *id, Some(*schema)))
            }
            _ => None,
        };
        if let Some((doc, id, row)) = capture {
            let cell = |child| {
                if let Some(schema) = row {
                    Cell::RowCapture(schema, child)
                } else {
                    Cell::Capture(child)
                }
            };
            let mut at = id;
            let mut complete = true;
            for name in &path.fields {
                if doc.at(at).is_none() {
                    return FieldResult::Cell(Cell::Missing);
                }
                if doc.at(at).is_some_and(|raw| raw.is_array()) {
                    complete = false;
                    break;
                }
                let Some(child) = doc.schema.child(at, name) else {
                    complete = false;
                    break;
                };
                at = child;
            }
            if complete {
                return FieldResult::Cell(if doc.at(at).is_some() {
                    cell(at)
                } else {
                    Cell::Missing
                });
            }
        }
        let Some(value) = base.value(self) else {
            return FieldResult::Cell(Cell::Missing);
        };
        let context = Context {
            value,
            wrapped,
            scope: None,
        };
        FieldResult::Path(path.select_context(&context))
    }
    pub(super) fn each<'e, E>(
        &self,
        source: &Cell<'e, 'i>,
        schema: &'e acquisition::Schema,
        emit: impl FnMut(&Input<'_, 'i, '_>, Cell<'e, 'i>, usize) -> Result<(), E>,
    ) -> Result<(), E> {
        self.each_selected(source, schema, |_| true, emit)
    }
    pub(super) fn each_selected<'e, E>(
        &self,
        source: &Cell<'e, 'i>,
        schema: &'e acquisition::Schema,
        mut selected: impl FnMut(usize) -> bool,
        mut emit: impl FnMut(&Input<'_, 'i, '_>, Cell<'e, 'i>, usize) -> Result<(), E>,
    ) -> Result<(), E> {
        let Some(value) = source.value(self) else {
            return Ok(());
        };
        if self.root_document().is_some() && schema.has_fields() {
            if let Value::Raw(raw) = &value
                && raw.is_array()
            {
                let mut inline = [acquisition::Span::default(); 16];
                let mut heap = Vec::new();
                let spans = if schema.nodes.len() <= inline.len() {
                    &mut inline[..schema.nodes.len()]
                } else {
                    heap.resize(schema.nodes.len(), acquisition::Span::default());
                    heap.as_mut_slice()
                };

                return crate::json::schema_each(*raw, schema, spans, |spans, index| {
                    if !selected(index) {
                        return Ok(());
                    }
                    let row = acquisition::Prepared {
                        text: raw.as_str(),
                        schema,
                        spans,
                        bindings: &[],
                        owner: None,
                    };
                    emit(
                        &Input::CapturedRow {
                            parent: self,
                            row: &row,
                        },
                        Cell::RowCapture(RowId::of(&row), 0),
                        index,
                    )
                });
            }
            return if value.is_array() {
                self.each_values(value.elements(), schema, selected, emit)
            } else {
                self.each_values(std::iter::once(value), schema, selected, emit)
            };
        }
        if value.is_array() {
            for (index, item) in value.elements().enumerate() {
                if selected(index) {
                    emit(self, Cell::Value(item), index)?;
                }
            }
        } else if selected(0) {
            emit(self, source.clone(), 0)?;
        }
        Ok(())
    }
    // Retained candidates borrow original values; only one row's requested
    // spans are live. This is shared by native sequences and eager stage barriers.
    pub(super) fn each_values<'e, E>(
        &self,
        values: impl Iterator<Item = Value<'e, 'i>>,
        schema: &'e acquisition::Schema,
        mut selected: impl FnMut(usize) -> bool,
        mut emit: impl FnMut(&Input<'_, 'i, '_>, Cell<'e, 'i>, usize) -> Result<(), E>,
    ) -> Result<(), E> {
        let acquire = self.root_document().is_some() && schema.has_fields();
        let mut inline = [acquisition::Span::default(); 16];
        let mut heap = Vec::new();
        let spans = if !acquire {
            &mut inline[..0]
        } else if schema.nodes.len() <= inline.len() {
            &mut inline[..schema.nodes.len()]
        } else {
            heap.resize(schema.nodes.len(), acquisition::Span::default());
            heap.as_mut_slice()
        };
        for (index, value) in values.enumerate() {
            if !selected(index) {
                continue;
            }
            if acquire
                && let Value::Raw(raw) = value
                && raw.as_bytes()[0] == b'{'
            {
                crate::json::capture_schema_validated(raw, schema, spans);
                let row = acquisition::Prepared {
                    text: raw.as_str(),
                    schema,
                    spans,
                    bindings: &[],
                    owner: None,
                };
                emit(
                    &Input::CapturedRow {
                        parent: self,
                        row: &row,
                    },
                    Cell::RowCapture(RowId::of(&row), 0),
                    index,
                )?;
            } else {
                emit(self, Cell::Value(value), index)?;
            }
        }
        Ok(())
    }
    pub(super) fn each_ranges<'e, E>(
        &self,
        raw: RawJson<'i>,
        ranges: &[acquisition::Span],
        schema: &'e acquisition::Schema,
        mut emit: impl FnMut(&Input<'_, 'i, '_>, Cell<'e, 'i>, usize) -> Result<(), E>,
    ) -> Result<(), E> {
        if self.root_document().is_some() && schema.has_fields() {
            let mut inline = [acquisition::Span::default(); 16];
            let mut heap = Vec::new();
            let spans = if schema.nodes.len() <= inline.len() {
                &mut inline[..schema.nodes.len()]
            } else {
                heap.resize(schema.nodes.len(), acquisition::Span::default());
                heap.as_mut_slice()
            };
            crate::json::schema_ranges_each(raw, ranges, schema, spans, |spans, index| {
                let row = acquisition::Prepared {
                    text: raw.as_str(),
                    schema,
                    spans,
                    bindings: &[],
                    owner: None,
                };
                emit(
                    &Input::CapturedRow {
                        parent: self,
                        row: &row,
                    },
                    Cell::RowCapture(RowId::of(&row), 0),
                    index,
                )
            })
        } else {
            crate::json::ranges_each(raw, ranges, |raw, index| {
                emit(self, Cell::Value(Value::Raw(raw)), index)
            })
        }
    }
}
impl<'e, 'i> Cell<'e, 'i> {
    pub(super) fn acquire(
        &self,
        input: &Input<'_, 'i, '_>,
        schema: &'e acquisition::Schema,
    ) -> Self {
        let covered = match self {
            Self::Capture(id) => input
                .root_document()
                .is_some_and(|d| d.schema.covers(*id, schema)),
            Self::RowCapture(row, id) => input
                .row_document(*row)
                .is_some_and(|d| d.schema.covers(*id, schema)),
            Self::Indexed(doc, id) => doc.schema.covers(*id, schema),
            _ => false,
        };
        if covered {
            return self.clone();
        }
        if let Some(Value::Raw(raw)) = self.value(input)
            && raw.as_bytes()[0] == b'{'
        {
            let mut spans =
                vec![acquisition::Span::default(); schema.nodes.len()].into_boxed_slice();
            crate::json::capture_schema_validated(raw, schema, &mut spans);
            return Self::Indexed(std::rc::Rc::new(Indexed { schema, raw, spans }), 0);
        }
        self.clone()
    }

    pub(super) fn value(&self, input: &Input<'_, 'i, '_>) -> Option<Value<'e, 'i>> {
        match self {
            Self::Missing => None,
            Self::Indexed(doc, id) => doc.at(*id).map(Value::Raw),
            Self::Capture(id) => input
                .root_document()
                .expect("captured root")
                .at(*id)
                .map(Value::Raw),
            Self::RowCapture(schema, id) => input
                .row_document(*schema)
                .expect("active lexical row")
                .at(*id)
                .map(Value::Raw),
            Self::Value(v) => Some(v.clone()),
        }
    }
    pub(super) fn into_value(self, input: &Input<'_, 'i, '_>) -> Option<Value<'e, 'i>> {
        match self {
            Self::Value(value) => Some(value),
            cell => cell.value(input),
        }
    }

    // Transfer a borrowed numeric token into a primitive only when semantic uses
    // permit it. This never checks type/range or changes an owned value's identity.
    #[inline]
    pub(super) fn atomic(self, input: &Input<'_, 'i, '_>) -> Self {
        match self {
            Self::Value(Value::Raw(raw)) => Self::Value(Value::Raw(raw).atomic()),
            Self::Capture(_) | Self::RowCapture(..) | Self::Indexed(..) => self
                .value(input)
                .map(|v| v.atomic())
                .map_or(Self::Missing, Self::Value),
            _ => self,
        }
    }
    #[inline]
    pub(super) fn number(
        &self,
        input: &Input<'_, 'i, '_>,
        offset: usize,
    ) -> Result<Option<f64>, Error> {
        if let Self::Value(Value::Number(n)) = self {
            return checked_number(*n, offset);
        }
        let Some(value) = self.value(input) else {
            return Ok(None);
        };
        match value.atomic() {
            Value::Undefined => Ok(None),
            Value::Number(n) => checked_number(n, offset),
            _ => Err(crate::value::type_error(offset)),
        }
    }
    #[inline]
    pub(super) fn arithmetic(
        &self,
        op: Op,
        right: &Self,
        input: &Input<'_, 'i, '_>,
        offset: usize,
    ) -> Result<Self, Error> {
        let left = self.number(input, offset)?;
        let right = right.number(input, offset)?;
        Ok(match (left, right) {
            (Some(l), Some(r)) => Self::Value(Value::Number(match op {
                Op::Add => l + r,
                Op::Subtract => l - r,
                Op::Multiply => l * r,
                Op::Divide => l / r,
                Op::Remainder => l % r,
                _ => unreachable!("numeric IR operation"),
            })),
            _ => Self::Missing,
        })
    }
    pub(super) fn truth(&self, input: &Input<'_, 'i, '_>, offset: usize) -> Result<bool, Error> {
        if let Self::Value(Value::Boolean(value)) = self {
            return Ok(*value);
        }
        self.value(input).map_or(Ok(false), |v| v.truth(offset))
    }
    pub(super) fn operand(&self, input: &Input<'_, 'i, '_>) -> Operand<'e, 'i> {
        self.value(input).map_or(Operand::Missing, Operand::One)
    }
    pub(super) fn from_operand(value: Operand<'e, 'i>) -> Result<Self, Error> {
        Self::from_native_operand(value.normalize())
    }
    pub(super) fn from_native_operand(value: Operand<'e, 'i>) -> Result<Self, Error> {
        Ok(crate::function::retained(value)?.map_or(Self::Missing, Self::Value))
    }
}
// A path stays borrowed/lazy until its consumer actually needs a retained value.
// Scalar captures bypass sequence machinery. Both choices share the same read.
pub(super) enum FieldResult<'e, 'i> {
    Cell(Cell<'e, 'i>),
    Path(crate::path::PathEvaluation<'e, 'i>),
}
impl<'e, 'i> FieldResult<'e, 'i> {
    pub(super) fn retain(self) -> Result<Cell<'e, 'i>, Error> {
        match self {
            Self::Cell(cell) => Ok(cell),
            Self::Path(path) => Cell::from_operand(path.operand()?),
        }
    }
    pub(super) fn walk<E>(
        self,
        input: &Input<'_, 'i, '_>,
        output: &mut dyn FnMut(Value<'e, 'i>) -> Result<(), crate::ConsumeError<E>>,
    ) -> Result<(), crate::ConsumeError<E>> {
        match self {
            Self::Cell(cell) => {
                if let Some(value) = cell.into_value(input) {
                    output(value)?;
                }
                Ok(())
            }
            Self::Path(path) => path.try_for_each(output),
        }
    }
}

#[inline]
fn checked_number(n: f64, offset: usize) -> Result<Option<f64>, Error> {
    if n.is_infinite() {
        Err(crate::value::range_error(offset))
    } else if n.is_nan() {
        Err(crate::value::type_error(offset))
    } else {
        Ok(Some(n))
    }
}
