//! Compile once and consume an NDJSON stream without collecting results.
use std::io::{self, BufRead, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args().nth(1).unwrap_or_else(|| "$".into());
    let expression = jx::compile(&source)?;
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut output = io::BufWriter::new(io::stdout().lock());
    let mut record = Vec::new();
    let mut encoded = Vec::new();
    // This small embedding example expects trusted, bounded input. The CLI also
    // enforces byte limits while reading an unterminated record.
    loop {
        record.clear();
        if input.read_until(b'\n', &mut record)? == 0 {
            break;
        }
        if record.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        expression.evaluate(&record)?.try_for_each(|value| {
            encoded.clear();
            value.write_compact(&mut encoded)?;
            encoded.push(b'\n');
            output.write_all(&encoded)
        })?;
    }
    output.flush()?;
    Ok(())
}
