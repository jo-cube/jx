"""Sequential warmed runtime profiles; allocation measurements run separately."""
import argparse,gzip,os,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument("before");p.add_argument("after");p.add_argument("directory");a=p.parse_args();root=Path(a.directory)
for label,size in [("transient",50102),("dynamic_scalar",50102),("strings",50102),("planned",50102)]:
    for tag,binary in [("before",a.before),("after",a.after)]:
        stem=root/f"profile-{label}-{tag}"
        env={**os.environ,"JX_BENCH_RUNTIME_ONLY":"1","JX_BENCH_FILTER":"runtime/"+label,"JX_BENCH_BYTES":str(size),"JX_BENCH_SAMPLE_MS":"1500"}
        with Path(str(stem)+".stderr").open("w") as stderr:
            child=subprocess.Popen([binary],env=env,stdout=subprocess.PIPE,stderr=stderr,text=True)
            prefix=[child.stdout.readline(),child.stdout.readline()]
            if not prefix[1].startswith("runtime/"+label+","):raise RuntimeError(prefix)
            result=subprocess.run(["sample",str(child.pid),"5","1","-file",str(stem)+".txt"],capture_output=True,text=True,check=True)
            rest=child.stdout.read();code=child.wait()
            if code:raise RuntimeError(code)
        Path(str(stem)+".csv").write_text("".join(prefix)+rest)
        Path(str(stem)+".sample").write_text(result.stdout+result.stderr)
        trace=Path(str(stem)+".txt")
        with gzip.open(str(trace)+".gz","wb") as out:out.write(trace.read_bytes())
        trace.unlink();print(label,tag,"complete",flush=True)
