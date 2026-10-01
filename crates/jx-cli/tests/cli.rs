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
    let output = run(&["a["], b"");
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

#[test]
fn filters_frame_values_and_report_streamed_errors() {
    let output = run(&["a[$ > 1]"], b"{\"a\":[0,2,3]}\n{\"a\":[]}\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"2\n3\n");
    let output = run(&["a[$+1 > 1]"], b"{\"a\":[1,null,3]}\n{\"a\":[4]}\n");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"1\n");
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("line 1:") && error.contains("TypeError"));
    let output = run(&["a[true]"], b"{\"a\":[1,2],\"unused\":[0,]}\n");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[test]
fn aggregates_distinguish_missing_empty_and_failed_records() {
    let output = run(
        &["$sum(a)"],
        b"{}\n{\"a\":[]}\n{\"a\":[1,2]}\n{\"a\":null}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"0\n3\n");
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("line 4:") && error.contains("TypeError"));
    let output = run(&["$count(a[$ > 1])"], b"{\"a\":[0,2,3]}\n{}\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"2\n0\n");
    let output = run(&["$sum(a[$ + 1 > 0])"], b"{\"a\":[1,null]}\n");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[test]
fn constructors_frame_complete_outputs_and_mapped_failures() {
    let output = run(
        &[r#"{"sum":$sum(a[$>1]),"values":[a[$>1]],"missing":absent,"null":null}"#],
        b"{\"a\":[1,2,3]}\n{}\n",
    );
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"{\"sum\":5,\"values\":[2,3],\"null\":null}\n{\"values\":[],\"null\":null}\n"
    );
    let output = run(&["a.[b,b+1]"], br#"{"a":[{"b":1},{"b":3}]}"#);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"[1,2]\n[3,4]\n");
    let output = run(&[r#"a.{"x":b+1}"#], br#"{"a":[{"b":1},{"b":null}]}"#);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"{\"x\":2}\n");
    let output = run(&[r#"[a.{"x":b+1}]"#], br#"{"a":[{"b":1},{"b":null}]}"#);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let output = run(&[r#"{"x":1,"\u0078":2}"#], b"{}\n");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("DuplicateKey")
    );
}

#[test]
fn lexical_runtime_is_fresh_for_each_record() {
    let output = run(
        &["($x:=a;$f:=function(){$x? $x+1 : $$.fallback};$f())"],
        b"{\"a\":2}\n{\"fallback\":9}\n{\"a\":4}\n",
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"3\n9\n5\n");
    let output = run(&["function(){1}"], b"null\n");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("functions have no JSON encoding")
    );
}

#[test]
fn kept_sequences_and_reductions_preserve_record_framing() {
    let output = run(&["a[]"], b"{}\n{\"a\":null}\n{\"a\":1}\n{\"a\":[1,2]}\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"[null]\n[1]\n[1,2]\n");
    let output = run(
        &["a^(>v){k:v[]}"],
        br#"{"a":[{"k":"x","v":1},{"k":"x","v":2}]}"#,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"{\"x\":[2,1]}\n");
}

#[test]
fn scoped_paths_reset_bindings_and_preserve_sequence_framing() {
    let input = b"{\"a\":[10,20]}\n{\"a\":[30]}\n{\"a\":[]}\n";
    let output = run(&["a#$i.{\"value\":$,\"index\":$i}"], input);
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"{\"value\":10,\"index\":0}\n{\"value\":20,\"index\":1}\n{\"value\":30,\"index\":0}\n"
    );
    assert_eq!(run(&["a#$i.$i[]"], input).stdout, b"[0,1]\n[0]\n");
}

#[test]
fn builtins_frame_sequences_and_escape_computed_strings() {
    let input = b"{\"a\":[1,2]}\n{\"a\":[3]}\n{\"a\":[]}\n{}\n";
    assert_eq!(
        run(&["$map(a,function($v){$v*2})"], input).stdout,
        b"2\n4\n6\n"
    );
    assert_eq!(
        run(&["$map(a,function($v){$v*2})[]"], input).stdout,
        b"[2,4]\n[6]\n"
    );
    let output = run(
        &["$uppercase(text)"],
        b"{\"text\":\"a\\\"b\\nc\"}\n{\"text\":null}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"\"A\\\"B\\u000aC\"\n");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("line 2:")
    );
}

#[test]
fn conversion_and_chaining_preserve_ndjson_boundaries() {
    let output = run(
        &[r#"{"label":"n=" & $number(n),"json":$string(obj)}"#],
        b"{\"n\":\"03\",\"obj\":{\"v\":1.200}}\n{\"n\":true}\n",
    );
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"{\"label\":\"n=3\",\"json\":\"{\\\"v\\\":1.2}\"}\n{\"label\":\"n=1\"}\n"
    );
    let input = b"{\"a\":[1,2]}\n{\"a\":[3]}\n{}\n";
    assert_eq!(
        run(&["a ~> $map($string)"], input).stdout,
        b"\"1\"\n\"2\"\n\"3\"\n"
    );
    assert_eq!(
        run(&["a ~> $map($string)[]"], input).stdout,
        b"[\"1\",\"2\"]\n[\"3\"]\n"
    );
    let output = run(
        &["($p:=$number(?);$p(n))"],
        b"{\"n\":\"2\"}\n{\"n\":null}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"2\n");
}

#[test]
fn matcher_results_keep_ndjson_framing_and_record_isolation() {
    let output = run(
        &["{'matches':$match(text,/(a)(b)?/),'clean':$replace(text,/a/,'X')}"],
        br#"{"text":"ab a"}
{"text":"zzz"}
{"text":"a"}
"#,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, br#"{"matches":[{"match":"ab","index":0,"groups":["a","b"]},{"match":"a","index":3,"groups":["a",null]}],"clean":"Xb X"}
{"clean":"zzz"}
{"matches":{"match":"a","index":0,"groups":["a",null]},"clean":"X"}
"#);
    let output = run(&["$match(text,/a*/)"], b"{\"text\":\"ab\"}\n");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("(RegexError)"));
}

#[test]
fn parent_navigation_and_transforms_preserve_record_boundaries() {
    let input = br#"{"orders":[{"id":"A","items":[{"n":1},{"n":2}]}]}
{"orders":[{"id":"B","items":[{"n":3}]}]}
"#;
    let output = run(&["orders.items.{ 'order':%.id,'n':n }"], input);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"{\"order\":\"A\",\"n\":1}\n{\"order\":\"A\",\"n\":2}\n{\"order\":\"B\",\"n\":3}\n"
    );
    let output = run(&["$ ~> |orders.items[n>1]|{'n':n+1}|"], input);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout,b"{\"orders\":[{\"id\":\"A\",\"items\":[{\"n\":1},{\"n\":3}]}]}\n{\"orders\":[{\"id\":\"B\",\"items\":[{\"n\":4}]}]}\n");
    let output = run(&["$ ~> |orders.items|5|"], input);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("(TypeError)"));
}

#[test]
fn everyday_helpers_and_user_errors_stream_records() {
    let output = run(
        &["{'n':$round(n,2),'id':$pad(id,-3,'0')}"],
        br#"{"n":4.525,"id":"a"}
{"n":2.345,"id":"b"}
"#,
    );
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"{\"n\":4.52,\"id\":\"00a\"}\n{\"n\":2.34,\"id\":\"00b\"}\n"
    );
    let output = run(
        &["($assert(n>0,'positive required');n)"],
        b"{\"n\":1}\n{\"n\":0}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"1\n");
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("line 2"));
    assert!(error.contains("positive required"));
    assert!(error.contains("(AssertionFailed)"));
}

#[test]
fn formatting_is_compiled_once_and_streams_records() {
    let output = run(
        &["{'n':$formatNumber(n,'0.00'),'date':$fromMillis(t,'[Y0001]-[M01]-[D01]')}"],
        br#"{"n":4.525,"t":0}
{"n":2.345,"t":1526947200000}
"#,
    );
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"{\"n\":\"4.52\",\"date\":\"1970-01-01\"}\n{\"n\":\"2.34\",\"date\":\"2018-05-22\"}\n"
    );
    let output = run(
        &["$toMillis(date)"],
        b"{\"date\":\"2018-05-22\"}\n{\"date\":\"bad\"}\n",
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"1526947200000\n");
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("(DateTimeError)")
    );
}
