"""Sequential warmed macOS samples of existing benchmark workloads."""
import argparse, gzip, json, os, pathlib, subprocess, time
p = argparse.ArgumentParser()
p.add_argument('binary'); p.add_argument('controls'); p.add_argument('directory')
p.add_argument('--native', action='store_true')
p.add_argument('--native-only', action='store_true')
a = p.parse_args(); root = pathlib.Path(a.directory); root.mkdir(parents=True, exist_ok=True)
for item in json.load(open(a.controls)):
    print(item['name'], flush=True)
    env = {**os.environ, 'JX_BENCH_FILTER':item['filter'], 'JX_BENCH_BYTES':str(item['bytes']), 'JX_BENCH_SAMPLE_MS':'1200'}
    if a.native: env['JX_BENCH_NATIVE']='1'
    if a.native_only: env['JX_BENCH_NATIVE_ONLY']='1'
    trace = root / ('profile-'+item['name']+'.txt')
    with open(root / (item['name']+'.csv'), 'w') as output, open(root / (item['name']+'.stderr'), 'w') as err:
        process = subprocess.Popen([a.binary], env=env, stdout=output, stderr=err)
        time.sleep(1.5)
        subprocess.run(['/usr/bin/sample',str(process.pid),'5','1','-file',str(trace)],check=True,stdout=subprocess.DEVNULL,stderr=err)
        if process.wait()!=0: raise RuntimeError(item['name'])
    with gzip.open(str(trace)+'.gz','wb') as out: out.write(trace.read_bytes())
    trace.unlink()
