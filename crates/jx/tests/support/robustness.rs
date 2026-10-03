use jx::{Error, EvaluationOptions, Limits};
use std::io::{self, Write};

pub const EXPRESSIONS: &[&str] = &[
    "$",
    "a",
    "a.b",
    "a*a+b*b+a",
    "flag ? a*a+b : b*b+a",
    "$sum(rows[n>0].(n*n+n+1))",
    "$map(rows,function($r){$r.n&'!'})",
    "{'value':a,'rows':rows[n>0].n[]}",
    "$sort(rows,function($a,$b){$a.n>$b.n}).n",
    "($f:=$eval('function($x){function(){$x}}');$g:=$f(a);$g())",
    "$toMillis(date,picture)",
    "$formatInteger(a,picture)",
    "$formatNumber(a,picture)",
    "($f:=$eval('function(){' & $string({'value':a}) & '}');$f())",
    r#"($f:=$eval('function($r){{"n":$r.n+1,"keep":' & $string({'value':a}) & '}}');$map(rows,$f))"#,
];

pub fn snapshot(expression: &jx::Expression, input: &[u8]) -> (Vec<Vec<u8>>, Option<Error>) {
    let mut output = Vec::new();
    let result = expression.evaluate(input).and_then(|values| {
        values.for_each(|value| {
            let mut bytes = Vec::new();
            if value.write_compact(&mut bytes).is_ok() {
                jx::validate(&bytes).expect("successful serialization must produce valid JSON");
                output.push(bytes);
            }
        })
    });
    (output, result.err())
}

struct FailingWriter(usize);
impl Write for FailingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0 == 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let count = bytes.len().min(self.0);
        self.0 -= count;
        Ok(count)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// Shared by deterministic mutation tests and the optional coverage-guided target.
// Arbitrary expressions are compiled only: non-cooperative regex/host work cannot
// safely be bounded by a cooperative runtime deadline. Evaluation uses fixed,
// bounded templates with arbitrary JSON, including dynamic-code retention.
pub fn exercise(bytes: &[u8]) {
    let source = std::str::from_utf8(bytes.get(..bytes.len().min(256)).unwrap());
    if let Ok(source) = source {
        let _ = jx::compile(source);
    }
    let bytes = &bytes[..bytes.len().min(16 * 1024)];
    let expected = jx::validate(bytes).err();
    let selector = bytes.iter().fold(0usize, |hash, &byte| {
        hash.wrapping_mul(31).wrapping_add(byte as usize)
    }) % EXPRESSIONS.len();
    let expression = jx::compile(EXPRESSIONS[selector]).unwrap();
    let options = EvaluationOptions {
        limits: Some(Limits {
            max_work: 2048,
            max_items: 2048,
            max_output_bytes: 64 * 1024,
            ..Default::default()
        }),
        ..Default::default()
    };
    // Small fixed-template inputs exercise optimized paths that controls bypass,
    // while bounding constructor expansion separately from cooperative quotas.
    if expected.is_none() && bytes.len() <= 1024 {
        let _ = snapshot(&expression, bytes);
    }
    let evaluated = expression.evaluate_with(Some(bytes), options);
    if let Some(expected) = expected {
        assert_eq!(evaluated.unwrap_err(), expected);
    } else if let Ok(values) = evaluated {
        let _ = values.for_each(|value| {
            let mut encoded = Vec::new();
            if value.write_compact(&mut encoded).is_ok() {
                jx::validate(&encoded).unwrap();
                let boundary = bytes.len() % (encoded.len() + 1);
                if boundary < encoded.len() {
                    assert!(value.write_compact(&mut FailingWriter(boundary)).is_err());
                }
                if let Ok(owned) = value.to_owned() {
                    let mut detached = Vec::new();
                    owned.as_value().write_compact(&mut detached).unwrap();
                    jx::validate(&detached).unwrap();
                }
            }
        });
    }
}
