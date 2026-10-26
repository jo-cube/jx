use super::*;
pub(super) fn run(options: &CompileOptions) {
    for size in [48, 500] {
        let base = r#"{"metrics":{"v0":3,"v1":7},"pad":""}"#;
        let row = base.replace(
            "\"pad\":\"\"",
            &format!("\"pad\":\"{}\"", "x".repeat(size - base.len())),
        );
        assert_eq!(row.len(), size);
        let input = format!("{{\"items\":[{}]}}", vec![row; 1024].join(","));
        for (form, source) in [
            ("field", "$sum(items.metrics.v0)"),
            ("path", "$sum(items.(metrics.v0))"),
            ("map", "$sum($map(items,function($v){$v.metrics.v0}))"),
            ("alias", "($m:=items.metrics;$sum($m.v0))"),
            ("alias_path", "($m:=items.(metrics);$sum($m.v0))"),
            (
                "callback_alias",
                "($m:=$map(items,function($v){$v.metrics});$sum($m.v0))",
            ),
            (
                "multi_direct",
                "$sum(items.metrics.v0)+$count(items.metrics.v0)",
            ),
            ("multi_alias", "($m:=items.metrics.v0;$sum($m)+$count($m))"),
            ("objects", "items.({'a':metrics.v0,'b':metrics.v1})"),
            ("object_alias", "items.($m:=metrics;{'a':$m.v0,'b':$m.v1})"),
        ] {
            bench(
                &format!("sequence/1024/{size}/{form}"),
                &[source.into()],
                &input,
                options,
            );
        }
    }
    let input = format!(
        "{{\"items\":[{}]}}",
        (1..=1024)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    for (form, source) in [
        ("path", "$sum(items.($*2+$))"),
        ("map", "$sum($map(items,function($v){$v*2+$v}))"),
        ("alias", "($a:=items;$sum($a.($*2+$)))"),
    ] {
        bench(
            &format!("sequence/numbers/{form}"),
            &[source.into()],
            &input,
            options,
        );
    }
}

pub(super) fn convergence(options: &CompileOptions) {
    for n in [8, 1024] {
        let input = format!(
            "{{\"a\":1,\"b\":1024,\"items\":[{}]}}",
            vec![r#"{"metrics":{"v0":3,"v1":7},"children":[{"v":1},{"v":2}],"k":"x"}"#; n]
                .join(",")
        );
        for (name, source) in [
            ("nested_fold", "$sum(items.metrics.(v0))"),
            ("field_fold", "$sum(items.metrics.v0)"),
            ("stages", "items.children[v>0].(v+1).($string($))"),
            ("positions", "items.children[true][-1].v"),
            ("group", "items{k:$sum(metrics.v0)}"),
            ("order", "items^(metrics.v0).k"),
            ("range", "[a..b].($*2)"),
        ] {
            bench(
                &format!("convergence/{n}/{name}"),
                &[source.into()],
                &input,
                options,
            );
        }
    }
}

// Sequence length, selectivity and equivalent producer/call boundaries. Generate
// records in memory; validation and serialization remain in every timed sample.
pub(super) fn pipelines(options: &CompileOptions) {
    for rows in [0, 1, 2, 8, 64, 128, 1024, 8192] {
        for size in [48, 500] {
            for percent in [0, 50, 100] {
                let input = format!(
                    "{{\"items\":[{}]}}",
                    (0..rows)
                        .map(|i| {
                            let positive = percent == 100 || (percent == 50 && i % 2 == 0);
                            let base = format!(
                                "{{\"metrics\":{{\"v0\":{},\"v1\":7}},\"pad\":\"\"}}",
                                if positive { 3 } else { -3 }
                            );
                            let padding = size - base.len();
                            base.replace(
                                "\"pad\":\"\"",
                                &format!("\"pad\":\"{}\"", "x".repeat(padding)),
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(",")
                );
                for (name, source) in [
                    (
                        "direct",
                        "$sum(items[metrics.v0>0].(metrics.v0*metrics.v1+metrics.v0))",
                    ),
                    (
                        "group",
                        "$sum((items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))",
                    ),
                    (
                        "alias",
                        "($rows:=items;$sum(($rows[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0)))",
                    ),
                    (
                        "call",
                        "function($rows){$sum(($rows[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))}(items)",
                    ),
                    (
                        "callbacks",
                        "$sum($map($filter(items,function($v){$v.metrics.v0>0}),function($v){$v.metrics.v0*$v.metrics.v1+$v.metrics.v0}))",
                    ),
                ] {
                    bench(
                        &format!("pipelines/{rows}/{size}/{percent}/{name}"),
                        &[source.into()],
                        &input,
                        options,
                    );
                }
            }
        }
    }
}
