#!/usr/bin/env python3
"""Warmed macOS stack samples; profile CSVs are not throughput comparisons."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('binary', type=Path)
parser.add_argument('destination', type=Path)
parser.add_argument('--cases', type=Path, default=Path('benchmarks/execution/profile-cases.json'))
args = parser.parse_args()
args.destination.mkdir(parents=True, exist_ok=True)
cases = json.loads(args.cases.read_text())
metadata = dict(binary=str(args.binary.resolve()), sha256=hashlib.sha256(args.binary.read_bytes()).hexdigest(),
                warm_after_ready_seconds=1, sample_seconds=4, ordinary=True, setup_reverse=True, cases=cases)
(args.destination/'run.json').write_text(json.dumps(metadata, indent=2)+'\n')
for workload, mode in cases:
    label = workload.replace('/', '-')+'-'+mode
    # The retained diagnostic initializes stderr before the allocator baseline.
    # Otherwise the first readiness print retains 64 bytes of Rust runtime state.
    env = dict(os.environ, ARCH_FILTER=workload, ARCH_MODES=mode, ARCH_MS='1200',
               ARCH_SAMPLES='7', ARCH_PROFILE='1', ARCH_ORDINARY='1', ARCH_REVERSE='1')
    with (args.destination/(label+'.csv')).open('w') as output, (args.destination/(label+'.stderr')).open('w') as errors:
        process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=output,
                                   stderr=subprocess.PIPE, text=True)
        ready = False
        for line in process.stderr:
            errors.write(line)
            errors.flush()
            if line.startswith('measuring,') and workload in line and line.rstrip().endswith('/'+mode):
                ready = True
                break
        if not ready:
            raise RuntimeError('benchmark never reached selected profile workload: '+label)
        time.sleep(1)
        subprocess.run(['sample', str(process.pid), '4', '-file', str(args.destination/(label+'.txt'))],
                       stdout=errors, stderr=errors, check=True)
        for line in process.stderr:
            errors.write(line)
        if process.wait() != 0:
            raise RuntimeError('profile workload failed: '+label)
    print('Profiled '+label, flush=True)
