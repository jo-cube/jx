#!/usr/bin/env python3
"""Matched old engine / entry foundation / current foundation; sequential only."""
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

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('entry', type=Path)
parser.add_argument('current', type=Path)
parser.add_argument('destination', type=Path)
parser.add_argument('--cases', type=Path, default=Path('benchmarks/execution/workloads.json'))
parser.add_argument('--ordinary', action='store_true')
parser.add_argument('--entry-engine', action='store_true', help='also measure legacy execution from the entry binary')
parser.add_argument('--expanded', action='store_true', help='new coverage: compare engine/current only')
parser.add_argument('--calls', action='store_true', help='enable call fixtures in entry and current; requires matched admission')
parser.add_argument('--raw', action='store_true', help='include validated scan execution controls')
args = parser.parse_args()
args.destination.mkdir(parents=True, exist_ok=True)
requested_cases = list(dict.fromkeys(json.loads(args.cases.read_text())))
# Filters are a union of substrings. A narrower redundant selector must not
# benchmark the same workload twice or produce a misleading sample count.
cases = [case for case in requested_cases
         if not any(other != case and other in case for other in requested_cases)]
versions = [('engine', args.current.resolve(), 'engine'),
            ('entry', args.entry.resolve(), 'foundation'),
            ('current', args.current.resolve(), 'foundation')]
if args.entry_engine:
    versions.insert(0, ('entry_engine', args.entry.resolve(), 'engine'))
if args.expanded:
    versions = [v for v in versions if v[0] not in ('entry', 'entry_engine')]
if args.raw:
    versions += [('entry_raw', args.entry.resolve(), 'vm_raw'), ('current_raw', args.current.resolve(), 'vm_raw')]
metadata = dict(started=time.time(), ordinary=args.ordinary, entry_engine=args.entry_engine, expanded=args.expanded, calls=args.calls, raw=args.raw, samples=7,
                milliseconds=200, cases=cases, requested_cases=requested_cases,
                executables={label: dict(path=str(p), sha256=hashlib.sha256(p.read_bytes()).hexdigest())
                             for label, p, _ in versions})
(args.destination/'run.json').write_text(json.dumps(metadata, indent=2)+'\n')
raw = []
with (args.destination/'samples.csv').open('w') as output:
    writer = None
    for order in (0, 1):
        for case in cases:
            for label, binary, mode in versions[::(-1 if order else 1)]:
                env = dict(os.environ, ARCH_FILTER=case, ARCH_MODES=mode,
                           ARCH_MS='200', ARCH_SAMPLES='7')
                env.pop('ARCH_ORDINARY', None)
                env.pop('ARCH_EXPANDED', None)
                if args.ordinary:
                    env['ARCH_ORDINARY'] = '1'
                if args.expanded or args.calls:
                    env['ARCH_EXPANDED'] = '1'
                with (args.destination/'last-process.stderr').open('w') as errors:
                    p = subprocess.run([str(binary)], env=env, text=True,
                                       stdout=subprocess.PIPE, stderr=errors, check=True)
                rows = list(csv.DictReader(io.StringIO(p.stdout)))
                assert rows, (case, label)
                for r in rows:
                    r['workload'] = r['workload'].rsplit('/', 1)[0]
                    r.update(version=label, order=order)
                    if writer is None:
                        writer = csv.DictWriter(output, fieldnames=list(r))
                        writer.writeheader()
                    writer.writerow(r)
                    raw.append(r)
                output.flush()
            print(f'Completed order {order}: {case}', flush=True)
metadata['finished'] = time.time()
(args.destination/'run.json').write_text(json.dumps(metadata, indent=2)+'\n')
groups = {}
for r in raw:
    groups.setdefault((r['workload'], r['version']), []).append(r)
with (args.destination/'summary.csv').open('w') as output:
    fields = ['workload','version','input_bytes','records_per_second','input_bytes_per_second',
              'allocations','requested_bytes','peak_live_bytes','pass_spread_percent']
    writer = csv.DictWriter(output,fieldnames=fields)
    writer.writeheader()
    for (name,version), rows in groups.items():
        assert len(rows)==14, (name,version,len(rows))
        rates = [statistics.median(float(r['records'])/float(r['seconds']) for r in rows if r['order']==o) for o in (0,1)]
        record = dict(workload=name,version=version,input_bytes=rows[0]['input_bytes'],
                      pass_spread_percent=(max(rates)/min(rates)-1)*100)
        record['records_per_second'] = statistics.median(float(r['records'])/float(r['seconds']) for r in rows)
        for f in ('allocations','requested_bytes'):
            record[f] = statistics.median(float(r[f]) for r in rows)
        record['peak_live_bytes'] = max(int(r['peak_live_bytes']) for r in rows)
        record['input_bytes_per_second'] = record['records_per_second']*int(record['input_bytes'])
        writer.writerow(record)
print(f'Summarized {len(groups)} groups, 14 samples each',flush=True)
