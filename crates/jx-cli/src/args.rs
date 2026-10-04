use std::{ffi::OsString, path::PathBuf};

pub const HELP: &str = "Usage: jx [OPTIONS] [--] EXPRESSION [FILE ...]
       jx [OPTIONS] -f EXPRESSION_FILE [FILE ...]

Compile once and stream NDJSON. No files, or '-', reads stdin.
Missing emits no line; sequences emit one line per item; arrays stay one value.
Options precede the expression. Use -- before expressions beginning with '-'.

Options:
  -f, --expression-file PATH  Read UTF-8 JSONata source from a file
      --max-record-bytes N   Input limit excluding LF (default 1048576)
      --max-output-bytes N   Per-result limit excluding LF (default 16777216)
      --max-work N           Enable cooperative limits with N work checkpoints
  -h, --help                 Print help
      --version              Print version

Blank lines are ignored; CRLF and final lines without LF are accepted.
A failed result publishes no partial line; earlier complete lines remain.
Exit codes: 0 success/broken pipe, 1 record or I/O error, 2 usage/compile error.
";

pub struct Args {
    pub expression: String,
    pub files: Vec<PathBuf>,
    pub max_record_bytes: usize,
    pub max_output_bytes: usize,
    pub max_work: Option<usize>,
    pub version: bool,
    #[cfg(feature = "jit")]
    pub jit: bool,
}

impl Args {
    pub fn parse() -> Result<Option<Self>, String> {
        let mut args = std::env::args_os().skip(1);
        let mut max_record_bytes = 1024 * 1024;
        let mut options = true;
        let mut max_output_bytes = 16 * 1024 * 1024;
        let mut max_work = None;
        let mut version = false;
        #[cfg(feature = "jit")]
        let mut jit = false;
        let expression = loop {
            let arg = args.next().ok_or_else(|| HELP.to_owned())?;
            if options && (arg == "--help" || arg == "-h") {
                return Ok(None);
            }
            if options && arg == "--version" {
                version = true;
                break String::new();
            }
            if options && (arg == "--expression-file" || arg == "-f") {
                let path = args.next().ok_or("--expression-file requires a path")?;
                break std::fs::read_to_string(&path)
                    .map_err(|e| format!("{}: {e}", path.to_string_lossy()))?;
            }
            if options && (arg == "--max-output-bytes" || arg == "--max-work") {
                let n = args
                    .next()
                    .and_then(|n| n.into_string().ok())
                    .and_then(|n| n.parse::<usize>().ok())
                    .filter(|&n| n > 0)
                    .ok_or_else(|| {
                        format!("{} requires a positive integer", arg.to_string_lossy())
                    })?;
                if arg == "--max-output-bytes" {
                    max_output_bytes = n;
                } else {
                    max_work = Some(n);
                }
                continue;
            }
            if options && arg == "--" {
                options = false;
                continue;
            }
            #[cfg(feature = "jit")]
            if options && arg == "--jit" {
                jit = true;
                continue;
            }
            if options && arg == "--max-record-bytes" {
                max_record_bytes = args
                    .next()
                    .and_then(|n| n.into_string().ok())
                    .and_then(|n| n.parse::<usize>().ok())
                    .filter(|&n| n > 0)
                    .ok_or("--max-record-bytes requires a positive integer")?;
                continue;
            }
            if options && arg.to_string_lossy().starts_with('-') {
                return Err(format!("unknown option: {}", arg.to_string_lossy()));
            }
            break arg
                .into_string()
                .map_err(|_: OsString| "expression must be UTF-8")?;
        };
        Ok(Some(Self {
            expression,
            files: args.map(PathBuf::from).collect(),
            max_record_bytes,
            max_output_bytes,
            max_work,
            version,
            #[cfg(feature = "jit")]
            jit,
        }))
    }
}
