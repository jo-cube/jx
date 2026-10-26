//! Copied actual workers; only public compilation selection differs.
use super::{facade_worker, fixtures, metrics, reference};
use std::{
    hint::black_box,
    sync::Barrier,
    time::{Duration, Instant},
};
struct Case {
    name: String,
    inputs: Vec<Option<Vec<u8>>>,
    source: String,
    drops: Vec<String>,
    tombstones: Vec<String>,
    vars: Option<String>,
}
fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (size, width) in [(500, 8), (1024, 40), (16384, 40)] {
        let source = format!(
            "{{{}}}",
            (0..width)
                .map(|i| format!("'v{i}':$number(metrics.v{i})*2"))
                .chain([
                    "'id':id".into(),
                    "'name':$uppercase(name)".into(),
                    "'label':$lookup($vars.labels,id)".into(),
                    "'region':profile.region".into(),
                    "'text':$string(metrics.v1)".into()
                ])
                .collect::<Vec<_>>()
                .join(",")
        );
        for eval in [false, true]
            .into_iter()
            .filter(|eval| !*eval || size == 1024)
        {
            cases.push(Case {
                name: format!(
                    "trial/{size}/{width}/{}",
                    if eval { "eval" } else { "direct" }
                ),
                source: if eval {
                    format!("$eval({source:?})")
                } else {
                    source.clone()
                },
                inputs: (0..64)
                    .map(|i| {
                        if i == 63 {
                            None
                        } else {
                            Some(fixtures::record(size, width, i, i % 19 == 0))
                        }
                    })
                    .collect(),
                drops: vec![
                    "env!='production'".into(),
                    "$not($exists($lookup($vars.allowed,id)))".into(),
                    "$number(metrics.v0)<$vars.cutoff".into(),
                ],
                tombstones: vec!["deleted=true".into(), "id in ['k29','k43']".into()],
                vars: None,
            });
        }
    }
    for rows in [8, 128] {
        let input = format!(
            "{{\"items\":[{}]}}",
            (0..rows)
                .map(|i| String::from_utf8(fixtures::record(500, 8, i, i % 2 == 0)).unwrap())
                .collect::<Vec<_>>()
                .join(",")
        )
        .into_bytes();
        for (form, source) in [
            (
                "fold",
                "$sum((items[metrics.v0>0]).(metrics.v0*metrics.v1+metrics.v0))",
            ),
            (
                "callbacks",
                "{'values':$map(items,function($v){{'id':$v.id,'x':$v.metrics.v0*2}})}",
            ),
        ] {
            cases.push(Case {
                name: format!("trial/{rows}/{form}"),
                inputs: vec![Some(input.clone()), None],
                source: source.into(),
                drops: vec![],
                tombstones: vec![],
                vars: None,
            });
        }
    }
    cases.extend(application_cases());
    cases
}

