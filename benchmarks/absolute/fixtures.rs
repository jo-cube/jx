#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Validate,
    Scalar,
    Nested,
    Lookup,
    Predicate,
    Project,
    Fold,
    Filtered,
    Callback,
}
pub struct Case {
    pub name: String,
    pub kind: Kind,
    pub source: String,
    pub width: usize,
    pub rows: usize,
    pub keys: Vec<String>,
    pub inputs: Vec<Option<Vec<u8>>>,
    pub worker: bool,
}
// Byte-for-byte the committed actual-worker generator: escaped name, numeric metrics,
// production/drop/tombstone distribution, and one absent payload per 64 records.
pub fn record(size: usize, fields: usize, flag: usize, negative: bool) -> Vec<u8> {
    let metrics = (0..fields)
        .map(|i| {
            format!(
                "\"v{i}\":{}",
                if negative && i == 0 {
                    -1
                } else {
                    (i + 1) as i32
                }
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut s = format!(
        r#"{{"id":"k{}","name":"Ad\u0061","deleted":{},"env":"{}","flag":{},"metrics":{{{metrics}}},"profile":{{"region":"eu"}},"pad":""}}"#,
        flag % 64,
        flag % 13 == 0,
        if flag % 10 == 0 { "test" } else { "production" },
        flag % 2 == 0
    );
    let pad = size.saturating_sub(s.len());
    s = s.replace("\"pad\":\"\"", &format!("\"pad\":\"{}\"", "x".repeat(pad)));
    s.into_bytes()
}
fn project(width: usize, form: &str) -> String {
    let base = if form == "alias" || form == "conditional" {
        "$m."
    } else if form == "call" {
        "$r.metrics."
    } else {
        "metrics."
    };
    let root = if form == "call" { "$r." } else { "" };
    let members = (0..width)
        .map(|i| format!("'v{i}':{base}v{i}+{base}v{i}"))
        .chain([
            format!("'id':{root}id"),
            format!("'name':$uppercase({root}name)"),
            format!("'label':$lookup($vars.labels,{root}id)"),
            format!("'region':{root}profile.region"),
        ])
        .collect::<Vec<_>>()
        .join(",");
    let obj = format!("{{{members}}}");
    match form {
        "alias" => format!("($m:=metrics;{obj})"),
        "conditional" => format!("($m:=flag?metrics:metrics;{obj})"),
        "call" => format!("(function($r){{{obj}}})($)"),
        _ => obj,
    }
}
pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let filter = std::env::var("ABS_CASE").unwrap_or_default();
    let wanted = |name: &str| filter.is_empty() || filter.split('|').any(|f| f == name);
    for (size, width) in [
        (500, 8),
        (1024, 8),
        (1024, 24),
        (1024, 32),
        (1024, 40),
        (10240, 80),
        (1048576, 80),
    ] {
        for form in ["direct", "alias", "conditional", "call"] {
            if !wanted(&format!("project/{size}/{width}/{form}")) {
                continue;
            }
            cases.push(Case {
                name: format!("project/{size}/{width}/{form}"),
                kind: Kind::Project,
                source: project(width, form),
                width,
                rows: 0,
                keys: (0..width).map(|i| format!("v{i}")).collect(),
                inputs: (0..16)
                    .map(|i| Some(record(size, width, i, false)))
                    .collect(),
                worker: false,
            });
        }
    }
    for size in [500, 1024, 10240, 1048576] {
        for (name, kind, source) in [
            ("validate", Kind::Validate, "$"),
            ("scalar", Kind::Scalar, "metrics.v0*metrics.v1+metrics.v0"),
            ("lookup", Kind::Lookup, "$lookup($vars.labels,id)"),
            (
                "predicate",
                Kind::Predicate,
                "env='production' and $lookup($vars.labels,id) in ['label-1','label-3','label-17'] and metrics.v0>0 and profile.region='eu'",
            ),
        ] {
            if !wanted(&format!("{name}/{size}")) {
                continue;
            }
            cases.push(Case {
                name: format!("{name}/{size}"),
                kind,
                source: source.into(),
                width: 2,
                rows: 0,
                keys: vec![],
                inputs: (0..64)
                    .map(|i| {
                        let b = record(size, 8, i, false);
                        Some(
                            if matches!(kind, Kind::Lookup | Kind::Predicate) && i % 4 == 0 {
                                String::from_utf8(b)
                                    .unwrap()
                                    .replacen("\"id\":\"k", "\"id\":\"z", 1)
                                    .into_bytes()
                            } else {
                                b
                            },
                        )
                    })
                    .collect(),
                worker: false,
            });
        }
    }
    for depth in [2, 8, 32] {
        if !wanted(&format!("nested/{depth}")) {
            continue;
        }
        let input = format!(
            "{}{{\"a\":{{\"x\":7,\"y\":3}}}}{}",
            "{\"n\":".repeat(depth),
            "}".repeat(depth)
        );
        let path = "n.".repeat(depth) + "a";
        cases.push(Case {
            name: format!("nested/{depth}"),
            kind: Kind::Nested,
            source: format!("{path}.x*{path}.y+{path}.x"),
            width: 0,
            rows: 0,
            keys: vec![],
            inputs: vec![Some(input.into_bytes())],
            worker: false,
        });
    }
    for rows in [0, 1, 2, 8, 128, 1024, 8192] {
        for size in [48, 500] {
            for select in [0, 50, 100] {
                if !filter.is_empty()
                    && !filter
                        .split('|')
                        .any(|f| f.starts_with(&format!("sequence/{rows}/{size}/{select}/")))
                {
                    continue;
                }
                let input = format!(
                    "{{\"items\":[{}]}}",
                    (0..rows)
                        .map(|i| {
                            let negative = match select {
                                0 => true,
                                50 => i % 2 == 0,
                                _ => false,
                            };
                            if size == 48 {
                                let mut s = format!(
                                    r#"{{"id":"k{}","metrics":{{"v0":{},"v1":2}}}}"#,
                                    i % 64,
                                    if negative { -1 } else { 1 }
                                );
                                s.push_str(&" ".repeat(48 - s.len()));
                                s
                            } else {
                                String::from_utf8(record(size, 2, i, negative)).unwrap()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                );
                for (form, source, kind) in [
                    (
                        "numeric",
                        "$sum(items.(metrics.v0*metrics.v1+metrics.v0))",
                        Kind::Fold,
                    ),
                    (
                        "direct",
                        "$sum(items[metrics.v0>0].(metrics.v0*metrics.v1+metrics.v0))",
                        Kind::Filtered,
                    ),
                    (
                        "lexical",
                        "$sum((items[($m:=metrics;$m.v0>0)]).(metrics.v0*metrics.v1+metrics.v0))",
                        Kind::Filtered,
                    ),
                    (
                        "call",
                        "function($rows){$sum(($rows[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))}(items)",
                        Kind::Filtered,
                    ),
                    (
                        "helper",
                        "($unused:=function($x){$x};$sum($map($filter(items,function($v){$v.metrics.v0>0}),function($v){$v.metrics.v0*$v.metrics.v1+$v.metrics.v0})))",
                        Kind::Filtered,
                    ),
                    (
                        "group",
                        "$sum((items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))",
                        Kind::Filtered,
                    ),
                    (
                        "alias",
                        "($s:=items[metrics.v0>0];$sum($s.(metrics.v0*metrics.v1+metrics.v0)))",
                        Kind::Filtered,
                    ),
                    (
                        "callbacks",
                        "$sum($map($filter(items,function($v){$v.metrics.v0>0}),function($v){$v.metrics.v0*$v.metrics.v1+$v.metrics.v0}))",
                        Kind::Filtered,
                    ),
                    (
                        "output",
                        "$map($filter(items,function($v){$v.metrics.v0>0}),function($v){{'id':$v.id,'x':$v.metrics.v0*$v.metrics.v1+$v.metrics.v0}})",
                        Kind::Callback,
                    ),
                ] {
                    if !wanted(&format!("sequence/{rows}/{size}/{select}/{form}")) {
                        continue;
                    }
                    cases.push(Case {
                        name: format!("sequence/{rows}/{size}/{select}/{form}"),
                        kind,
                        source: source.into(),
                        width: 2,
                        rows,
                        keys: vec![],
                        inputs: vec![Some(input.clone().into_bytes())],
                        worker: false,
                    });
                }
            }
        }
    }
    for (size, width) in [(500, 8), (1024, 40), (10240, 80)] {
        if !wanted(&format!("worker/project/{size}/{width}")) {
            continue;
        }
        cases.push(Case {
            name: format!("worker/project/{size}/{width}"),
            kind: Kind::Project,
            source: project(width, "direct"),
            width,
            rows: 0,
            keys: (0..width).map(|i| format!("v{i}")).collect(),
            inputs: (0..64)
                .map(|i| {
                    if i == 63 {
                        None
                    } else {
                        Some(record(size, width, i, false))
                    }
                })
                .collect(),
            worker: true,
        });
    }
    for rows in [8, 1024] {
        if !wanted(&format!("worker/fold/{rows}")) {
            continue;
        }
        let input = format!(
            "{{\"items\":[{}]}}",
            (0..rows)
                .map(|i| String::from_utf8(record(500, 8, i, false)).unwrap())
                .collect::<Vec<_>>()
                .join(",")
        );
        cases.push(Case {
            name: format!("worker/fold/{rows}"),
            kind: Kind::Filtered,
            source: "$sum((items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))".into(),
            width: 2,
            rows,
            keys: vec![],
            inputs: vec![Some(input.into_bytes())],
            worker: true,
        });
    }
    cases
}
