#!/usr/bin/env python3
"""Reversed independent-process controls; all timing has allocation counting off."""
import argparse,csv,hashlib,json,os,platform,statistics,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__);p.add_argument('binary',type=Path);p.add_argument('destination',type=Path);p.add_argument('--ms',type=int,default=200);p.add_argument('--samples',type=int,default=5);p.add_argument('--suite',choices=['full','phases','parallel','profiles','counters','references','references-counters','validation','typed-parallel','typed-profiles','worker-boundary','simd','simd-counters','simd-profiles','default'],default='full');a=p.parse_args()
binary=a.binary.resolve();out=a.destination.resolve();out.mkdir(parents=True,exist_ok=True)
cases=['validate/500','validate/1024','validate/10240','validate/1048576','scalar/500','scalar/1024','lookup/1024','predicate/1024']
cases += [f'project/{size}/{n}/direct' for size,n in [(500,8),(1024,8),(1024,24),(1024,32),(1024,40),(10240,80),(1048576,80)]]
cases += [f'project/1024/40/{form}' for form in ['alias','conditional','call']]+[f'nested/{n}' for n in [2,8,32]]
cases += [f'sequence/{n}/48/50/{form}' for n in [8,128,1024,8192] for form in ['numeric','group']]
cases += [f'sequence/{n}/500/50/{form}' for n in [8,1024] for form in ['numeric','group']]
cases += ['sequence/8/500/50/alias','sequence/8/500/50/callbacks','sequence/1024/500/50/callbacks','sequence/8/500/50/output','sequence/1024/500/50/output']
cases += [f'sequence/{n}/48/50/group' for n in [0,1,2]]+[f'sequence/8/500/{s}/group' for s in [0,100]]
cases += ['worker/project/500/8','worker/project/1024/40','worker/project/10240/80','worker/fold/8','worker/fold/1024']
ledger=[]
def execute(label,env,cmd=None,profile=False):
    cmd=cmd or ['/usr/bin/time','-l',str(binary)]
    fullenv={**os.environ,'ABS_MS':str(a.ms),'ABS_SAMPLES':str(a.samples),**env}
    begin=time.time()
    with (out/(label+'.csv')).open('w') as log,(out/(label+'.stderr')).open('w') as err:
        if not profile:rc=subprocess.run(cmd,env=fullenv,stdout=log,stderr=err).returncode
        else:
            proc=subprocess.Popen(cmd,env=fullenv,stdout=log,stderr=subprocess.PIPE,text=True)
            for line in proc.stderr:
                err.write(line);err.flush()
                if line.startswith('ready,'):
                    time.sleep(.4);subprocess.run(['sample',str(proc.pid),'3','-file',str(out/(label+'.sample.txt'))],stdout=err,stderr=err,check=True);break
            for line in proc.stderr:err.write(line)
            rc=proc.wait()
    ledger.append(dict(label=label,env=env,command=cmd,binary_sha256=hashlib.sha256(Path(cmd[-1]).read_bytes()).hexdigest(),exit=rc,wall=time.time()-begin))
    (out/'run.json').write_text(json.dumps(dict(binary=str(binary),sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),platform=platform.platform(),samples=a.samples,ms=a.ms,commands=ledger),indent=2)+'\n')
    if rc:raise RuntimeError(label+' failed')
    print(label,flush=True)
if a.suite=='full':
    for order in range(2):
        for mode in (['legacy','foundation','rust','serde'] if order==0 else ['serde','rust','foundation','legacy']):
            execute(f'order{order}-{mode}',dict(ABS_MODE=mode,ABS_CASE='|'.join(cases)))
elif a.suite=='phases':
    selected=['scalar/500','project/500/8/direct','project/1024/40/direct','predicate/1024','sequence/8/500/50/group','sequence/1024/500/50/group']
    for order in range(2):
        for mode in (['legacy','foundation'] if order==0 else ['foundation','legacy']):
            for phase in ['validate','prepare','prepared','values','validated','compile']:
                execute(f'{order}-{mode}-{phase}',dict(ABS_MODE=mode,ABS_PHASE=phase,ABS_CASE='|'.join(selected)))
    for order in range(2):execute(f'{order}-rust-prepared',dict(ABS_MODE='rust',ABS_PHASE='prepared',ABS_CASE='|'.join(selected)))
