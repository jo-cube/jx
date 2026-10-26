#!/usr/bin/env python3
"""Compare preserved engine executables in alternating independent processes."""
import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('baseline', type=Path)
p.add_argument('candidate', type=Path)
p.add_argument('destination', type=Path)
p.add_argument('--cases', default='scalar/500|lookup/1024|predicate/1024|project/500/8/direct|project/1024/40/direct|project/1024/40/alias|project/1024/40/conditional|project/1024/40/call|project/1048576/80/direct|nested/32|sequence/0/48/50/group|sequence/1/48/50/group|sequence/2/48/50/group|sequence/8/48/50/numeric|sequence/1024/48/50/numeric|sequence/8/500/0/group|sequence/8/500/50/group|sequence/8/500/100/group|sequence/1024/500/50/group|sequence/8/500/50/alias|sequence/8/500/50/callbacks|sequence/1024/500/50/callbacks|sequence/8/500/50/output|sequence/1024/500/50/output|worker/project/500/8|worker/project/1024/40|worker/fold/8|worker/fold/1024')
p.add_argument('--extra-cases', default='')
p.add_argument('--ms', type=int, default=200)
p.add_argument('--samples', type=int, default=5)
p.add_argument('--rounds', type=int, default=2)
p.add_argument('--phase', default='full')
p.add_argument('--mode', default='foundation')
p.add_argument('--references', action='store_true')
p.add_argument('--simd-reference', type=Path)
a = p.parse_args()
if a.extra_cases:
    a.cases += "|" + a.extra_cases
out = a.destination.resolve()
out.mkdir(parents=True, exist_ok=True)
versions = [('entry', a.baseline.resolve(), a.mode), ('candidate', a.candidate.resolve(), a.mode)]
if a.references:
    versions += [('legacy', a.baseline.resolve(), 'legacy'), ('typed', a.baseline.resolve(), 'typed')]
if a.simd_reference:
    versions.append(('simd-copy', a.simd_reference.resolve(), 'simd-copy'))
ledger, rows = [], []
for repeat in range(a.rounds):
    for label, binary, mode in versions if repeat % 2 == 0 else reversed(versions):
        name = f'{repeat}-{label}'
        env = dict(ABS_CASE=a.cases, ABS_MODE=mode, ABS_PHASE=a.phase, ABS_MS=str(a.ms), ABS_SAMPLES=str(a.samples))
        cmd = ['/usr/bin/time', '-l', str(binary)]
        begin = time.time()
        with (out / f'{name}.csv').open('w') as stdout, (out / f'{name}.stderr').open('w') as stderr:
            result = subprocess.run(cmd, env={**os.environ, **env}, stdout=stdout, stderr=stderr)
        ledger.append(dict(label=name, command=cmd, env=env, sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), exit=result.returncode, seconds=time.time()-begin))
        (out / 'run.json').write_text(json.dumps(ledger, indent=2) + '\n')
        result.check_returncode()
        with (out / f'{name}.csv').open() as f:
            for row in csv.DictReader(f):
                if a.phase == 'compile' and row['phase'] != 'compile':
                    raise ValueError('worker modes do not measure compilation; select the equivalent core expression')
                row.update(version=label, process=name)
                rows.append(row)
        print(name, flush=True)
with (out / 'samples.csv').open('w') as f:
    w = csv.DictWriter(f, fieldnames=rows[0]); w.writeheader(); w.writerows(rows)
groups = {}
for row in rows:
    groups.setdefault((row['case'], row['version']), []).append(row)
summary = []
for (case, version), observations in groups.items():
    first = observations[0]
    rates = [float(r['records']) / float(r['wall_seconds']) for r in observations]
    cpu = [float(r['cpu_seconds']) / float(r['records']) * 1e9 for r in observations]
    rate = statistics.median(rates)
    processes = {}
    for row, r in zip(observations, rates): processes.setdefault(row['process'], []).append(r)
    summary.append(dict(case=case, version=version, phase=first['phase'], records_s=rate, cpu_ns=statistics.median(cpu), input_B_s=rate*float(first['input_bytes']), output_B_s=rate*float(first['output_bytes']), allocations=float(first['allocations']), allocated_B=float(first['allocated_bytes']), peak_B=float(first['peak_heap']), process_rates={k:statistics.median(v) for k,v in processes.items()}))
(out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
for case in dict.fromkeys(r['case'] for r in summary):
    data = {r['version']:r for r in summary if r['case']==case}
    b, c = data['entry'], data['candidate']
    print(f"{case}: {b['records_s']:,.0f} -> {c['records_s']:,.0f} ({c['records_s']/b['records_s']-1:+.1%}), alloc {b['allocations']} -> {c['allocations']}")
