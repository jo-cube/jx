"""Profile warmed kernels in the same binary, sequentially in both modes."""
import argparse,gzip,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument("binary");p.add_argument("directory");a=p.parse_args();root=Path(a.directory)
for label,workload,size in [("scalar","jit/arithmetic",500),("fold","jit/fold",14794),("pipeline","jit/filter_map_fold",14794),("scan","jit/dense",1048576)]:
    for native in [False,True]:
        stage="native" if native else "interpreter";stem=root/f"profile-{label}-{stage}"
        env={**os.environ,"JX_BENCH_NATIVE_ONLY":"1","JX_BENCH_FILTER":workload,"JX_BENCH_BYTES":str(size),"JX_BENCH_SAMPLE_MS":"1500"}
        env.pop("JX_BENCH_NATIVE",None)
        if native:env["JX_BENCH_NATIVE"]="1"
        with Path(str(stem)+".stderr").open("w") as stderr:
            child=subprocess.Popen([a.binary],env=env,stdout=subprocess.PIPE,stderr=stderr,text=True)
            prefix=[child.stdout.readline(),child.stdout.readline()]
            if not prefix[1].startswith(workload+","):raise RuntimeError(prefix)
            sample=subprocess.run(["sample",str(child.pid),"5","1","-file",str(stem)+".txt"],capture_output=True,text=True,check=True)
            suffix=child.stdout.read();code=child.wait()
            if code:raise RuntimeError(code)
        Path(str(stem)+".csv").write_text("".join(prefix)+suffix)
        Path(str(stem)+".sample").write_text(sample.stdout+sample.stderr)
        trace=Path(str(stem)+".txt")
        with gzip.open(str(trace)+".gz","wb") as out:out.write(trace.read_bytes())
        trace.unlink();print(label,stage,"complete",flush=True)
