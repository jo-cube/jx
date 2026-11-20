use std::io::{self, BufRead, Write};

use crate::args::ErrorPolicy;

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

fn push_bounded(buffer: &mut Vec<u8>, byte: u8, limit: usize) -> io::Result<()> {
    if buffer.len() >= limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "record exceeds --max-record-bytes",
        ));
    }
    buffer.push(byte);
    Ok(())
}

/// Read one complete JSON value (NDJSON, pretty-printed multi-line, or
/// concatenated/whitespace-separated) into `buffer` without exceeding `limit`
/// bytes. Returns the 1-based starting line number, or `Ok(None)` at EOF.
pub fn next_value(
    reader: &mut impl BufRead,
    buffer: &mut Vec<u8>,
    limit: usize,
    line: &mut usize,
) -> io::Result<Option<usize>> {
    buffer.clear();
    // Skip leading JSON whitespace while bounding any single non-newline run so
    // pathological whitespace-only inputs cannot stream unboundedly.
    let mut gap_bytes = 0usize;
    let first = loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(None);
        }
        let mut consumed = 0;
        let mut found = None;
        for &byte in available {
            if is_ws(byte) {
                consumed += 1;
                if byte == b'\n' {
                    *line += 1;
                    gap_bytes = 0;
                } else {
                    gap_bytes += 1;
                    if gap_bytes > limit {
                        reader.consume(consumed);
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "record exceeds --max-record-bytes",
                        ));
                    }
                }
            } else {
                found = Some(byte);
                break;
            }
        }
        reader.consume(consumed);
        if let Some(byte) = found {
            break byte;
        }
    };

    let start_line = *line;
    match first {
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut in_string = false;
            let mut escaped = false;
            loop {
                let available = reader.fill_buf()?;
                if available.is_empty() {
                    return Ok(Some(start_line));
                }
                let mut consumed = 0;
                let mut done = false;
                for &byte in available {
                    if buffer.len() >= limit {
                        reader.consume(consumed);
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "record exceeds --max-record-bytes",
                        ));
                    }
                    buffer.push(byte);
                    consumed += 1;
                    if byte == b'\n' {
                        *line += 1;
                    }
                    if in_string {
                        if escaped {
                            escaped = false;
                        } else if byte == b'\\' {
                            escaped = true;
                        } else if byte == b'"' {
                            in_string = false;
                        }
                    } else {
                        match byte {
                            b'"' => in_string = true,
                            b'{' | b'[' => depth += 1,
                            b'}' | b']' => {
                                depth -= 1;
                                if depth == 0 {
                                    done = true;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                reader.consume(consumed);
                if done {
                    return Ok(Some(start_line));
                }
            }
        }
        b'"' => {
            let mut started = false;
            let mut escaped = false;
            loop {
                let available = reader.fill_buf()?;
                if available.is_empty() {
                    return Ok(Some(start_line));
                }
                let mut consumed = 0;
                let mut done = false;
                for &byte in available {
                    if buffer.len() >= limit {
                        reader.consume(consumed);
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "record exceeds --max-record-bytes",
                        ));
                    }
                    buffer.push(byte);
                    consumed += 1;
                    if byte == b'\n' {
                        *line += 1;
                    }
                    if !started {
                        started = true;
                    } else if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        done = true;
                        break;
                    }
                }
                reader.consume(consumed);
                if done {
                    return Ok(Some(start_line));
                }
            }
        }
        _ => {
            // Primitive (number, true, false, null) or unexpected leading token.
            loop {
                let available = reader.fill_buf()?;
                if available.is_empty() {
                    return Ok(Some(start_line));
                }
                let mut consumed = 0;
                let mut done = false;
                for &byte in available {
                    if !buffer.is_empty()
                        && (is_ws(byte)
                            || matches!(byte, b'{' | b'[' | b'"' | b'}' | b']' | b',' | b':'))
                    {
                        done = true;
                        break;
                    }
                    push_bounded(buffer, byte, limit)?;
                    consumed += 1;
                    if matches!(byte, b'}' | b']' | b',' | b':') {
                        done = true;
                        break;
                    }
                }
                reader.consume(consumed);
                if done {
                    return Ok(Some(start_line));
                }
            }
        }
    }
}

#[derive(Default)]
pub struct Buffers {
    pub input: Vec<u8>,
    compact: Vec<u8>,
    output: Vec<u8>,
    workspace: jx::execution::Workspace,
}