elif a.suite=='parallel':
    for order in range(2):
        for case in ['project/1024/40/direct','sequence/8/500/50/group','sequence/1024/500/50/group']:
            for mode in (['legacy','foundation','rust'] if order==0 else ['rust','foundation','legacy']):
                for n in ([1,2,4,8] if order==0 else [8,4,2,1]):
                    execute(f'{order}-{case.replace("/","-")}-{mode}-{n}',dict(ABS_MODE=mode,ABS_CASE=case,ABS_THREADS=str(n)))
elif a.suite=='counters':
    for order in range(2):
        for case in ['worker/project/1024/40','worker/fold/8','scalar/500','sequence/1024/48/50/numeric']:
            for mode in (['legacy','foundation','rust'] if order==0 else ['rust','foundation','legacy']):
                execute(f'{order}-{case.replace("/","-")}-{mode}',dict(ABS_MODE=mode,ABS_CASE=case))
elif a.suite=='default':
    default=binary.parents[3]/'default-harness/target/release/jx-absolute-default'
    for order in range(2):
        for case in ['worker/project/500/8','worker/project/1024/40','worker/fold/8']:
            versions=[('mixed',binary),('default',default)]
            for version,executable in (versions if order==0 else versions[::-1]):
                execute(f'{order}-{case.replace("/","-")}-{version}',dict(ABS_MODE='legacy',ABS_CASE=case),cmd=['/usr/bin/time','-l',str(executable)])
elif a.suite in ['simd','simd-counters']:
    selected=['validate/1024','scalar/500','lookup/1024','predicate/1024','project/1024/40/direct','project/1048576/80/direct','sequence/8/48/50/group','sequence/1024/48/50/numeric','sequence/8/500/50/group','sequence/1024/500/50/output','worker/project/1024/40','worker/fold/8','worker/fold/1024']
    if a.suite=='simd-counters':selected=['worker/project/1024/40','worker/fold/8']
    modes=['legacy','foundation','typed','simd','simd-copy']
    for order in range(2):
        if a.suite=='simd':
            for mode in (modes if order==0 else modes[::-1]):
                executable=binary.with_name('simd-reference') if mode.startswith('simd') else binary
                execute(f'{order}-{mode}',dict(ABS_MODE=mode,ABS_CASE='|'.join(selected)),cmd=['/usr/bin/time','-l',str(executable)])
        else:
            for case in selected:
                for mode in (['foundation','typed','simd-copy'] if order==0 else ['simd-copy','typed','foundation']):
                    executable=binary.with_name('simd-reference') if mode.startswith('simd') else binary
                    execute(f'{order}-{case.replace("/","-")}-{mode}',dict(ABS_MODE=mode,ABS_CASE=case),cmd=['/usr/bin/time','-l',str(executable)])
elif a.suite=='simd-profiles':
    for case in ['worker/project/1024/40','worker/fold/8']:
        execute(case.replace('/','-')+'-simd-copy',dict(ABS_MODE='simd-copy',ABS_CASE=case,ABS_MS='8000',ABS_SAMPLES='1'),cmd=[str(binary.with_name('simd-reference'))],profile=True)
elif a.suite=='worker-boundary':
    for order in range(2):
        for mode in (['legacy','foundation','typed'] if order==0 else ['typed','foundation','legacy']):
            for phase in (['full','borrowed-worker'] if order==0 else ['borrowed-worker','full']):
                execute(f'{order}-{mode}-{phase}',dict(ABS_MODE=mode,ABS_PHASE=phase,ABS_CASE='worker/project/500/8|worker/project/1024/40|worker/fold/8'))
elif a.suite=='references-counters':
    for order in range(2):
        for case in ['worker/project/1024/40','worker/fold/8','sequence/1024/48/50/numeric']:
            for mode in (['legacy','foundation','typed'] if order==0 else ['typed','foundation','legacy']):
                execute(f'{order}-{case.replace("/","-")}-{mode}',dict(ABS_MODE=mode,ABS_CASE=case))