// Fixed distribution, independent whitelist/blacklist probes and compound keys.
// Unknown shapes are covered by worker/API tests; these are successful-work fixtures.
fn application_cases() -> Vec<Case> {
    use serde_json::{Value, json};
    use std::collections::BTreeMap;
    let mut cases = Vec::new();
    for (size, width) in [(500, 8), (1024, 40), (16384, 40)] {
        for reject in [32, 128, 224] {
            let vars = serde_json::to_string(&json!({
                "whitelist": (0..4096).filter(|i| i % 256 >= reject)
                    .map(|i| (format!("k{i}:update"), true)).collect::<BTreeMap<_,_>>(),
                "blacklist": (0..4096).filter(|i| i % 31 == 0)
                    .map(|i| (format!("k{i}:update"), true)).collect::<BTreeMap<_,_>>(),
                "labels": (0..4096).map(|i| (format!("k{i}"),format!("label-{i}"))).collect::<BTreeMap<_,_>>(),
                "cutoff": 0
            })).unwrap();
            let inputs = (0..256).map(|i| {
                if i == 255 { return None; }
                let mut row = json!({"customer":{"id":format!("k{i}"),"region":"eu"},
                    "event":{"type":"update"}, "name":"Müller", "env":if i%29==0{"staging"}else{"production"},
                    "deleted":i%19==0, "metrics":(0..width).map(|n|(format!("v{n}"),Value::from((i+n)%17+1))).collect::<BTreeMap<_,_>>(),
                    "padding":""});
                let base = serde_json::to_vec(&row).unwrap().len();
                assert!(base<=size);
                row["padding"]=Value::from("x".repeat(size-base));
                let bytes=serde_json::to_vec(&row).unwrap(); assert_eq!(bytes.len(),size); Some(bytes)
            }).collect::<Vec<_>>();
            let source = format!(
                "{{{}}}",
                (0..width)
                    .map(|i| format!("'v{i}':$number(metrics.v{i})*2"))
                    .chain([
                        "'key':customer.id & ':' & event.type".into(),
                        "'name':$uppercase(name)".into(),
                        "'label':$lookup($vars.labels,customer.id)".into(),
                        "'region':customer.region".into(),
                        "'text':$string(metrics.v1)".into()
                    ])
                    .collect::<Vec<_>>()
                    .join(",")
            );
            for eval in [false, true]
                .into_iter()
                .filter(|e| !*e || (size == 1024 && reject == 128))
            {
                cases.push(Case {
                    name: format!(
                        "application/{size}/{reject}/{}",
                        if eval { "eval" } else { "direct" }
                    ),
                    source: if eval {
                        format!("$eval({source:?})")
                    } else {
                        source.clone()
                    },
                    inputs: inputs.clone(),
                    vars: Some(vars.clone()),
                    drops: vec![
                        "env!='production'".into(),
                        "$not($exists($lookup($vars.whitelist,customer.id & ':' & event.type)))"
                            .into(),
                        "$exists($lookup($vars.blacklist,customer.id & ':' & event.type))".into(),
                        "$number(metrics.v0)<$vars.cutoff".into(),
                    ],
                    tombstones: vec!["deleted=true".into()],
                });
            }
        }
    }
    cases
}

