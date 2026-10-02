"""Separate-process CLI peak RSS; outputs checked, no benchmark timing overlap."""
import hashlib, json, pathlib, re, subprocess
cli=pathlib.Path('target/release/jx')
rows=[]
workloads=[
 ('dynamic_literal','$eval(code)',{'code':'{"out":{"n":7,"a":[1,2,3]}}'},[{"out":{"n":7,"a":[1,2,3]}}]),
 ('dynamic_closure_map','($f:=$eval(code);$map(a,$f))',{'code':'function($x){$x*2}','a':list(range(1024))},list(range(0,2048,2))),
 ('dynamic_tail','($f:=$eval(code);$f(n,0))',{'code':'($f:=function($n,$a){$n=0?$a:$f($n-1,$a+1)};$f)','n':100000},[100000]),
]
for name,expr,data,expected in workloads:
 for records in [1,100]:
  run=subprocess.run(['/usr/bin/time','-l',str(cli),'--',expr],input=(json.dumps(data,separators=(',',':'))+'\n')*records,capture_output=True,text=True,check=True)
  assert [json.loads(s) for s in run.stdout.splitlines()]==expected*records
  rss=int(re.search(r'(\d+)\s+maximum resident set size',run.stderr).group(1))
  rows.append(dict(workload=name,expression=expr,input_bytes=len(json.dumps(data,separators=(',',':'))),parameters={**{k:v for k,v in data.items() if k!='a'},**({'a_length':len(data['a'])} if 'a' in data else {})},records=records,max_rss_bytes=rss))
  print(name,records,rss,flush=True)
pathlib.Path('benchmarks/m22/memory.json').write_text(json.dumps(dict(cli_sha256=hashlib.sha256(cli.read_bytes()).hexdigest(),method='Separate /usr/bin/time -l CLI processes. Compact output verified. RSS includes executable, input/output buffers and allocator high water; it is not live retained or per-frame storage.',results=rows),indent=2)+'\n')
