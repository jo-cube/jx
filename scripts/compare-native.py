"""Same-binary JIT/interpreter measurements, sequential in both orders."""
import argparse, csv, json, os, statistics, subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument("binary");p.add_argument("prefix");p.add_argument("--reverse",action="store_true");p.add_argument("--ms",default="100");a=p.parse_args()
data={}
for native in ([True,False] if a.reverse else [False,True]):
    env={**os.environ,"JX_BENCH_NATIVE_ONLY":"1","JX_BENCH_SAMPLE_MS":a.ms}
    env.pop("JX_BENCH_NATIVE",None)
    if native:env["JX_BENCH_NATIVE"]="1"
    run=subprocess.run([a.binary],env=env,text=True,capture_output=True,check=True)
    tag="native" if native else "interpreter"
    Path(a.prefix+"-"+tag+".csv").write_text(run.stdout);Path(a.prefix+"-"+tag+".log").write_text(run.stderr)
    groups={}
    for row in csv.DictReader(run.stdout.splitlines()):groups.setdefault((row["workload"],row["input_bytes"]),[]).append(row)
    data[tag]={key:{"rate":statistics.median(float(r["records_per_second"]) for r in rows),"allocations":float(rows[0]["allocations_per_record"]),"bytes":float(rows[0]["allocated_bytes_per_record"])} for key,rows in groups.items()}
with open(a.prefix+"-comparison.csv","w") as f:
    w=csv.writer(f);w.writerow(["workload","input_bytes","interpreter_records_s","native_records_s","change_percent","interpreter_allocations","native_allocations","interpreter_allocated_bytes","native_allocated_bytes","break_even_records"])
    for key,before in data["interpreter"].items():
        after=data["native"][key]
        saved=1/before["rate"]-1/after["rate"]
        compiler=data["native"].get(("jit/compile_"+key[0].removeprefix("jit/"),"0"))
        interpreted_compiler=data["interpreter"].get(("jit/compile_"+key[0].removeprefix("jit/"),"0"))
        cost=(1/compiler["rate"]-1/interpreted_compiler["rate"]) if compiler and interpreted_compiler else None
        w.writerow([*key,round(before["rate"]),round(after["rate"]),round((after["rate"]/before["rate"]-1)*100,2),before["allocations"],after["allocations"],before["bytes"],after["bytes"],round(cost/saved) if cost and saved>0 else ""])