macro_rules! implementation {
 ($fn:ident,$module:ident)=>{
 fn $fn(c:&Case,vars:&str,threads:usize,check:bool)->Vec<String>{
  use $module::jsonata::{Worker,Action,PassPayload,ErrorPolicies,InvalidJsonPolicy,EvaluationPolicy};
  let plan=$module::build_plan(&c.drops,&c.tombstones,false,Some(&c.source),Some(vars),false).unwrap();
  let input=plan.input_plan();let mut worker=Worker::new(&plan,&input,false,16384);
  let policies=ErrorPolicies{invalid_json:InvalidJsonPolicy::Fail,evaluation:EvaluationPolicy::Fail};
  let mut spare=Vec::with_capacity(c.inputs.iter().flatten().map(Vec::len).max().unwrap());let mut i=0;
  let execute=|worker:&mut Worker<'_>,i:usize,spare:&mut Vec<u8>|{
   spare.clear();let mut source=c.inputs[i%c.inputs.len()].as_ref().map(|b|{spare.extend_from_slice(b);std::mem::take(spare)});
   let result=worker.execute_report(&mut source,policies).unwrap();
   let snapshot=check.then(||format!("{result:?}"));
   let n=match result.action{Action::Project(bytes)|Action::PassThrough(PassPayload::Exact(bytes))|Action::PassThrough(PassPayload::Json{bytes,..})=>{let n=bytes.len();black_box(&bytes);*spare=bytes;n},Action::Drop|Action::Tombstone=>{if let Some(b)=source{*spare=b};0}};
   (n,snapshot)
  };
  if check{return (0..c.inputs.len()).map(|i|execute(&mut worker,i,&mut spare).1.unwrap()).collect()}
  let bytes=c.inputs.iter().flatten().map(Vec::len).sum::<usize>() as f64/c.inputs.len() as f64;
  if threads==1{metrics::measure_cycle(&c.name,stringify!($module),"worker",bytes,c.inputs.len(),||{let n=execute(&mut worker,i,&mut spare).0;i+=1;n});return vec![]}
  metrics::parallel_disabled();let barrier=Barrier::new(threads+1);
  let ms=std::env::var("ABS_MS").ok().map(|x|x.parse().unwrap()).unwrap_or(500);
  let (records,outputs,wall,cpu)=std::thread::scope(|scope|{
   let handles=(0..threads).map(|_|{let barrier=&barrier;let plan=&plan;let execute=&execute;scope.spawn(move||{
    let input=plan.input_plan();let mut worker=Worker::new(plan,&input,false,16384);let mut spare=Vec::with_capacity(c.inputs.iter().flatten().map(Vec::len).max().unwrap());
    for i in 0..64_usize.div_ceil(c.inputs.len())*c.inputs.len(){execute(&mut worker,i,&mut spare);}
    barrier.wait();let at=Instant::now();let mut n=0;let mut output=0;
    while at.elapsed()<Duration::from_millis(ms){for _ in 0..32{output+=execute(&mut worker,n,&mut spare).0;n+=1}}
    (n,output)
   })}).collect::<Vec<_>>();barrier.wait();let at=Instant::now();let cpu=metrics::cpu();let (n,out)=handles.into_iter().map(|h|h.join().unwrap()).fold((0,0),|(n,o),(a,b)|(n+a,o+b));(n,out,at.elapsed().as_secs_f64(),metrics::cpu()-cpu)
  });
  println!("{}",serde_json::json!({"case":c.name,"mode":stringify!($module),"threads":threads,"records":records,"output_bytes":outputs,"wall_seconds":wall,"cpu_seconds":cpu,"input_bytes":bytes}));vec![]
 }
 }
}
implementation!(legacy, reference);
implementation!(facade, facade_worker);
pub fn run() {
    let vars=serde_json::to_string(&serde_json::json!({"labels":(0..4096).map(|i|(format!("k{i}"),format!("label-{i}"))).collect::<std::collections::BTreeMap<_,_>>(),"allowed":(0..4096).filter(|i|i%7!=0).map(|i|(format!("k{i}"),true)).collect::<std::collections::BTreeMap<_,_>>(),"cutoff":0})).unwrap();
    let mode = std::env::var("ABS_MODE").unwrap_or_else(|_| "facade".into());
    let threads = std::env::var("ABS_THREADS")
        .ok()
        .map(|s| s.parse().unwrap())
        .unwrap_or(1);
    let filter = std::env::var("ABS_CASE").unwrap_or_default();
    let cases = cases();
    if threads == 1 {
        println!(
            "case,mode,phase,sample,records,wall_seconds,cpu_seconds,input_bytes,output_bytes,allocations,allocated_bytes,peak_bytes,probe_output_bytes"
        );
    }
    for c in cases
        .iter()
        .filter(|c| filter.is_empty() || filter.split('|').any(|f| f == c.name))
    {
        let vars = c.vars.as_deref().unwrap_or(&vars);
        let expected = legacy(c, vars, 1, true);
        assert_eq!(facade(c, vars, 1, true), expected, "{}", c.name);
        if let Ok(destination) = std::env::var("ABS_EXPORT") {
            std::fs::create_dir_all(&destination).unwrap();
            std::fs::write(std::path::Path::new(&destination).join(c.name.replace('/', "-") + ".json"),
                serde_json::to_vec(&serde_json::json!({"case":c.name,"source":c.source,"drops":c.drops,"tombstones":c.tombstones,"vars":vars,"inputs":c.inputs.iter().map(|b|b.as_ref().map(|b|std::str::from_utf8(b).unwrap())).collect::<Vec<_>>(),"expected":expected})).unwrap()).unwrap();
        }
        eprintln!(
            "fixture,{}",
            serde_json::json!({"case":c.name,"source":c.source,"drops":c.drops,"tombstones":c.tombstones,"input_sizes":c.inputs.iter().map(|x|x.as_ref().map_or(0,Vec::len)).collect::<Vec<_>>(),"checked_records":expected.len(),"drops_observed":expected.iter().filter(|s|s.contains("action: Drop")).count(),"tombstones_observed":expected.iter().filter(|s|s.contains("action: Tombstone")).count(),"outcome_lengths":expected.iter().map(String::len).collect::<Vec<_>>()})
        );
        if std::env::var_os("ABS_CHECK").is_none() {
            match mode.as_str() {
                "legacy" => {
                    legacy(c, vars, threads, false);
                }
                "facade" => {
                    facade(c, vars, threads, false);
                }
                _ => panic!("mode"),
            };
        }
    }
}
