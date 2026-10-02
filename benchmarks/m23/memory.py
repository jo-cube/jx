"""Verified separate-process peak RSS; no throughput measurements overlap."""
import argparse, hashlib, json, pathlib, re, subprocess
p=argparse.ArgumentParser();p.add_argument('before');p.add_argument('after');a=p.parse_args()
rows=[]
for width in [1024,16384]:
 data={'rows':[{'id':i} for i in range(width)],'n':width}
 input=json.dumps(data,separators=(',',':'))+'\n'
 for name,expr,expected in [
  ('transient_capture','$sum($map(rows,function($r){(function(){$r.id}())+0}))',width*(width-1)//2),
  ('escaped_capture','$sum($map($map(rows,function($r){function(){$r.id}}),function($f){$f()}))',width*(width-1)//2),
  ('captured_tail','($f:=function($n,$g){$n=0?$g():$f($n-1,function(){$n})};$f(n,function(){0}))',1),
  ('sort_keys','$sum(rows^(id).id)',width*(width-1)//2),
 ]:
  for records in [1,20]:
   for tag,cli in [('before',a.before),('after',a.after)]:
    run=subprocess.run(['/usr/bin/time','-l',cli,'--',expr],input=input*records,text=True,capture_output=True,check=True)
    assert [json.loads(s) for s in run.stdout.splitlines()]==[expected]*records
    rss=int(re.search(r'(\d+)\s+maximum resident set size',run.stderr).group(1))
    rows.append(dict(workload=name,width=width,expression=expr,input_bytes=len(input)-1,records=records,version=tag,max_rss_bytes=rss));print(name,width,records,tag,rss,flush=True)
pathlib.Path('benchmarks/m23/memory.json').write_text(json.dumps(dict(binaries={tag:hashlib.sha256(pathlib.Path(cli).read_bytes()).hexdigest() for tag,cli in [('before',a.before),('after',a.after)]},method='Separate CLI processes. Peak RSS includes process, buffers and allocator high water, not exact live frame storage. Outputs checked; independent records are repeated to expose retention across evaluations.',results=rows),indent=2)+'\n')