elif a.suite=='references':
    selected=['scalar/500','project/1024/40/direct','worker/project/1024/40','worker/fold/8','worker/fold/1024']
    selected += [f'sequence/{n}/{size}/50/{form}' for n,size in [(8,48),(1024,48),(8,500),(1024,500)] for form in ['numeric','group']]
    selected += ['sequence/8/500/50/callbacks','sequence/8/500/50/alias']
    modes=['legacy','foundation','rust','typed','typed-vec','typed-raw','typed-serde']
    for order in range(2):
        for mode in (modes if order==0 else modes[::-1]):
            execute(f'{order}-{mode}',dict(ABS_MODE=mode,ABS_CASE='|'.join(selected)))
elif a.suite=='validation':
    for order in range(2):
        for mode in (['jx','serde_raw','serde_ignored'] if order==0 else ['serde_ignored','serde_raw','jx']):
            execute(f'{order}-{mode}',dict(ABS_MODE=mode))
elif a.suite=='typed-parallel':
    for order in range(2):
        for case in ['sequence/8/500/50/group','sequence/1024/500/50/group']:
            for n in ([1,2,4,8] if order==0 else [8,4,2,1]):
                execute(f'{order}-{case.replace("/","-")}-typed-{n}',dict(ABS_MODE='typed',ABS_CASE=case,ABS_THREADS=str(n)))
elif a.suite=='typed-profiles':
    for case in ['worker/fold/8','sequence/1024/48/50/numeric']:
        execute(case.replace('/','-')+'-typed',dict(ABS_MODE='typed',ABS_CASE=case,ABS_MS='8000',ABS_SAMPLES='1'),cmd=[str(binary)],profile=True)
elif a.suite=='profiles':
    for case in ['worker/project/1024/40','worker/fold/8','sequence/1024/48/50/numeric']:
        for mode in ['legacy','foundation','rust']:
            execute(case.replace('/','-')+'-'+mode,dict(ABS_MODE=mode,ABS_CASE=case,ABS_MS='8000',ABS_SAMPLES='1'),cmd=[str(binary)],profile=True)
# Preserve process medians and within-process sample ranges; ranges are not CIs.
rows=[]
for job in ledger:
    if a.suite in ['parallel','typed-parallel']:continue
    for r in csv.DictReader((out/(job['label']+'.csv')).open()):
        r['process']=job['label'];rows.append(r)
if rows:
    with (out/'samples.csv').open('w') as f:w=csv.DictWriter(f,fieldnames=rows[0]);w.writeheader();w.writerows(rows)
    groups={}
    for r in rows:groups.setdefault((r['case'],r['mode'],r['phase']),[]).append(r)
    summary=[]
    for (case,mode,phase),rs in groups.items():
        rates=[float(r['records'])/float(r['wall_seconds']) for r in rs]
        cpus=[float(r['cpu_seconds'])/float(r['records'])*1e9 for r in rs]
        wall=[float(r['wall_seconds'])/float(r['records'])*1e9 for r in rs]
        cpuutil=[float(r['cpu_seconds'])/float(r['wall_seconds']) for r in rs]
        rate=statistics.median(rates);r=rs[0]
        summary.append(dict(case=case,mode=mode,phase=phase,records_s=rate,wall_ns=statistics.median(wall),cpu_ns=statistics.median(cpus),cpu_cores=statistics.median(cpuutil),input_B_s=rate*float(r['input_bytes']),output_B_s=rate*float(r['output_bytes']),input_B=float(r['input_bytes']),output_B=float(r['output_bytes']),allocations=r['allocations'],allocated_B=r['allocated_bytes'],peak_B=r['peak_heap'],min_rate=min(rates),max_rate=max(rates),processes=len(set(x['process'] for x in rs))))
    with (out/'summary.csv').open('w') as f:w=csv.DictWriter(f,fieldnames=summary[0]);w.writeheader();w.writerows(summary)
