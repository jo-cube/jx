use std::io::{self, BufRead, Write};

/// Read no more than limit bytes, even if an input has no newline. Vec capacity
/// grows only with accepted records and is reused across records and files.
fn record(reader: &mut impl BufRead, buffer: &mut Vec<u8>, limit: usize) -> io::Result<bool> {
    buffer.clear();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(!buffer.is_empty());
        }
        let newline = available.iter().position(|&b| b == b'\n');
        let count = newline.unwrap_or(available.len());
        if count > limit - buffer.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "record exceeds --max-record-bytes",
            ));
        }
        buffer.extend_from_slice(&available[..count]);
        reader.consume(count + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(true);
        }
    }
}

pub fn run(
    mut reader: impl BufRead,
    output: &mut impl Write,
    expression: &jx::Expression,
    buffer: &mut Vec<u8>,
    limit: usize,
) -> io::Result<()> {
    let mut line = 1;
    loop {
        let present = record(&mut reader, buffer, limit)
            .map_err(|error| io::Error::new(error.kind(), format!("line {line}: {error}")))?;
        if !present {
            return Ok(());
        }
        if !buffer.iter().all(|b| matches!(b, b' ' | b'\t' | b'\r')) {
            let values = expression.evaluate(buffer).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidData, format!("line {line}: {error}"))
            })?;
            values.try_for_each(|value| {
                value.write_compact(&mut *output)?;
                output.write_all(b"\n")
            })?;
        }
        line += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn framing_is_independent_of_buffer_boundaries() {
        let input = b"{\"a\":1}\r\n\n{\"a\":null}\n{\"a\":[2,3]}";
        for capacity in 1..=input.len() {
            let reader = BufReader::with_capacity(capacity, Cursor::new(input));
            let mut output = Vec::new();
            run(
                reader,
                &mut output,
                &jx::compile("a").unwrap(),
                &mut Vec::new(),
                32,
            )
            .unwrap();
            assert_eq!(output, b"1\nnull\n[2,3]\n");
        }
    }

    #[test]
    fn oversized_unterminated_record_stays_bounded() {
        let input = vec![b' '; 100_000];
        let mut reader = BufReader::with_capacity(3, Cursor::new(input));
        let mut buffer = Vec::new();
        let error = run(
            &mut reader,
            &mut Vec::new(),
            &jx::compile("$").unwrap(),
            &mut buffer,
            8,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(buffer.len() <= 8);
        assert!(
            reader.get_ref().position() <= 12,
            "stop reading at the bound, not at EOF"
        );
    }

    #[test]
    fn output_failure_stops_before_consuming_the_next_record() {
        struct Fails;
        impl Write for Fails {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut input = Cursor::new(b"1\n2\n3\n");
        let error = run(
            &mut input,
            &mut Fails,
            &jx::compile("$").unwrap(),
            &mut Vec::new(),
            16,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(input.position(), 2);
    }
}