pub struct Config {
    pub record_bytes: usize,
    pub output_bytes: usize,
    pub work: Option<usize>,
    pub raw_output: bool,
    pub pretty: bool,
    pub on_error: ErrorPolicy,
}

struct Line<'a> {
    bytes: &'a mut Vec<u8>,
    limit: usize,
}

impl Write for Line<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "result exceeds --max-output-bytes",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn eval_options<'e, 'i>(work: Option<usize>) -> jx::EvaluationOptions<'e, 'i> {
    match work {
        Some(max_work) => jx::EvaluationOptions {
            limits: Some(jx::Limits {
                max_work,
                ..Default::default()
            }),
            ..Default::default()
        },
        None => jx::EvaluationOptions::default(),
    }
}

fn format_pretty(compact: &[u8], out: &mut Line<'_>) -> io::Result<()> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < compact.len() {
        let byte = compact[i];
        if in_string {
            let start = i;
            while i < compact.len() {
                let b = compact[i];
                i += 1;
                if escaped {
                    escaped = false;
                } else if b == b'\\' {
                    escaped = true;
                } else if b == b'"' {
                    in_string = false;
                    break;
                }
            }
            out.write_all(&compact[start..i])?;
            continue;
        }
        match byte {
            b'"' => {
                in_string = true;
                out.write_all(b"\"")?;
                i += 1;
            }
            b'{' | b'[' => {
                let close = if byte == b'{' { b'}' } else { b']' };
                if compact.get(i + 1) == Some(&close) {
                    out.write_all(&compact[i..i + 2])?;
                    i += 2;
                } else {
                    depth += 1;
                    out.write_all(&[byte, b'\n'])?;
                    write_indent(out, depth)?;
                    i += 1;
                }
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                out.write_all(b"\n")?;
                write_indent(out, depth)?;
                out.write_all(&[byte])?;
                i += 1;
            }
            b',' => {
                out.write_all(b",\n")?;
                write_indent(out, depth)?;
                i += 1;
            }
            b':' => {
                out.write_all(b": ")?;
                i += 1;
            }
            _ => {
                let start = i;
                i += 1;
                while i < compact.len()
                    && !matches!(compact[i], b'"' | b'{' | b'[' | b'}' | b']' | b',' | b':')
                {
                    i += 1;
                }
                out.write_all(&compact[start..i])?;
            }
        }
    }
    Ok(())
}

fn write_indent(out: &mut Line<'_>, depth: usize) -> io::Result<()> {
    const SPACES: &[u8] = b"                                ";
    let mut remaining = depth.saturating_mul(2);
    while remaining > 0 {
        let chunk = remaining.min(SPACES.len());
        out.write_all(&SPACES[..chunk])?;
        remaining -= chunk;
    }
    Ok(())
}

fn emit_value(
    value: &jx::Value<'_, '_>,
    compact: &mut Vec<u8>,
    output_buf: &mut Vec<u8>,
    output: &mut impl Write,
    config: &Config,
    context: &str,
) -> io::Result<()> {
    output_buf.clear();
    let mut line_writer = Line {
        bytes: output_buf,
        limit: config.output_bytes,
    };
    if config.raw_output {
        match value.as_str() {
            Ok(Some(text)) => {
                line_writer.write_all(text.as_bytes()).map_err(|e| {
                    io::Error::new(e.kind(), format!("{context}serialization: {e}"))
                })?;
                output_buf.push(b'\n');
                return output.write_all(output_buf);
            }
            Ok(None) => {}
            Err(error) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{context}serialization: {error}"),
                ));
            }
        }
    }
    if config.pretty {
        compact.clear();
        value
            .write_compact(Line {
                bytes: compact,
                limit: config.output_bytes,
            })
            .map_err(|e| io::Error::new(e.kind(), format!("{context}serialization: {e}")))?;
        format_pretty(compact, &mut line_writer)
            .map_err(|e| io::Error::new(e.kind(), format!("{context}serialization: {e}")))?;
    } else {
        value
            .write_compact(line_writer)
            .map_err(|e| io::Error::new(e.kind(), format!("{context}serialization: {e}")))?;
    }
    output_buf.push(b'\n');
    output.write_all(output_buf)
}

