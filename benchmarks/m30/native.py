"""Reuse the native same-binary comparison on selected controls, in both orders."""
import argparse, json, os, subprocess
p=argparse.ArgumentParser(); p.add_argument('binary'); p.add_argument('controls'); p.add_argument('directory'); a=p.parse_args()
for item in json.load(open(a.controls)):
    print(item['name'],flush=True)
    env={**os.environ,'JX_BENCH_FILTER':item['filter'],'JX_BENCH_BYTES':str(item['bytes'])}
    for reverse in [False,True]:
        prefix=a.directory+'/native-'+item['name']+('-reverse' if reverse else '')
        cmd=['python3','benchmarks/m26/measure.py',a.binary,prefix,'--ms','300']
        if reverse: cmd.append('--reverse')
        subprocess.run(cmd,env=env,check=True)
