#![forbid(unsafe_code)]
mod args;
mod stream;

use std::{
    fs::File,
    io::{self, BufReader, BufWriter, Write},
    process::ExitCode,
};

fn main() -> ExitCode {
    let args = match args::Args::parse() {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{}", args::HELP);
            #[cfg(feature = "jit")]
            println!(
                "--jit enables bounded native numeric plans; other regions keep the interpreter."
            );
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("jx: {error}");
            return ExitCode::from(2);
        }
    };
    let expression = match jx::compile(&args.expression) {
        Ok(expression) => expression,
        Err(error) => {
            eprintln!("jx: expression: {error}");
            return ExitCode::from(2);
        }
    };
    #[cfg(feature = "jit")]
    let expression = if args.jit {
        let mut expression = expression;
        expression.enable_native();
        expression
    } else {
        expression
    };
    match run(args, &expression) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("jx: {error}");
            ExitCode::from(1)
        }
    }
}

fn run(args: args::Args, expression: &jx::Expression) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    let stdout = io::stdout();
    let mut output = BufWriter::new(stdout.lock());
    let mut buffer = Vec::new();
    let files = if args.files.is_empty() {
        vec!["-".into()]
    } else {
        args.files
    };
    let result = files.into_iter().try_for_each(|path| {
        let process = if path.as_os_str() == "-" {
            stream::run(
                &mut stdin,
                &mut output,
                expression,
                &mut buffer,
                args.max_record_bytes,
            )
        } else {
            File::open(&path).and_then(|file| {
                stream::run(
                    BufReader::new(file),
                    &mut output,
                    expression,
                    &mut buffer,
                    args.max_record_bytes,
                )
            })
        };
        process
            .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))
    });
    // Earlier complete records remain observable even when a later record fails.
    let flushed = output.flush();
    result.and(flushed)
}
