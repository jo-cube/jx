use std::{ffi::OsString, path::PathBuf};

pub const HELP: &str = "Usage: jx [OPTIONS] [--] EXPRESSION [FILE ...]
       jx [OPTIONS] -f EXPRESSION_FILE [OPTIONS] [FILE ...]

Compile once and stream JSON (NDJSON, pretty-printed, or whitespace-separated).
No files, or '-', reads stdin. Missing emits no output; sequences emit one
value per item; arrays stay one value. Use -- before expressions or files
starting with '-'.

Options:
  -f, --expression-file PATH  Read UTF-8 JSONata source from a file
      --vars OBJECT           Strict JSON object bound as $vars
      --vars-file PATH        Read strict JSON $vars object from a UTF-8 file
  -r, --raw-output            Emit string results unquoted (raw UTF-8)
  -p, --pretty                Pretty-print JSON output with 2-space indentation
  -n, --null-input            Evaluate once with absent input instead of reading input
  -s, --slurp                 Read all input JSON values into a single array
      --check                 Validate options, $vars, and expression, then exit
      --on-error MODE         Record error policy: fail (default) or skip
      --max-record-bytes N    Input JSON value limit in bytes (default 1048576)
      --max-output-bytes N    Per-result output limit in bytes (default 16777216)
      --max-work N            Enable cooperative limits with N work checkpoints
  -h, --help                  Print help
      --version               Print version

Examples:
  jx 'customer.id' records.ndjson
  jx -r 'database.host' config.json
  jx -s '$sum($.amount)' orders.ndjson
  jx -n --vars '{\"n\":6}' '$vars.n * 7'

A failed result publishes no partial output; earlier complete outputs remain.
Exit codes: 0 success/broken pipe, 1 record or I/O error, 2 usage/compile error.
";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorPolicy {
    Fail,
    Skip,
}

pub struct Args {
    pub expression: String,
    pub files: Vec<PathBuf>,
    pub vars: Option<String>,
    pub vars_file: Option<PathBuf>,
    pub raw_output: bool,
    pub pretty: bool,
    pub null_input: bool,
    pub slurp: bool,
    pub check: bool,
    pub on_error: ErrorPolicy,
    pub max_record_bytes: usize,
    pub max_output_bytes: usize,
    pub max_work: Option<usize>,
    pub version: bool,
}

impl Args {
    pub fn parse() -> Result<Option<Self>, String> {
        Self::parse_from(std::env::args_os().skip(1))
    }

