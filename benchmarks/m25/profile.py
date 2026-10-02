"""Sample the selected warmed evaluation, sequentially; preserves full traces."""
import argparse,csv,gzip,json,os,pathlib,subprocess
p=argparse.ArgumentParser();p.add_argument('before');p.add_argument('after');p.add_argument('directory');a=p.parse_args()
root=pathlib.Path(a.directory)
selectors=[('calls','acquisition/call_three',500),('lookup','acquisition/lookup_8192_short_raw',500),('escaped','acquisition/lookup_32768_unicode_escaped',500),('strings','acquisition/string_construct',100)]
for label,workload,size in selectors:
    for stage,exe in [('before',a.before),('after',a.after)]:
        stem=root/f'profile-{label}-{stage}'
        env={**os.environ,'JX_BENCH_FILTER':workload,'JX_BENCH_BYTES':str(size),'JX_BENCH_SAMPLE_MS':'1500'}
        with open(str(stem)+'.stderr','w') as stderr:
            child=subprocess.Popen([exe],env=env,stdout=subprocess.PIPE,stderr=stderr,text=True)
            prefix=[child.stdout.readline(),child.stdout.readline()]
            if not prefix[1].startswith(workload+','):raise RuntimeError(prefix)
            sample=subprocess.run(['sample',str(child.pid),'5','1','-file',str(stem)+'.txt'],capture_output=True,text=True,check=True)
            suffix=child.stdout.read();code=child.wait()
            if code:raise RuntimeError(code)
        (pathlib.Path(str(stem)+'.csv')).write_text(''.join(prefix)+suffix)
        (pathlib.Path(str(stem)+'.sample')).write_text(sample.stdout+sample.stderr)
        trace=pathlib.Path(str(stem)+'.txt')
        with gzip.open(str(trace)+'.gz','wb') as output:output.write(trace.read_bytes())
        trace.unlink()
        print(label,stage,'complete',flush=True)
