//! Independent handwritten numeric references, specialized to the documented data schema.
//! The streaming and Vec variants perform identical successful work; neither is a JSONata engine.
use crate::{
    fixtures::{Case, Kind},
    handwritten,
};
use serde::{
    Deserialize,
    de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::{collections::HashMap, fmt};
#[derive(Deserialize)]
struct Metrics {
    v0: f64,
    v1: f64,
}
#[derive(Deserialize)]
struct Row {
    metrics: Metrics,
}
#[derive(Deserialize)]
struct Document {
    items: Vec<Row>,
}
struct Array<'a, F>(&'a mut F);
impl<'de, F: FnMut(f64, f64)> DeserializeSeed<'de> for Array<'_, F> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de, F: FnMut(f64, f64)> Visitor<'de> for Array<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("numeric rows")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<(), A::Error> {
        while let Some(row) = a.next_element::<Row>()? {
            (self.0)(row.metrics.v0, row.metrics.v1)
        }
        Ok(())
    }
}
struct Root<'a, F>(&'a mut F);
impl<'de, F: FnMut(f64, f64)> Visitor<'de> for Root<'_, F> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("root object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<(), A::Error> {
        #[derive(Deserialize)]
        #[serde(field_identifier)]
        enum Key {
            #[serde(rename = "items")]
            Items,
            #[serde(other)]
            Other,
        }
        while let Some(key) = m.next_key::<Key>()? {
            match key {
                Key::Items => m.next_value_seed(Array(self.0))?,
                Key::Other => {
                    m.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
pub fn run(
    case: &Case,
    input: &[u8],
    labels: &HashMap<String, String>,
    out: &mut Vec<u8>,
    validate: bool,
    materialize: bool,
) {
    if case.kind == Kind::Scalar {
        if validate {
            jx::validate(input).unwrap();
        }
        let row: Row = serde_json::from_slice(input).unwrap();
        handwritten::number(out, row.metrics.v0 * row.metrics.v1 + row.metrics.v0);
        return;
    }
    if !matches!(case.kind, Kind::Fold | Kind::Filtered) {
        return handwritten::run(case, input, labels, out, validate);
    }
    if validate {
        jx::validate(input).unwrap();
    }
    let mut sum = 0.;
    let mut count = 0;
    let mut push = |a: f64, b: f64| {
        if case.kind == Kind::Fold || a > 0. {
            sum += a * b + a;
            count += 1;
        }
    };
    if materialize {
        let document: Document = serde_json::from_slice(input).unwrap();
        for row in document.items {
            push(row.metrics.v0, row.metrics.v1)
        }
    } else {
        let mut d = serde_json::Deserializer::from_slice(input);
        de::Deserializer::deserialize_map(&mut d, Root(&mut push)).unwrap();
        d.end().unwrap();
    }
    if count > 0 {
        handwritten::number(out, sum)
    }
}

pub fn validate_raw(input: &[u8]) {
    std::hint::black_box(serde_json::from_slice::<&serde_json::value::RawValue>(input).unwrap());
}
