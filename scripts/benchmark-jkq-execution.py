#!/usr/bin/env python3
"""Matched actual jkq worker / foundation-adapted worker; no broker or sibling writes."""
import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import statistics
import subprocess
import time
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('binary',type=Path)
p.add_argument('destination',type=Path)
p.add_argument('--entry',type=Path,help='also compare the saved entry foundation')
p.add_argument('--cases',type=Path,help='JSON list of exact workload names')
a=p.parse_args();a.destination.mkdir(parents=True,exist_ok=True)
cases=[f'project/{size}/{fields}/{form}' for size,fields in [(500,8),(1024,40),(10240,80)] for form in ['direct','alias','conditional']]
cases += [f'array/{n}/{form}' for n in [8,1024] for form in ['direct','lexical','map','fold']]+['pass','validate','lookup']
if a.cases:
 cases=json.loads(a.cases.read_text())
 assert isinstance(cases,list) and cases and all(isinstance(c,str) for c in cases)
metadata=dict(binary=str(a.binary.resolve()),sha256=hashlib.sha256(a.binary.read_bytes()).hexdigest(),cases=cases,started=time.time(),samples=7,milliseconds=200,latency_samples=1024,ordinary=True)
(a.destination/'run.json').write_text(json.dumps(metadata,indent=2)+'\n')
versions=[('reference',a.binary.resolve(),'reference'),('foundation',a.binary.resolve(),'foundation')]
if a.entry:
 versions[1:1]=[('entry_reference',a.entry.resolve(),'reference'),('entry',a.entry.resolve(),'foundation')]
 metadata['entry']=dict(binary=str(a.entry.resolve()),sha256=hashlib.sha256(a.entry.read_bytes()).hexdigest())
 (a.destination/'run.json').write_text(json.dumps(metadata,indent=2)+'\n')
rows=[]
with (a.destination/'samples.csv').open('w') as output:
 writer=None
 for order in [0,1]:
  for case in cases:
   for version,binary,mode in versions[::(-1 if order else 1)]:
    env=dict(os.environ,JKQ_EXACT='1',JKQ_FILTER=case,JKQ_ENGINE=mode,JKQ_SAMPLES='7')
    r=subprocess.run([str(binary)],env=env,text=True,capture_output=True,check=True)
    records=list(csv.DictReader(io.StringIO(r.stdout)));assert len(records)==7,(case,mode,len(records))
    for record in records:
     record.update(order=order,version=version)
     if writer is None:writer=csv.DictWriter(output,fieldnames=list(record));writer.writeheader()
     writer.writerow(record);rows.append(record)
    output.flush()
   print(f'Completed {order}: {case}',flush=True)
metadata['finished']=time.time();(a.destination/'run.json').write_text(json.dumps(metadata,indent=2)+'\n')
groups={}
for r in rows:groups.setdefault((r['workload'],r['version']),[]).append(r)
fields=['workload','version','input_bytes','records_per_second','input_bytes_per_second','allocations','requested_bytes','peak_live_bytes','p50_ns','p95_ns','p99_ns','pass_spread_percent']
with (a.destination/'summary.csv').open('w') as output:
 writer=csv.DictWriter(output,fieldnames=fields);writer.writeheader()
 for (name,version),values in groups.items():
  assert len(values)==14
  med=lambda field:statistics.median(float(r[field]) for r in values)
  rates=[statistics.median(float(r['records_per_second']) for r in values if r['order']==o) for o in [0,1]]
  record=dict(workload=name,version=version,input_bytes=med('input_bytes'),records_per_second=med('records_per_second'),allocations=med('allocations'),requested_bytes=med('requested_bytes'),peak_live_bytes=max(int(r['peak_live_bytes']) for r in values),p50_ns=med('p50_ns'),p95_ns=med('p95_ns'),p99_ns=med('p99_ns'),pass_spread_percent=(max(rates)/min(rates)-1)*100)
  record['input_bytes_per_second']=record['input_bytes']*record['records_per_second'];writer.writerow(record)
print(f'{len(groups)} groups, 14 samples/group')