    pub fn parse_from(mut args: impl Iterator<Item = OsString>) -> Result<Option<Self>, String> {
        let mut max_record_bytes = 1024 * 1024;
        let mut max_output_bytes = 16 * 1024 * 1024;
        let mut max_work = None;
        let mut expression_file: Option<PathBuf> = None;
        let mut expression: Option<String> = None;
        let mut vars: Option<String> = None;
        let mut vars_file: Option<PathBuf> = None;
        let mut raw_output = false;
        let mut pretty = false;
        let mut null_input = false;
        let mut slurp = false;
        let mut check = false;
        let mut on_error = ErrorPolicy::Fail;
        let mut files = Vec::new();
        let mut options = true;

        while let Some(arg) = args.next() {
            if options && (arg == "--help" || arg == "-h") {
                return Ok(None);
            }
            if options && arg == "--version" {
                return Ok(Some(Self {
                    expression: String::new(),
                    files: Vec::new(),
                    vars: None,
                    vars_file: None,
                    raw_output: false,
                    pretty: false,
                    null_input: false,
                    slurp: false,
                    check: false,
                    on_error: ErrorPolicy::Fail,
                    max_record_bytes,
                    max_output_bytes,
                    max_work,
                    version: true,
                }));
            }
            if options && arg == "--" {
                options = false;
                continue;
            }
            if options && (arg == "--expression-file" || arg == "-f") {
                if expression_file.is_some() {
                    return Err("--expression-file may only be specified once".to_owned());
                }
                let path = args.next().ok_or("--expression-file requires a path")?;
                expression_file = Some(PathBuf::from(path));
                continue;
            }
            if options && arg == "--vars" {
                if vars.is_some() {
                    return Err("--vars may only be specified once".to_owned());
                }
                let value = args
                    .next()
                    .ok_or("--vars requires a JSON object")?
                    .into_string()
                    .map_err(|_| "--vars must be UTF-8".to_owned())?;
                vars = Some(value);
                continue;
            }
            if options && arg == "--vars-file" {
                if vars_file.is_some() {
                    return Err("--vars-file may only be specified once".to_owned());
                }
                let path = args.next().ok_or("--vars-file requires a path")?;
                vars_file = Some(PathBuf::from(path));
                continue;
            }
            if options && (arg == "-r" || arg == "--raw-output") {
                raw_output = true;
                continue;
            }
            if options && (arg == "-p" || arg == "--pretty") {
                pretty = true;
                continue;
            }
            if options && (arg == "-n" || arg == "--null-input") {
                null_input = true;
                continue;
            }
            if options && (arg == "-s" || arg == "--slurp") {
                slurp = true;
                continue;
            }
            if options && arg == "--check" {
                check = true;
                continue;
            }
            if options && arg == "--on-error" {
                let value = args
                    .next()
                    .and_then(|v| v.into_string().ok())
                    .ok_or("--on-error requires 'fail' or 'skip'")?;
                on_error = match value.as_str() {
                    "fail" => ErrorPolicy::Fail,
                    "skip" => ErrorPolicy::Skip,
                    other => {
                        return Err(format!(
                            "unknown --on-error policy: {other} (expected 'fail' or 'skip')"
                        ));
                    }
                };
                continue;
            }
            if options && (arg == "--max-output-bytes" || arg == "--max-work") {
                let n = parse_positive_usize(args.next(), &arg.to_string_lossy())?;
                if arg == "--max-output-bytes" {
                    max_output_bytes = n;
                } else {
                    max_work = Some(n);
                }
                continue;
            }
            if options && arg == "--max-record-bytes" {
                max_record_bytes = parse_positive_usize(args.next(), "--max-record-bytes")?;
                continue;
            }
            if options && let Some(short) = arg.to_str().and_then(|s| s.strip_prefix('-')) {
                if short.len() > 1
                    && !short.starts_with('-')
                    && short.chars().all(|c| matches!(c, 'r' | 'p' | 'n' | 's'))
                {
                    for flag in short.chars() {
                        match flag {
                            'r' => raw_output = true,
                            'p' => pretty = true,
                            'n' => null_input = true,
                            's' => slurp = true,
                            _ => unreachable!(),
                        }
                    }
                    continue;
                }
                if !short.is_empty() {
                    return Err(format!("unknown option: -{short}"));
                }
            } else if options && arg.to_string_lossy().starts_with('-') && arg != "-" {
                return Err(format!("unknown option: {}", arg.to_string_lossy()));
            }

            // First positional argument: expression unless -f was already provided.
            if expression_file.is_none() && expression.is_none() {
                expression = Some(
                    arg.into_string()
                        .map_err(|_| "expression must be UTF-8".to_owned())?,
                );
            } else {
                files.push(PathBuf::from(arg));
            }
            files.extend(args.map(PathBuf::from));
            break;
        }

        if vars.is_some() && vars_file.is_some() {
            return Err("--vars and --vars-file cannot be combined".to_owned());
        }
        if null_input && slurp {
            return Err("--null-input and --slurp cannot be combined".to_owned());
        }
        if null_input && !files.is_empty() {
            return Err("--null-input cannot be combined with input files".to_owned());
        }

        let expression = match (expression_file, expression) {
            (Some(path), None) => std::fs::read_to_string(&path)
                .map_err(|e| format!("{}: {e}", path.to_string_lossy()))?,
            (None, Some(expr)) => expr,
            (Some(_), Some(_)) => {
                unreachable!("positional expression is treated as file when -f is set")
            }
            (None, None) => return Err(HELP.to_owned()),
        };

        Ok(Some(Self {
            expression,
            files,
            vars,
            vars_file,
            raw_output,
            pretty,
            null_input,
            slurp,
            check,
            on_error,
            max_record_bytes,
            max_output_bytes,
            max_work,
            version: false,
        }))
    }
}

fn parse_positive_usize(value: Option<OsString>, flag: &str) -> Result<usize, String> {
    value
        .and_then(|n| n.into_string().ok())
        .and_then(|n| n.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .ok_or_else(|| format!("{flag} requires a positive integer"))
}
