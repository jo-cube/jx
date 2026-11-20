#![forbid(unsafe_code)]
mod args;
mod stream;

use std::{
    fs::{self, File},
    io::{self, BufReader, BufWriter, Write},
    process::ExitCode,
};

const IO_BUFFER_BYTES: usize = 64 * 1024;

fn main() -> ExitCode {
    let args = match args::Args::parse() {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{}", args::HELP);
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("jx: {error}");
            return ExitCode::from(2);
        }
    };
    if args.version {
        println!("jx {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let options = match compile_options(&args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("jx: {error}");
            return ExitCode::from(2);
        }
    };
    let expression = match options.compile(&args.expression) {
        Ok(expression) => expression,
        Err(error) => {
            let location = error
                .location(&args.expression)
                .map(|(line, column)| format!("{line}:{column}"))
                .unwrap_or_else(|| format!("byte {}", error.offset));
            eprintln!("jx: expression:{location}: {:?}: {error}", error.phase);
            return ExitCode::from(2);
        }
    };
    if args.check {
        return ExitCode::SUCCESS;
    }
    match run(args, &expression) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("jx: {error}");
            ExitCode::from(1)
        }
    }
}

fn compile_options(args: &args::Args) -> Result<jx::CompileOptions, String> {
    let raw_vars = if let Some(path) = &args.vars_file {
        Some(
            fs::read_to_string(path)
                .map_err(|error| format!("cannot read --vars-file {}: {error}", path.display()))?,
        )
    } else {
        args.vars.clone()
    };
    let Some(source) = raw_vars else {
        return Ok(jx::CompileOptions::default());
    };
    let value = jx::OwnedValue::from_json(source.as_bytes())
        .map_err(|error| format!("$vars input must be a valid JSON object: {error}"))?;
    if value.as_value().value_type() != jx::ValueType::Object {
        return Err("$vars input must be a JSON object".to_owned());
    }
    Ok(jx::CompileOptions::default().constant_binding("vars", value))
}

fn run(args: args::Args, expression: &jx::Expression) -> io::Result<()> {
    let stdout = io::stdout();
    let mut output = BufWriter::with_capacity(IO_BUFFER_BYTES, stdout.lock());
    let mut buffer = stream::Buffers::default();
    let config = stream::Config {
        record_bytes: args.max_record_bytes,
        output_bytes: args.max_output_bytes,
        work: args.max_work,
        raw_output: args.raw_output,
        pretty: args.pretty,
        on_error: args.on_error,
    };
    if args.null_input {
        let result =
            stream::evaluate_and_write(None, &mut output, expression, &mut buffer, &config, "");
        let flushed = output.flush();
        return result.and(flushed);
    }

    let stdin = io::stdin();
    let mut stdin = BufReader::with_capacity(IO_BUFFER_BYTES, stdin.lock());
    let files = if args.files.is_empty() {
        vec!["-".into()]
    } else {
        args.files
    };

    if args.slurp {
        let mut slurped = Vec::new();
        slurped.push(b'[');
        let mut scratch = Vec::new();
        let mut count = 0usize;
        let read_result = files.into_iter().try_for_each(|path| {
            let name = path.display().to_string();
            let step = if path.as_os_str() == "-" {
                stream::slurp_reader(
                    &mut stdin,
                    &mut slurped,
                    &mut scratch,
                    &mut count,
                    &config,
                    &name,
                )
            } else {
                File::open(&path).and_then(|file| {
                    stream::slurp_reader(
                        BufReader::with_capacity(IO_BUFFER_BYTES, file),
                        &mut slurped,
                        &mut scratch,
                        &mut count,
                        &config,
                        &name,
                    )
                })
            };
            step.map_err(|error| io::Error::new(error.kind(), format!("{name}: {error}")))
        });
        read_result?;
        if slurped.len() >= config.record_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "record exceeds --max-record-bytes",
            ));
        }
        slurped.push(b']');
        let result = stream::evaluate_and_write(
            Some(&slurped),
            &mut output,
            expression,
            &mut buffer,
            &config,
            "",
        );
        let flushed = output.flush();
        return result.and(flushed);
    }

    let result = files.into_iter().try_for_each(|path| {
        let name = path.display().to_string();
        let process = if path.as_os_str() == "-" {
            stream::run(
                &mut stdin,
                &mut output,
                expression,
                &mut buffer,
                &config,
                &name,
            )
        } else {
            File::open(&path).and_then(|file| {
                stream::run(
                    BufReader::with_capacity(IO_BUFFER_BYTES, file),
                    &mut output,
                    expression,
                    &mut buffer,
                    &config,
                    &name,
                )
            })
        };
        process.map_err(|error| io::Error::new(error.kind(), format!("{name}: {error}")))
    });
    // Earlier complete records remain observable even when a later record fails.
    let flushed = output.flush();
    result.and(flushed)
}
