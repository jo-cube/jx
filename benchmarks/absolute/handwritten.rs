//! Shape-specialized, fully validated reference. This is not a JSONata interpreter.
//! Parsing uses Serde seeds to discard unused fields and reduce arrays incrementally.
use crate::fixtures::{Case, Kind};
use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::{borrow::Cow, fmt};

pub struct Row<'a> {
    pub id: Cow<'a, str>,
    pub name: Cow<'a, str>,
    pub env: Cow<'a, str>,
    pub region: Cow<'a, str>,
    pub deleted: bool,
    pub metrics: [f64; 80],
    pub x: f64,
    pub y: f64,
}
impl Default for Row<'_> {
    fn default() -> Self {
        Self {
            id: Cow::Borrowed(""),
            name: Cow::Borrowed(""),
            env: Cow::Borrowed(""),
            region: Cow::Borrowed(""),
            deleted: false,
            metrics: [0.; 80],
            x: 0.,
            y: 0.,
        }
    }
}
// Borrow unescaped strings; decode escaped strings correctly without a document DOM.
struct Text;
impl<'de> DeserializeSeed<'de> for Text {
    type Value = Cow<'de, str>;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Cow<'de, str>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("string")
            }
            fn visit_borrowed_str<E: de::Error>(self, s: &'de str) -> Result<Self::Value, E> {
                Ok(Cow::Borrowed(s))
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<Self::Value, E> {
                Ok(Cow::Owned(s.into()))
            }
            fn visit_string<E: de::Error>(self, s: String) -> Result<Self::Value, E> {
                Ok(Cow::Owned(s))
            }
        }
        d.deserialize_str(V)
    }
}
struct Object<'a, 'de> {
    row: &'a mut Row<'de>,
    depth: u8,
    width: usize,
    strings: bool,
}
impl<'de> DeserializeSeed<'de> for Object<'_, 'de> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for Object<'_, 'de> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("record object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<(), A::Error> {
        while let Some(key) = m.next_key_seed(Text)? {
            match (self.depth, key.as_ref()) {
                (0, "n") => m.next_value_seed(Object {
                    depth: 0,
                    row: self.row,
                    width: self.width,
                    strings: self.strings,
                })?,
                (0, "a") => m.next_value_seed(Object {
                    depth: 4,
                    row: self.row,
                    width: 0,
                    strings: false,
                })?,
                (0, "metrics") => m.next_value_seed(Object {
                    depth: 1,
                    row: self.row,
                    width: self.width,
                    strings: self.strings,
                })?,
                (0, "profile") if self.strings => m.next_value_seed(Object {
                    depth: 2,
                    row: self.row,
                    width: 0,
                    strings: true,
                })?,
                (0, "nested") => m.next_value_seed(Object {
                    depth: 3,
                    row: self.row,
                    width: 0,
                    strings: false,
                })?,
                (3, "a") => m.next_value_seed(Object {
                    depth: 4,
                    row: self.row,
                    width: 0,
                    strings: false,
                })?,
                (0, "id") if self.strings => self.row.id = m.next_value_seed(Text)?,
                (0, "name") if self.strings => self.row.name = m.next_value_seed(Text)?,
                (0, "env") if self.strings => self.row.env = m.next_value_seed(Text)?,
                (0, "deleted") if self.strings => self.row.deleted = m.next_value()?,
                (2, "region") => self.row.region = m.next_value_seed(Text)?,
                (4, "x") => self.row.x = m.next_value()?,
                (4, "y") => self.row.y = m.next_value()?,
                (1, k)
                    if k.strip_prefix('v')
                        .and_then(|s| s.parse::<usize>().ok())
                        .is_some_and(|i| i < self.width) =>
                {
                    let i = k[1..].parse::<usize>().unwrap();
                    self.row.metrics[i] = m.next_value()?;
                }
                _ => {
                    m.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
pub fn parse<'a>(input: &'a [u8], width: usize, strings: bool) -> Row<'a> {
    let mut row = Row::default();
    let mut d = serde_json::Deserializer::from_slice(input);
    Object {
        row: &mut row,
        depth: 0,
        width,
        strings,
    }
    .deserialize(&mut d)
    .unwrap();
    d.end().unwrap();
    row
}
struct Items<'a, F> {
    width: usize,
    strings: bool,
    consume: &'a mut F,
}
impl<'de, F: FnMut(Row<'de>)> DeserializeSeed<'de> for Items<'_, F> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de, F: FnMut(Row<'de>)> Visitor<'de> for Items<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("items array")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        struct Element {
            width: usize,
            strings: bool,
        }
        impl<'de> DeserializeSeed<'de> for Element {
            type Value = Row<'de>;
            fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
                let mut row = Row::default();
                Object {
                    row: &mut row,
                    depth: 0,
                    width: self.width,
                    strings: self.strings,
                }
                .deserialize(d)?;
                Ok(row)
            }
        }
        while let Some(row) = seq.next_element_seed(Element {
            width: self.width,
            strings: self.strings,
        })? {
            (self.consume)(row)
        }
        Ok(())
    }
}
struct RootItems<'a, F> {
    width: usize,
    strings: bool,
    consume: &'a mut F,
}
impl<'de, F: FnMut(Row<'de>)> Visitor<'de> for RootItems<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("root object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<(), A::Error> {
        while let Some(key) = m.next_key_seed(Text)? {
            if key == "items" {
                m.next_value_seed(Items {
                    width: self.width,
                    strings: self.strings,
                    consume: self.consume,
                })?
            } else {
                m.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}
pub fn each<'de>(input: &'de [u8], strings: bool, mut consume: impl FnMut(Row<'de>)) {
    let mut d = serde_json::Deserializer::from_slice(input);
    de::Deserializer::deserialize_map(
        &mut d,
        RootItems {
            width: 2,
            strings,
            consume: &mut consume,
        },
    )
    .unwrap();
    d.end().unwrap();
}
pub fn number(out: &mut Vec<u8>, n: f64) {
    if n.fract() == 0. && n.abs() < 9_007_199_254_740_992. {
        serde_json::to_writer(out, &(n as i64)).unwrap()
    } else {
        serde_json::to_writer(out, &n).unwrap()
    }
}
fn string(out: &mut Vec<u8>, s: &str) {
    serde_json::to_writer(out, s).unwrap()
}
fn member(out: &mut Vec<u8>, name: &str, first: &mut bool) {
    if !*first {
        out.push(b',')
    }
    *first = false;
    string(out, name);
    out.push(b':')
}
pub fn render(
    case: &Case,
    row: &Row<'_>,
    labels: &std::collections::HashMap<String, String>,
    out: &mut Vec<u8>,
) {
    match case.kind {
        Kind::Scalar => number(out, row.metrics[0] * row.metrics[1] + row.metrics[0]),
        Kind::Nested => number(out, row.x * row.y + row.x),
        Kind::Lookup => {
            if let Some(label) = labels.get(row.id.as_ref()) {
                string(out, label)
            }
        }
        Kind::Predicate => out.extend_from_slice(
            if row.env == "production"
                && labels
                    .get(row.id.as_ref())
                    .is_some_and(|s| matches!(s.as_str(), "label-1" | "label-3" | "label-17"))
                && row.metrics[0] > 0.
                && row.region == "eu"
            {
                b"true"
            } else {
                b"false"
            },
        ),
        Kind::Project => {
            out.push(b'{');
            let mut first = true;
            for (i, key) in case.keys.iter().enumerate() {
                member(out, key, &mut first);
                number(out, row.metrics[i] + row.metrics[i])
            }
            member(out, "id", &mut first);
            string(out, &row.id);
            member(out, "name", &mut first);
            string(out, &row.name.to_uppercase());
            if let Some(label) = labels.get(row.id.as_ref()) {
                member(out, "label", &mut first);
                string(out, label)
            }
            member(out, "region", &mut first);
            string(out, &row.region);
            out.push(b'}');
        }
        _ => unreachable!(),
    }
}
pub fn run(
    case: &Case,
    input: &[u8],
    labels: &std::collections::HashMap<String, String>,
    out: &mut Vec<u8>,
    validate: bool,
) {
    if validate {
        jx::validate(input).unwrap();
    }
    if case.kind == Kind::Validate {
        if !validate {
            let _: IgnoredAny = serde_json::from_slice(input).unwrap();
        }
        return;
    }
    if case.rows > 0 || matches!(case.kind, Kind::Fold | Kind::Filtered | Kind::Callback) {
        let mut sum = 0.;
        let mut count = 0;
        each(input, case.kind == Kind::Callback, |row| {
            if case.kind == Kind::Fold || row.metrics[0] > 0. {
                if case.kind == Kind::Callback {
                    if count == 0 {
                        out.push(b'[')
                    } else {
                        out.push(b',')
                    }
                    out.extend_from_slice(b"{\"id\":");
                    string(out, &row.id);
                    out.extend_from_slice(b",\"x\":");
                    number(out, row.metrics[0] * row.metrics[1] + row.metrics[0]);
                    out.push(b'}');
                    count += 1;
                } else {
                    sum += row.metrics[0] * row.metrics[1] + row.metrics[0];
                    count += 1;
                }
            }
        });
        if case.kind == Kind::Callback {
            if count == 1 {
                out.remove(0);
            } else if count > 1 {
                out.push(b']')
            }
        } else if count > 0 {
            number(out, sum)
        }
    } else {
        let row = parse(
            input,
            case.width.max(2),
            matches!(case.kind, Kind::Lookup | Kind::Predicate | Kind::Project),
        );
        render(case, &row, labels, out)
    }
}
