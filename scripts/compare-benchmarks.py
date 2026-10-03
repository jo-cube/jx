"""Sequential, both-order comparisons of warmed benchmark medians."""
import argparse, csv, json, os, statistics, subprocess
p=argparse.ArgumentParser()
p.add_argument('before');p.add_argument('after');p.add_argument('filters');p.add_argument('prefix');p.add_argument('--reverse',action='store_true')
a=p.parse_args();out=[[],[]]
for item in json.load(open(a.filters)):
    print(f"{item['filter']} / {item['bytes']} B",flush=True)
    env={**os.environ,'JX_BENCH_FILTER':item['filter'],'JX_BENCH_BYTES':str(item['bytes']),'JX_BENCH_SAMPLE_MS':'300'}
    for i in ([1,0] if a.reverse else [0,1]):
        run=subprocess.run([a.before if i==0 else a.after],env=env,text=True,capture_output=True,check=True)
        rows=list(csv.DictReader(run.stdout.splitlines()))
        if not rows:raise RuntimeError(f'No measurements for {item}')
        out[i].extend(rows)
fields=list(out[0][0])
for tag,rows in zip(['before','after'],out):
    with open(a.prefix+'-'+tag+'.csv','w') as f:
        w=csv.DictWriter(f,fieldnames=fields);w.writeheader();w.writerows(rows)
def group(rows):
    result={}
    for r in rows:result.setdefault((r['workload'],r['input_bytes']),[]).append(r)
    return {key:dict(rate=statistics.median(float(r['records_per_second']) for r in vs),allocations=float(vs[0]['allocations_per_record']),allocated_bytes=float(vs[0]['allocated_bytes_per_record'])) for key,vs in result.items()}
b,c=map(group,out)
with open(a.prefix+'-comparison.csv','w') as f:
    w=csv.writer(f);w.writerow(['workload','input_bytes','before_records_s','after_records_s','change_percent','before_allocations','after_allocations','before_allocated_bytes','after_allocated_bytes'])
    for k,x in b.items():
        if k not in c:continue
        y=c[k];w.writerow([*k,round(x['rate']),round(y['rate']),round((y['rate']/x['rate']-1)*100,2),x['allocations'],y['allocations'],x['allocated_bytes'],y['allocated_bytes']])
