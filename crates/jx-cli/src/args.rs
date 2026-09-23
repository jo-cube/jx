use std::{ffi::OsString, path::PathBuf};

pub const HELP: &str = "Usage: jx [--max-record-bytes N] [--] EXPRESSION [FILE ...]\n\
Compile once and process NDJSON. No files, or '-', reads stdin.\n\
Missing results produce no line. Selected arrays stay on one line.\n\
Blank lines are ignored. Default record limit: 1048576 bytes excluding LF.\n";

pub struct Args {
    pub expression: String,
    pub files: Vec<PathBuf>,
    pub max_record_bytes: usize,
}

impl Args {
    pub fn parse() -> Result<Option<Self>, String> {
        let mut args = std::env::args_os().skip(1);
        let mut max_record_bytes = 1024 * 1024;
        let mut options = true;
        let expression = loop {
            let arg = args.next().ok_or_else(|| HELP.to_owned())?;
            if options && (arg == "--help" || arg == "-h") {
                return Ok(None);
            }
            if options && arg == "--" {
                options = false;
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
        }))
    }
}