pub fn evaluate_and_write(
    input: Option<&[u8]>,
    output: &mut impl Write,
    expression: &jx::Expression,
    buffers: &mut Buffers,
    config: &Config,
    context: &str,
) -> io::Result<()> {
    let values = expression
        .evaluate_with_workspace(input, eval_options(config.work), &mut buffers.workspace)
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{context}{:?}: {error}", error.phase),
            )
        })?;
    values
        .try_for_each(|value| {
            emit_value(
                &value,
                &mut buffers.compact,
                &mut buffers.output,
                output,
                config,
                context,
            )
        })
        .map_err(|error| match error {
            jx::ConsumeError::Consumer(error) => error,
            jx::ConsumeError::Evaluation(error) => io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{context}{:?}: {error}", error.phase),
            ),
        })
}

pub fn run(
    mut reader: impl BufRead,
    output: &mut impl Write,
    expression: &jx::Expression,
    buffers: &mut Buffers,
    config: &Config,
    source_name: &str,
) -> io::Result<()> {
    let mut line = 1;
    loop {
        let Some(start_line) = next_value(
            &mut reader,
            &mut buffers.input,
            config.record_bytes,
            &mut line,
        )
        .map_err(|error| io::Error::new(error.kind(), format!("line {line}: {error}")))?
        else {
            return Ok(());
        };
        let context = format!("line {start_line}: ");
        let input_slice = std::mem::take(&mut buffers.input);
        let result = evaluate_and_write(
            Some(&input_slice),
            output,
            expression,
            buffers,
            config,
            &context,
        );
        buffers.input = input_slice;
        match result {
            Ok(()) => {}
            Err(error) if config.on_error == ErrorPolicy::Skip && !is_output_fatal(&error) => {
                eprintln!("jx: {source_name}: {error}");
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn slurp_reader(
    mut reader: impl BufRead,
    slurp_buf: &mut Vec<u8>,
    scratch: &mut Vec<u8>,
    count: &mut usize,
    config: &Config,
    source_name: &str,
) -> io::Result<()> {
    let mut line = 1;
    loop {
        let Some(start_line) = next_value(&mut reader, scratch, config.record_bytes, &mut line)
            .map_err(|error| io::Error::new(error.kind(), format!("line {line}: {error}")))?
        else {
            return Ok(());
        };
        if let Err(error) = jx::validate(scratch) {
            let err = io::Error::new(
                io::ErrorKind::InvalidData,
                format!("line {start_line}: {:?}: {error}", error.phase),
            );
            if config.on_error == ErrorPolicy::Skip {
                eprintln!("jx: {source_name}: {err}");
                continue;
            }
            return Err(err);
        }
        let extra = scratch.len() + usize::from(*count > 0) + 1;
        if extra > config.record_bytes.saturating_sub(slurp_buf.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("line {start_line}: record exceeds --max-record-bytes"),
            ));
        }
        if *count > 0 {
            slurp_buf.push(b',');
        }
        slurp_buf.extend_from_slice(scratch);
        *count += 1;
    }
}

fn is_output_fatal(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::BrokenPipe
            | io::ErrorKind::PermissionDenied
            | io::ErrorKind::WriteZero
            | io::ErrorKind::Interrupted
            | io::ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    fn default_config(record_bytes: usize, output_bytes: usize) -> Config {
        Config {
            record_bytes,
            output_bytes,
            work: None,
            raw_output: false,
            pretty: false,
            on_error: ErrorPolicy::Fail,
        }
    }

    #[test]
    fn framing_is_independent_of_buffer_boundaries() {
        let input = b"{\"a\":1}\r\n\n{\"a\":null}\n{\n  \"a\": [\n    2,\n    3\n  ]\n}";
        for capacity in 1..=input.len() {
            let reader = BufReader::with_capacity(capacity, Cursor::new(input));
            let mut output = Vec::new();
            run(
                reader,
                &mut output,
                &jx::compile("a").unwrap(),
                &mut Buffers::default(),
                &default_config(64, 1024),
                "-",
            )
            .unwrap();
            assert_eq!(output, b"1\nnull\n[2,3]\n");
        }
    }

    #[test]
    fn oversized_unterminated_record_stays_bounded() {
        let input = vec![b' '; 100_000];
        let mut reader = BufReader::with_capacity(3, Cursor::new(input));
        let mut buffer = Buffers::default();
        let error = run(
            &mut reader,
            &mut Vec::new(),
            &jx::compile("$").unwrap(),
            &mut buffer,
            &default_config(8, 1024),
            "-",
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(buffer.input.len() <= 8);
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
            &mut Buffers::default(),
            &default_config(16, 1024),
            "-",
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(input.position(), 1);
    }
}
