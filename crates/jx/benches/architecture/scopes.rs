use super::*;

pub(super) fn run(options: &CompileOptions) {
    // Vary scan cost independently of per-row numeric activation. All variants
    // validate, consume and serialize the same numeric result in both engines.
    for rows in [8, 1024] {
        for size in [48, 500] {
            let row = format!(
                "{{\"metrics\":{{\"v0\":1,\"v1\":2}},\"pad\":\"{}\"}}",
                "x".repeat(size - 36)
            );
            assert_eq!(row.len(), size);
            let input = format!("{{\"items\":[{}]}}", vec![row; rows].join(","));
            for (form, expression) in [
                ("constant", "$sum(items.(3))"),
                (
                    "map",
                    "$sum($map(items,function($v){$v.metrics.v0*$v.metrics.v1+$v.metrics.v0}))",
                ),
                (
                    "path",
                    "$sum((items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))",
                ),
                (
                    "local",
                    "$sum((items[($m:=metrics;$m.v0>0)]).(metrics.v0*metrics.v1+metrics.v0))",
                ),
            ] {
                bench(
                    &format!("numeric_stages/{rows}/{size}/{form}"),
                    &[expression.into()],
                    &input,
                    options,
                );
            }
        }
    }

    // Increasing live cross-stage values, including the physical inline storage
    // transition. Both engines validate, acquire, reduce and serialize equally.
    for width in [2, 8, 24, 40] {
        let row = format!(
            "{{\"metrics\":{{{}}}}}",
            (0..width)
                .map(|i| format!("\"v{i}\":{}", i + 1))
                .collect::<Vec<_>>()
                .join(",")
        );
        let input = format!("{{\"items\":[{}]}}", vec![row; 128].join(","));
        let sum = (0..width)
            .map(|i| format!("metrics.v{i}"))
            .collect::<Vec<_>>()
            .join("+");
        for (form, predicate) in [
            ("direct", format!("{sum}>0")),
            ("local", format!("($n:={sum};$n>0)")),
        ] {
            bench(
                &format!("fold_width/{width}/{form}"),
                &[format!("$sum((items[{predicate}]).(({sum})*2))")],
                &input,
                options,
            );
        }
    }

    // All fixtures also run on the entry foundation: matched obligations and
    // identical harness, not a comparison with newly supported syntax.
    for rows in [8, 128] {
        let row = record(500, 8, true, false);
        let group = format!(
            "{{\"tag\":\"group\",\"items\":[{}]}}",
            vec![row; rows].join(",")
        );
        let input = format!("{{\"groups\":[{}]}}", vec![group; 4].join(","));
        for (form, source) in [
            (
                "nested_map",
                "$map(groups,function($g){$map($g.items,function($v){{'tag':$g.tag,'id':$v.id,'name':$uppercase($v.name),'a':$v.metrics.v0+$v.metrics.v1,'b':$v.profile.region}})})",
            ),
            (
                "nested_fold",
                "$sum($map(groups,function($g){$sum($map($g.items,function($v){$v.metrics.v0*$v.metrics.v1+$v.metrics.v0}))}))",
            ),
            (
                "nested_focus",
                "$map(groups,function($g){$map($g.items,function($v){($m:=$v.metrics;{'a':$m.v0+$m.v1,'b':$m.v0*$m.v1,'c':$v.id,'d':$g.tag})})})",
            ),
        ] {
            bench(
                &format!("scopes/{rows}/{form}"),
                &[source.into()],
                &input,
                options,
            );
        }
    }
    for accepted in [0, 10, 50, 100] {
        let input = format!(
            "{{\"items\":[{}]}}",
            (0..1024)
                .map(|i| {
                    let row = record(500, 8, true, false);
                    if i % 100 < accepted {
                        row
                    } else {
                        row.replace("\"v0\":1", "\"v0\":-1")
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        );
        let body =
            "{'id':id,'name':$uppercase(name),'n':metrics.v0+metrics.v1,'region':profile.region}";
        for (form, source) in [
            ("direct", format!("items[metrics.v0>0].{body}")),
            ("grouped", format!("(items[metrics.v0>0]).{body}")),
        ] {
            bench(
                &format!("selectivity/{accepted}/{form}"),
                &[source],
                &input,
                options,
            );
        }
    }

    for (distribution, pct) in [
        ("alternate", 50),
        ("mixed10", 10),
        ("mixed50", 50),
        ("mixed90", 90),
    ] {
        let input = format!(
            "{{\"items\":[{}]}}",
            (0..1024_u64)
                .map(|i| {
                    let mut mixed = i.wrapping_add(0x9e3779b97f4a7c15);
                    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
                    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d049bb133111eb);
                    mixed ^= mixed >> 31;
                    let accepted = if distribution == "alternate" {
                        i % 2 == 0
                    } else {
                        mixed % 100 < pct
                    };
                    let row = record(500, 8, true, false);
                    if accepted {
                        row
                    } else {
                        row.replace("\"v0\":1", "\"v0\":-1")
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        );
        let body =
            "{'id':id,'name':$uppercase(name),'n':metrics.v0+metrics.v1,'region':profile.region}";
        for (form, source) in [
            ("direct", format!("items[metrics.v0>0].{body}")),
            ("grouped", format!("(items[metrics.v0>0]).{body}")),
        ] {
            bench(
                &format!("selection_mix/{distribution}/{form}"),
                &[source],
                &input,
                options,
            );
        }
    }
    for n in [8, 40] {
        let input = record(1024, n, true, false);
        let project = |base: &str| {
            format!(
                "{{{}}}",
                (0..n)
                    .map(|i| format!("'v{i}':{base}v{i}+{base}v{i}"))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        for (form, source) in [
            ("direct", project("metrics.")),
            ("alias", format!("($m:=metrics;{})", project("$m."))),
            ("scope", format!("metrics.{}", project(""))),
        ] {
            bench(
                &format!("object_scope/{n}/{form}"),
                &[source],
                &input,
                options,
            );
        }
    }

    for n in [8, 40, 80] {
        let input = record(if n > 40 { 2048 } else { 1024 }, n, true, false);
        let source = format!(
            "({}{{'id':id}})",
            (0..n)
                .map(|i| format!("$v{i}:=metrics.v{i};"))
                .collect::<String>()
        );
        bench(&format!("dead_fields/{n}"), &[source], &input, options);
    }
    let input = format!(
        "{{\"items\":[{}]}}",
        vec![record(500, 8, true, false); 1024].join(",")
    );
    for (form, source) in [
        (
            "direct",
            "$sum(items[($m:=metrics;$m.v0>0)].(metrics.v0*metrics.v1+metrics.v0))",
        ),
        (
            "grouped",
            "$sum((items[($m:=metrics;$m.v0>0)]).(metrics.v0*metrics.v1+metrics.v0))",
        ),
    ] {
        bench(
            &format!("fold_local/{form}"),
            &[source.into()],
            &input,
            options,
        );
    }
    if std::env::var_os("ARCH_EXPANDED").is_some() {
        for rows in [8, 1024] {
            let input = format!(
                "{{\"items\":[{}]}}",
                vec![record(500, 8, true, false); rows].join(",")
            );
            for (form, source) in [
                ("names", "items.name"),
                ("nested", "items.metrics.v0"),
                ("array_result", "items.([metrics.v0,metrics.v1])"),
                (
                    "object",
                    "{'names':items.name,'regions':items.profile.region,'total':$sum(items.metrics.v0)}",
                ),
                (
                    "fallback",
                    "items.{'name':name ?? 'unknown','v':metrics.v0 ?: 0}",
                ),
            ] {
                bench(
                    &format!("expanded/{rows}/{form}"),
                    &[source.into()],
                    &input,
                    options,
                );
            }
        }
    }
}
