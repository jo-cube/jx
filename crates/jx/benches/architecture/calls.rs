use super::*;

pub(super) fn run(options: &CompileOptions) {
    let input = record(500, 8, true, false);
    for (name, source) in [
        (
            "alias",
            "($f:=function($v){$v.metrics.v0+$v.metrics.v1};$f($))",
        ),
        (
            "closure",
            "($make:=function($n){function($v){$v.metrics.v0+$n}};$f:=$make(metrics.v1);$f($))",
        ),
        (
            "partial",
            "($f:=function($a,$b){$a+$b};$g:=$f(?,metrics.v1);$g(metrics.v0))",
        ),
        (
            "wide",
            "$zip([metrics.v0],[metrics.v1],[metrics.v2],[metrics.v3],[metrics.v4])",
        ),
    ] {
        bench(&format!("calls/{name}"), &[source.into()], &input, options);
    }
    for (name, source) in [
        (
            "matcher",
            "{'name':$replace(name,/[A-Za-z]+/,function($m){$uppercase($m.match)}),'n':$count($match(name,/[A-Za-z]+/))}",
        ),
        (
            "formatted",
            "{'n':$formatNumber(metrics.v0,'0.00'),'hex':$formatBase(metrics.v1,16)}",
        ),
        ("dynamic", "$eval('metrics.v0+metrics.v1')"),
        ("transform", "$ ~> |metrics|{'v0':v0+1}|"),
    ] {
        bench(
            &format!("semantics/{name}"),
            &[source.into()],
            &input,
            options,
        );
    }
    if std::env::var_os("ARCH_CONTEXT").is_some() {
        for depth in [8, 48] {
            let source = format!(
                "{}.{{'i':$i{}}}",
                (0..depth)
                    .map(|i| format!("items#$i{i}"))
                    .collect::<Vec<_>>()
                    .join("."),
                depth - 1
            );
            for mode in ["engine", "foundation"] {
                measure(
                    &format!("compile/context/{depth}/{mode}"),
                    source.as_bytes(),
                    || {
                        if mode == "engine" {
                            black_box(options.compile(black_box(&source)).unwrap());
                        } else {
                            black_box(options.compile_program(black_box(&source)).unwrap());
                        }
                    },
                );
            }
        }
        for rows in [8, 1024] {
            let record = record(500, 8, true, false);
            let input = format!(
                "{{\"name\":\"parent\",\"items\":[{}]}}",
                vec![record; rows].join(",")
            );
            for (form, query) in [
                ("index", "items#$i.{'i':$i,'v':metrics.v0}"),
                ("parent", "items.{'parent':%.name,'v':metrics.v0}"),
                (
                    "captured_call",
                    "items.($p:=%;$f:=function(){$p.name};{'parent':$f(),'v':metrics.v0})",
                ),
                (
                    "returned_parent",
                    "items.($p:=%;$f:=function(){$p};{'parent':$f().name,'v':metrics.v0})",
                ),
                (
                    "captured_callback",
                    "items.($p:=%;$f:=function($v){$p.name};{'parent':$map([1],$f),'v':metrics.v0})",
                ),
                (
                    "captured_argument",
                    "items.($p:=%;$f:=function($q){$q.name};{'parent':$f($p),'v':metrics.v0})",
                ),
                (
                    "callback",
                    "items#$i.($f:=function(){$i};{'i':$f(),'v':metrics.v0})",
                ),
                ("filter", "items#$i[$i%2=0].{'i':$i,'v':metrics.v0}"),
                ("sort", "items#$i^(>metrics.v0).{'i':$i,'v':metrics.v0}"),
                ("group", "items#$i{'all':[$i,metrics.v0]}"),
            ] {
                bench(
                    &format!("context/{rows}/{form}"),
                    &[query.into()],
                    &input,
                    options,
                );
            }
        }
    }
    if std::env::var_os("ARCH_CONTEXT").is_some() {
        for rows in [8, 128] {
            let row = record(500, 8, true, false);
            let group = format!(
                "{{\"tag\":\"group\",\"items\":[{}]}}",
                vec![row; rows].join(",")
            );
            let input = format!(
                "{{\"name\":\"root\",\"groups\":[{}]}}",
                vec![group; 4].join(",")
            );
            for (form, query) in [
                (
                    "parent",
                    "groups.items.{'parent':%.tag,'root':%.%.name,'v':metrics.v0}",
                ),
                (
                    "callback",
                    "groups.items.($p:=%;$f:=function(){$p.tag};{'parent':$f(),'v':metrics.v0})",
                ),
            ] {
                bench(
                    &format!("context_nested/{rows}/{form}"),
                    &[query.into()],
                    &input,
                    options,
                );
            }
        }
    }
    let wide_input = record(1024, 40, true, false);
    // Same output as the ordinary wide projection. Isolate retained-scope and
    // call-port acquisition from the already indexed direct/alias controls.
    for (name, source) in [
        (
            "wide_projection",
            format!("($f:=function($doc){{{}}};$f($))", projection(40, false)),
        ),
        (
            "wide_parameter",
            format!("($f:=function($doc){{{}}};$f($))", parameter_projection(40)),
        ),
        (
            "wide_signed",
            format!(
                "($f:=function($doc)<o:o>{{{}}};$f($))",
                parameter_projection(40)
            ),
        ),
        (
            "wide_identity",
            format!(
                "($f:=function($doc){{$doc}};$doc:=$f($);{})",
                parameter_projection(40)
            ),
        ),
        (
            "wide_nested",
            format!(
                "($f:=function($doc){{function($doc){{{}}}($doc)}};$f($))",
                parameter_projection(40)
            ),
        ),
        (
            "wide_repeated",
            format!(
                "($f:=function($doc){{$doc}};$f($);$doc:=$f($);{})",
                parameter_projection(40)
            ),
        ),
        (
            "wide_dynamic",
            format!(
                "($f:=flag?function($d){{$d}}:function($d){{$d}};$doc:=$f($);{})",
                parameter_projection(40)
            ),
        ),
        (
            "wide_dynamic_projection",
            format!(
                "($f:=flag?function($doc){{{0}}}:function($doc){{{0}}};$f($))",
                parameter_projection(40)
            ),
        ),
        (
            "repeated_dynamic_projection",
            format!(
                "($f:=flag?function($doc){{{0}}}:function($doc){{{0}}};$f($);$f($))",
                parameter_projection(40)
            ),
        ),
        (
            "repeated_parameter_projection",
            format!(
                "($f:=function($doc){{{0}}};$f($);$f($))",
                parameter_projection(40)
            ),
        ),
        (
            "wide_rooted",
            format!(
                "($f:=function(){{{}}};$f())",
                parameter_projection(40).replace("$doc.", "$$.")
            ),
        ),
        (
            "wide_unused_function",
            format!("($f:=function($v){{$v}};{})", projection(40, true)),
        ),
    ] {
        bench(&format!("calls/{name}"), &[source], &wide_input, options);
    }
    let map_input = format!("{{\"items\":[{}]}}", vec![wide_input.clone(); 8].join(","));
    let map_source = format!("$map(items,function($doc){{{}}})", parameter_projection(40));
    for (name, source) in [
        ("wide_map", map_source.clone()),
        (
            "wide_map_unused",
            format!("($unused:=function($v){{$v}};{map_source})"),
        ),
    ] {
        bench(&format!("calls/{name}"), &[source], &map_input, options);
    }
    let dynamic = format!(
        "($f:=flag?function($d){{$d}}:function($d){{$d}};$doc:=$f($);{})",
        parameter_projection(40)
    );
    bench(
        "calls/shared_dynamic",
        &[projection(40, false), dynamic],
        &wide_input,
        options,
    );
    if std::env::var_os("ARCH_EXPANDED").is_some() {
        let input = format!(
            "{{\"items\":[{}]}}",
            vec![record(500, 8, true, false); 8].join(",")
        );
        for (name, source) in [
            ("dynamic", "items[metrics.v0]"),
            ("negative", "items[-1]"),
            ("duplicates", "items[[0,0,-1]]"),
            (
                "mapped",
                "items[metrics.v0].{'x':metrics.v0,'s':$uppercase(name)}",
            ),
        ] {
            bench(
                &format!("candidates/8/{name}"),
                &[source.into()],
                &input,
                options,
            );
        }
    }
    for rows in [8, 1024] {
        let input = format!(
            "{{\"items\":[{}]}}",
            vec![record(500, 8, true, false); rows].join(",")
        );
        for (name, source) in [
            (
                "alias_map",
                "($f:=function($v){{'x':$v.metrics.v0+$v.metrics.v1,'s':$uppercase($v.name)}};$map(items,$f))",
            ),
            ("row_closure", "items.($f:=function(){metrics.v0};$f())"),
            (
                "reduce",
                "$reduce(items,function($a,$v){$a+$v.metrics.v0},0)",
            ),
        ] {
            bench(
                &format!("calls/{rows}/{name}"),
                &[source.into()],
                &input,
                options,
            );
        }
        let source = "items.(metrics.v0*2)";
        let name = format!("delivery/{rows}");
        if std::env::var("ARCH_FILTER").is_ok_and(|filter| !name.contains(&filter)) {
            continue;
        }
        let expression = options.compile(source).unwrap();
        let program = options.compile_program(source).unwrap();
        let plan = InputPlan::new([&expression]);
        let mut acquisition = jx::execution::Acquisition::new([&program]);
        let mut output = Vec::with_capacity(input.len());
        for mode in ["engine", "foundation"] {
            measure(&format!("{name}/{mode}"), input.as_bytes(), || {
                let evaluation = if mode == "engine" {
                    plan.prepare(black_box(input.as_bytes()))
                        .unwrap()
                        .evaluate(0)
                } else {
                    let document = acquisition.prepare(black_box(input.as_bytes())).unwrap();
                    program.evaluate(&document)
                };
                write(evaluation.unwrap(), &mut output).unwrap();
            });
        }
    }
}

pub(super) fn parameter_projection(width: usize) -> String {
    projection(width, false)
        .replace("metrics.", "$doc.metrics.")
        .replace("$uppercase(name)", "$uppercase($doc.name)")
        .replace("$contains(name,", "$contains($doc.name,")
        .replace(" env=", " $doc.env=")
        .replace(",id)", ",$doc.id)")
        .replace(":nested.", ":$doc.nested.")
        .replace("*nested.", "*$doc.nested.")
        .replace(":profile.", ":$doc.profile.")
}
