"""Sequential both-order runtime comparison with identical fixture binaries."""
import argparse,csv,os,statistics,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument("before");p.add_argument("after");p.add_argument("prefix");p.add_argument("--reverse",action="store_true");p.add_argument("--ms",default="300");a=p.parse_args();data={}
for tag,binary in ([('after',a.after),('before',a.before)] if a.reverse else [('before',a.before),('after',a.after)]):
    r=subprocess.run([binary],env={**os.environ,'JX_BENCH_RUNTIME_ONLY':'1','JX_BENCH_SAMPLE_MS':a.ms},text=True,capture_output=True,check=True)
    Path(a.prefix+'-'+tag+'.csv').write_text(r.stdout);Path(a.prefix+'-'+tag+'.log').write_text(r.stderr)
    groups={}
    for row in csv.DictReader(r.stdout.splitlines()):groups.setdefault((row['workload'],row['input_bytes']),[]).append(row)
    data[tag]={k:(statistics.median(float(v['records_per_second']) for v in vs),vs[0]['allocations_per_record'],vs[0]['allocated_bytes_per_record']) for k,vs in groups.items()}
    print(tag,'complete',flush=True)
with open(a.prefix+'-comparison.csv','w') as f:
    w=csv.writer(f);w.writerow(['workload','input_bytes','before_records_s','after_records_s','change_percent','before_allocations','after_allocations','before_requested_bytes','after_requested_bytes'])
    for k,b in data['before'].items():
        c=data['after'][k];w.writerow([*k,round(b[0]),round(c[0]),round((c[0]/b[0]-1)*100,2),b[1],c[1],b[2],c[2]])
