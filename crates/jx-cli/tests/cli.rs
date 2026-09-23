use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

fn run(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_jx"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn ndjson_missing_null_arrays_and_final_unterminated_record() {
    let output = run(&["a"], b"\n {}\r\n{\"a\":null}\n{\"a\":[1, 2]}\n{\"a\":3}");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"null\n[1,2]\n3\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn errors_have_distinct_exit_codes_and_record_context() {
    let output = run(&["a[0]"], b"");
    assert_eq!(output.status.code(), Some(2));
    let output = run(&["$"], b"1\n[0,]\n2\n");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"1\n");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("-: line 2:")
    );
}

#[test]
fn exact_record_limit_including_cr_but_excluding_lf() {
    for input in [&b"1234\n"[..], &b"1234"[..]] {
        assert!(
            run(&["--max-record-bytes", "4", "$"], input)
                .status
                .success()
        );
    }
    for input in [&b"12345\n"[..], &b"12345"[..], &b"1234\r\n"[..]] {
        let output = run(&["--max-record-bytes", "4", "$"], input);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
    }
    assert_eq!(
        run(&["--max-record-bytes", "0", "$"], b"").status.code(),
        Some(2)
    );
}

#[test]
fn streams_multiple_files_and_stdin_in_order() {
    let path = std::env::temp_dir().join(format!("jx-cli-{}.ndjson", std::process::id()));
    std::fs::write(&path, b"{\"a\":1}\n{\"a\":2}").unwrap();
    let output = run(
        &["a", path.to_str().unwrap(), "-", path.to_str().unwrap()],
        b"{\"a\":3}\n",
    );
    std::fs::remove_file(path).unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"1\n2\n3\n1\n2\n");
}

#[test]
fn help_empty_input_options_and_invalid_utf8() {
    assert!(run(&["--help"], b"").status.success());
    assert!(run(&["--", "$"], b"").status.success());
    assert_eq!(run(&[], b"").status.code(), Some(2));
    assert_eq!(run(&["--unknown"], b"").status.code(), Some(2));
    assert_eq!(run(&["$"], b"\"\xff\"\n").status.code(), Some(1));
}

#[test]
fn sequences_are_separate_lines_and_raw_arrays_stay_values() {
    let input = b"{\"a\":[{\"b\":1},{\"b\":2}]}\n{\"a\":[{\"b\":[3]}]}\n{\"a\":[{\"b\":[]},{\"b\":[]}]}\n{\"a\":[{\"b\":null}]}\n";
    let output = run(&["a.b"], input);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"1\n2\n[3]\nnull\n");
    let output = run(&["a"], b"[{\"a\":[1]}]\n");
    assert_eq!(output.stdout, b"1\n");
    let output = run(&["$.a"], b"[{\"a\":[1]}]\n");
    assert_eq!(output.stdout, b"[1]\n");
}

#[test]
fn invalid_record_never_emits_a_partial_sequence() {
    let output = run(
        &["a.b"],
        b"{\"a\":[{\"b\":0}]}\n{\"a\":[{\"b\":1},{\"b\":2}],\"bad\":[0,]}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"0\n");
}

#[test]
fn scalar_results_and_runtime_errors_are_framed_per_record() {
    let output = run(&["a + 1"], b"{\"a\":2}\n{}\n{\"a\":null}\n{\"a\":4}\n");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"3\n");
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("line 3:") && error.contains("TypeError"));
    let output = run(&["--", "-a"], b"{\"a\":2}\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"-2\n");
    let output = run(&["a / 0"], b"{\"a\":1}\n{\"a\":0}\n");
    assert_eq!(output.stdout, b"null\nnull\n");
    let output = run(&["'hello'"], b"null\ntrue\n");
    assert_eq!(output.stdout, b"\"hello\"\n\"hello\"\n");
}
