// Optional differential campaign. No network or reference downloads.
const assert=require('node:assert/strict');
const path=require('node:path');
const fs=require('node:fs');
const {spawnSync}=require('node:child_process');
const upstream=path.resolve(process.argv[2]);const probe=path.resolve(process.argv[3]);
assert.equal(spawnSync('git',['-C',upstream,'rev-parse','HEAD'],{encoding:'utf8'}).stdout.trim(),'8ee4476f8a228bfc7a62979ae0a9c13a4043cd03');
const jsonata=require(path.join(upstream,'src/jsonata'));
const kinds={T1003:'TypeError',D3137:'UserError',T2001:'TypeError',T2002:'TypeError',T2003:'TypeError',T2004:'TypeError',T2006:'TypeError',T2007:'TypeError',T2008:'TypeError',T2009:'TypeError',T2010:'TypeError',T0410:'TypeError',T0411:'TypeError',T0412:'TypeError',D3070:'TypeError',D3072:'TypeError',D3030:'TypeError',D3040:'NumericRange',D1001:'NumericRange',D1002:'TypeError',D3020:'NumericRange',D3001:'NumericRange',D1009:'DuplicateKey'};
function normalized(value){return JSON.parse(JSON.stringify(value===undefined?[]:Array.isArray(value)&&value.sequence&&!value.keepSingleton?Array.from(value):[value]));}
function actual(value){return value.items_json===undefined?{error:value.error}:{items:JSON.parse(value.items_json)};}
const tests=[];
function add(expr,data,vars){tests.push({expr,input:JSON.stringify(data),...(vars?{vars}:{})});}
const atoms=[undefined,null,false,true,0,-0,2,-3.5,1e308,'3','x',[],[1],[1,2],{}];
const scalar=["{'a':$uppercase(s),'b':$uppercase(s),'c':$contains(s,'a') and $contains(s,'a')}","{'a':a>1?$uppercase(s):s,'b':$uppercase(s)}",'$number(b)*$number(b)+$number(b)',"($v:=$number(b);$v*$v+$v)",'a*a+b*b+a*b','($x:=a;$y:=b;($x*$x+$y*$y+$x*$y))','a>1?$number(b):$length(s)','a=2 or b=3','a>0 and b>0',"{'a':$string(a),'b':$number(b)}","{'k':a,'v':b,'q':a=b}",'a and b','a or b'];
for(const a of atoms)for(const b of atoms)for(const expr of scalar)add(expr,{a,b,s:'é😀'});
for(const s of ['', 'Ada', 'é中😀😀', 'ΟΣ ΟΣΑ', 'ßİı', 'a\ud800b', 'a/b', 'quote"slash\\tab\t'])for(const start of [-99,-2.5,-1,0,0.5,1,2.5,99])for(const length of [undefined,-1,0,1,3.5,99])add(length===undefined?'$substring(s,start)':'$substring(s,start,len)',{s,start,len:length});
for(const rows of [undefined,null,[],{},[{a:2,s:'Ada'}],[{a:2,s:'Ada'},{a:3,s:'é😀'}],[{a:'x'},{a:null}],[{a:[1,2]}],[[{a:1}]], [false,{},null]])for(const expr of ["$map(items,function($v){ {'a':$v.a,'s':$uppercase($v.s)} })","items.{'a':a,'s':$uppercase(s)}","($n:=2;$map(items,function($v){$v.a*$n}))","$map(items,function($v,$i,$a){ {'v':$v,'i':$i,'n':$count($a),'whole':$a} })","$filter(items,function($v){$v.a>1})",'$sum($map(items,function($v){$v.a}))','$sum($map(items,function($v){$number($v.a)}))','$sum((items[a>1]).(a*2))','$count($map(items,function($v){$v.a}))'])add(expr,{items:rows});
for(const input of ['{"a":{"x":1},"a":{},"s":"x","\\u0073":"é😀"}','{"a":1e999,"b":3,"s":"a"}',' {"a":-0,"b":0} '])for(const expr of scalar)tests.push({expr,input});
for(const expr of ["$lookup($vars.labels,id)","($t:=$vars.labels;$lookup($t,id))","{'a':$lookup($vars.labels,id),'s':$uppercase(s)}"])for(const id of [undefined,null,false,0,'a','b','unknown'])add(expr,{id,s:'Ada'},{labels:{a:'A',b:'B'}});
for(const items of [[[{},{}]], [{a:[1,2]}], [{a:[1]},{a:[2]}]])for(const expr of ['items.(1)','items.(a)','$sum(items.(a))','items.({"n":1})'])add(expr,{items});
for(const data of [[],[{a:1}],[{a:1},{a:2}]])for(const expr of ["$map($,function($v){{'n':$v.a}})","$map($,function($v){$})","$map($,function($v){$count($)})"])add(expr,data);
for(const items of [{children:[{a:1}]},{children:[{a:1},{a:2}]}].map(x=>[x,x]))for(const expr of ["$map(items,function($v){$map($v.children,function($w){$w.a})})","$map(items,function($v){{'values':$map($v.children,function($w){$w.a})}})","$map(items,function($v){[$v.children]})"])add(expr,{items});
for(const expr of ['flag ? items.(v*2) : ()','($x:=items.(v*2);$x)','items.(children.(v*2))','$sum((items[a>0]).(b*2))'])for(const flag of [true,false])for(const items of [[{a:1,b:null,v:2},{a:null,b:2,v:null}],[{a:1,b:2,v:2},{a:2,b:3,v:3}]])add(expr,{flag,items});
for(const expr of ["items[(a>0)].{'x':a}","items[($x:=a;$x>0)].{'x':a}","items[$contains(s,'a')].{'x':a}","items[($v:=s;$contains($v,'a'))].{'x':a}","items[a>0 ? $contains(s,'a') : false].{'x':a}","items['yes'].{'x':a}"])for(const items of [[],[{a:2,s:'Ada'},{a:-1,s:'beta'}],[{a:2,s:null},{a:null}]])add(expr,{items});
for(const expr of ["items[a>0][b>0].{'x':a+b,'s':$uppercase(s)}","(items[a>0])[b>0].{'x':a+b,'s':$uppercase(s)}","$sum(items[a>0][b>0].(a*b))"])for(const items of [[],[{a:1,b:2,s:'Ada'}],[{a:1,b:null,s:'Ada'},{a:null,b:2}]])add(expr,{items});
// Growing runtime surface: avoid relying on folded constant-only cases.
for(const a of atoms)for(const b of atoms)for(const expr of ['a ?: b','a ?? b','a in [b]','($x:=a;$x in [$x])'])add(expr,{a,b});
for(const expr of ['items[true]','items[true].($)','(items[true]).($)','items[true][true].($)','(items[true]).($count($))','items.(a)','items.([a])','(items[true]).(a)','a.x.y',"items[$number('bad')>0]", "$sum(items[true].(2))", "$sum((items[true]).(2))"])for(const data of [{},{items:[]},{items:[[]]},{items:[[],[]]},{items:[{a:[1,[2]]},{a:[3]}]},{items:[[{a:1},{a:2}],{a:3}]},{a:[{x:[{y:[1,2]}]},{x:[{y:3}]}]}])add(expr,data);
for(const expr of ['$reverse(a)','$distinct(a)','$sort(a)','$append(a,b)','$keys(a)','$spread(a)','$merge(a)','$zip(a,b)',"$split(s,',')","$replace(s,'a','x')",'$round(n,2)',"$pad(s,8,'_')"])for(const data of [{},{a:[3,1,1],b:[4,5],s:'a,b,a',n:1.235},{a:[{x:1},{y:2}],b:null,s:'Ada',n:-2.51}])add(expr,data);
for(const flag of [true,false])for(const expr of ["($m:=flag?a:b;{'x':$m.x,'y':$m.y})","(flag?a:b).{'x':x,'y':y}","$map(groups,function($g){$map($g.items,function($v){{'tag':$g.tag,'x':$v.x,'y':$v.y}})})"])add(expr,{flag,a:{x:1,y:2},b:{x:3},groups:[{tag:'one',items:[{x:1,y:2},{x:3}]},{tag:'two',items:[{y:4}]}]});
for(const expr of ["($$:={'x':1};$$.x)","($$:=other;$map(items,function($v){$$.x}))","(($$:={'x':1};$$.x);$$.x)","($$:=1;$)","items.($$:=$;{'x':$$.x})"])for(const data of [{},{x:2,other:{x:3},items:[{x:4},{x:5}]}])add(expr,data);
for(const expr of ["($unused:=items.name;1)","($unused:=items.(name);1)","($unused:=$map(items,function($v){$v.name});1)","($unused:=$number(a);1)","$sum(items[($v:=v;$v>0)].(b*2))"])for(const data of [{},{a:null,items:[{name:'Ada',v:2,b:3}]},{a:2,items:[{v:2,b:null},{v:null,b:3}]}])add(expr,data);
add("($x:=false;items[$x:=v>0];$x)",{items:[{v:2}]});
add("($x:=0;items[$x:=$x+1];$x)",{items:[{v:2}]});
for(const flag of [true,false])for(const data of [{a:{x:{v:1},y:{v:2}},b:{x:{v:3},y:{v:4}}},{a:[{x:{v:1}},{y:{v:2}}],b:null},{}])for(const expr of ["($m:=flag?a:b;$n:=flag?$m.x:$m.y;{'v':$n.v,'same':$n in [$n]})","($m:=flag?a:b;$map(items,function($v){{'x':$m.x,'y':$m.y}}))"])add(expr,{...data,flag,items:[1,2]});
// Rejection assertions exercise excluded semantics; inventory retains every case.
for(const expr of ['$eval(s)','$map(items,function($v){$v ~> $string()})','items[1]','items@$v.a',"{'x':a,'x':b}",'a in [a]'])add(expr,{a:1,b:2,s:'a',items:[{a:1}]});
// Shared row stages must preserve later predicate failures over mapper errors.
for(const aggregate of ['sum','count','average','min','max'])for(const predicate of ['v>0','($m:=$;$m.v>0)'])for(const a of [1,0,-2,null,true,'2',[1]])for(const b of [1,0,-2,null,true,'2',[1]])add(`$${aggregate}((items[${predicate}]).(v*b+v))`,{items:[{v:1,b:a},{v:b,b:2}]});
// Explicit field aliases preserve the same nested sequence and identity rules.
for(const data of [{},{a:{x:{y:[1,2]}}},{a:[{x:[{y:[1,2]}]},{x:[{y:[3]}]}]},{a:[[],[{x:[{y:[]},[{y:4}]]}]]}])for(const expr of ['($a:=a;$b:=$a.x;$b.y)',"($a:=a.x;$b:=$a.y;{'v':$b,'same':$b in [$b]})","($a:=a;{'v':$a.x.y,'base':$a})"])add(expr,data);
// Parameter holes and nested captures remain demanded after dead-input analysis.
for(const expr of ['$map(items,function($v){1})','$map(items,function($v,$i){$i})','$map(items,function($v,$i,$all){$all})','$map(items,function($v){$map([1,2],function($n){$v.name})})','$sum($map(items,function($v,$i){$i+1}))'])for(const data of [{},{items:[]},{items:[{name:'a'},{name:'b'}]},{items:{name:'one'}},{items:[1,[2],3]}])add(expr,data);
// Complete consumers must normalize the same field/path/native sequence values.
for(const aggregate of ['sum','count','average','min','max'])for(const argument of ['items.metrics.v0','items.(metrics.v0)','(items[true][true]).(metrics.v0)','($m:=items.metrics;$m.v0)'])for(const items of [undefined,[],[{},{}],[{metrics:{v0:3}}],[{metrics:{v0:[]}}],[{metrics:{v0:[1]}}],[{metrics:{v0:[1,2]}},{metrics:{v0:[3]}}],[{metrics:{v0:[[1,2]]}}],[{metrics:{v0:null}},{metrics:{v0:'bad'}}]])add(`$${aggregate}(${argument})`,{items});
// Immutable numeric views retain their owner and preserve checked operand types.
for(const id of [undefined,'missing','n','z','large','s','b','nil','a','o'])for(const expr of ['$lookup($vars.labels,id)*2','($n:=$lookup($vars.labels,id);$n*2+$n)','$sum($map(items,function($v){$lookup($vars.labels,$v.id)*2}))'])add(expr,{id,items:[{id},{id:'n'}]},{labels:{n:3,z:-0,large:1000000000000,s:'3',b:true,nil:null,a:[1],o:{}}});
// General IR calls: live captures, escaping environments, arity, signatures and
// native sequence boundaries. Keep failures visible in every execution mode.
for(const n of [undefined,null,0,2,'x'])for(const items of [undefined,[],[1],[1,2,3],[null,2],[[1,2]]])for(const expr of [
 '($f:=function($x){$x+n};$f(3))',
 '($make:=function($v){function($x){$v+$x}};$make(n)(3))',
 '($v:=n;$f:=function(){$v};$v:=n+1;$f())',
 '($f:=function($x)<n:n>{$x+1};$f(n))',
 '($f:=function($a,$b,$c,$d,$e){[$a,$b,$c,$d,$e]};$f(n,2,3,4,5))',
 '($f:=function($v){$v+n};$map(items,$f))',
 '$map(items,$string)', '$filter(items,$boolean)',
 '$reduce(items,function($a,$v){$a+$v},n)',
 '($f:=$substring(?,0,?);$g:=$f(?,2);$g(s))',
 '($f:=function($a){$map($a,function($v){$v})};$f(items)[])',
 '($f:=function($a){$map($a,function($v){$v})};($f(items))[])',
 '($m:=$map(items,function($v){$v});$m[])',
 'items[]', 'items.(v)[]'
])add(expr,{n,items,s:'é😀Ada'});
// Fusion may defer pure failures, but predicate writes finish before mapping.
for(const expr of ["($x:=false;$sum(items[$x:=v>0].($number($x))))","($x:=false;$sum((items[$x:=v>0]).($number($x))))"])for(const items of [[{v:1},{v:-1}],[{v:1},{v:2}]])add(expr,{items});
// Call ports and candidate contexts: capability-labelled composition controls.
function mechanism(capability,expr,data){add(expr,data);tests[tests.length-1].capability=capability;}
for(const flag of [true,false])for(const data of [
 {a:{x:1,y:2},b:{x:3,y:4}}, {a:[{x:1},{x:2}],b:{y:4}},
 {a:null,b:{x:3}}, {}, {a:{x:'bad'},b:{y:4}}
])for(const expr of [
 "($f:=function($d){$d};$m:=$f($);{'a':$m.a.x,'b':$m.b.y})",
 "($f:=function($d){$d.a};$m:=$f($);{'x':$m.x,'y':$m.y})",
 "($f:=function($a,$b){$b};$m:=$f(a,b);{'x':$m.x,'y':$m.y})",
 "($f:=function($d){$d.flag?{'a':$d.a.x,'b':$d.b.y}:{'a':$d.b.x,'b':$d.a.y}};$f($))",
 "($f:=function($d){$d.flag?$d:$d};$m:=$f($);{'a':$m.a.x,'b':$m.b.y})",
 "($f:=function($d){($g:=function($v){$v};$g($d))};$m:=$f($);{'a':$m.a.x,'b':$m.b.y})",
 "($f:=function($d)<o:o>{$d};$m:=$f($);{'a':$m.a.x,'b':$m.b.y})",
 "($f:=function($d){$d};$m:=$f(a);$m in [a])"
])mechanism('call dataflow',expr,{flag,...data});
for(const items of [[],[{v:0},{v:1},{v:2}],[{v:-1},{v:-2}],[[1,2],[3]], [null,0,true], [{v:null},{v:2}], [{v:'bad'},{v:2}]])for(const expr of [
 'items[0]', 'items[-1]', 'items[(0)]', 'items[v]', 'items[v>0?true:1]',
 'items[[0,0,-1]]', "items[[0,'x']]", 'items[v].(v+1)', '(items[[0]]).($)',
 'items[true][0]', '($f:=function($x){$x};items[$f(v)])'
])mechanism('candidate context',expr,{items});
// General semantic operations: shapes and failures belong to capability tests,
// not syntax-specific admission counters. Keep the input cross-product compact.
for(const items of [undefined,[],[{}],[{k:'a',v:2,children:[{v:1},{v:2}]},{k:'a',v:1,children:[{v:3}]}],[{k:null,v:'bad'},{k:'x',v:2}],[[{v:[1,[2]]}],{v:[3]}]])for(const expr of [
 'items[true][0]', 'items[v>0][-1]', '(items[true])[0]', 'items.children[0].v',
 'items.children[true][-1].v', 'items.(children).(v+1).($string($))',
 'items.(children[0]).v', 'items.([v]).($)', 'items.*.v', 'items.**.v',
 'items{k:v}', 'items{k:$sum(v)}', 'items.{k:v}', "items{'all':{'v':v,'n':$count($)}}",
 'items^(v,k)', 'items^(>v).k', '(items^(v))[0]',
])mechanism('general candidate stages',expr,{items});
for(const a of [undefined,null,-2,1,2.5,'x'])for(const b of [undefined,null,-1,3,'x'])for(const expr of ['[a..b]','[a..b][true][0]','[a..b].($+1)'])mechanism('range producer',expr,{a,b});
for(const expr of ['items{k:($ in [$$.items[0]])}',"items{'x':{'self':$,'same':$ in [$$.items]}}", "items{(v=1?$error('key'):k):$error('value')}"])mechanism('grouped focus',expr,{items:[{k:'a',v:2},{k:'a',v:1}]});
for(const data of [[],[{a:1}],[{a:1},{a:2}],{a:1}])for(const expr of ["{'x':{'self':$}}", "{'x':{'a':a}}", "{'x':$.(a+1)}", "{'x':{'n':$count($),'self':$}}", "($x:=1;{'v':($x:=$x+1)};$x)","($o:={'x':{'f':function(){$}}};$o.x.f())"])mechanism('constructor focus regions',expr,data);
async function main(){
 const child=spawnSync(probe,[],{input:tests.map(t=>JSON.stringify(t)).join('\n')+'\n',encoding:'utf8',maxBuffer:64*1024*1024});assert.equal(child.status,0,child.stderr);
 const answers=child.stdout.trim().split('\n').map(s=>JSON.parse(s));assert.equal(answers.length,tests.length);
 const inventory=[];let supported=0,runtime=0,compile=0;
 for(let i=0;i<tests.length;i++){
  const t=tests[i];let reference;
  try{reference={items:normalized(await jsonata(t.expr).evaluate(JSON.parse(t.input),t.vars?{vars:t.vars}:undefined))};}catch(e){assert.ok(kinds[e.code],`unclassified ${e.code} ${JSON.stringify(t)}`);reference={error:kinds[e.code]};}
  const answer=answers[i];assert.deepEqual(actual(answer.engine),reference,'engine '+JSON.stringify(t));
  let status;
  if(answer.unsupported){compile++;status='compile_rejected';}
  else {
   const rejection=answer.prototype.some(p=>p.error==='UnsupportedExpression');
   if(rejection){assert.ok(answer.prototype.every(p=>p.error==='UnsupportedExpression'),'mode-dependent rejection '+JSON.stringify(t));assert.equal(answer.complete.error,'UnsupportedExpression','complete rejection '+JSON.stringify(t));runtime++;status='shape_rejected';}
   else {for(const p of answer.prototype){assert.deepEqual(actual(p),reference,'prototype '+JSON.stringify(t));if(p.error)assert.deepEqual(p,answer.engine,'exact engine diagnostic '+JSON.stringify(t));}supported++;status='supported';}
  }
  if(status==='supported') {
   if(answer.engine_complete.error)assert.deepEqual(answer.complete,answer.engine_complete,'complete exact error '+JSON.stringify(t));
   else {assert.equal(answer.complete.present,answer.engine_complete.present,'complete presence '+JSON.stringify(t));if(answer.complete.present)assert.deepEqual(JSON.parse(answer.complete.json),JSON.parse(answer.engine_complete.json),'complete JSON '+JSON.stringify(t));}
  }
  inventory.push({...t,status,reference});
 }
 if(process.argv[4])fs.writeFileSync(process.argv[4],JSON.stringify({supported,runtime_rejected:runtime,compile_rejected:compile,tests:inventory},null,2)+'\n');
 console.log(JSON.stringify({total:tests.length,supported,runtime_rejected:runtime,compile_rejected:compile,modes:2}));
}
main().catch(e=>{console.error(e);process.exitCode=1;});
