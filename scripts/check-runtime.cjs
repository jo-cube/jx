// Closure escape, regional retirement and owned dynamic-plan boundaries.
const fs=require('node:fs');
const path=require('node:path');
const {check,root,checked}=require('./differential.cjs');
async function main() {
    for(const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/runtime.json')))) await check(test);
    for(const n of [0,1,2,8,32,128]) {
        const data={a:Array.from({length:n},(_,i)=>i),rows:Array.from({length:n},(_,i)=>({n:i,k:i%3}))};
        for(const expr of [
            '$map(a,function($x){($f:=function(){$x};$f()+1)})',
            '$map(a,function($x){($f:=function(){$x};$f())})',
            '$map(a,function($x){($f:=function($n){$n=0?$x:$f($n-1)};$f(3)+1)})',
            '($fs:=$map(a,function($x){function(){$x}});$map($fs,function($f){$f()}))',
            '$reduce(a,function($acc,$x){($f:=function(){$x};$acc+$f())},0)',
            '$filter(a,function($x){($f:=function(){$x>2};$f())})',
            '$sort(a,function($a,$b){($key:=function($x){$x};$key($a)>$key($b))})',
            '($fs:=rows#$i.function(){[$i,n]};$map($fs,function($f){$f()}))',
            '($fs:=rows^(>n).function(){n};$map($fs,function($f){$f()}))',
            '($fs:=rows{k:function(){$sum(n)}};$each($fs,function($f){$f()}))',
        ]) await check({expr,data});
    }
    for(const a of [undefined,null,false,0,1,[],[1],[1,2],[1,"x"],[[1]],{},[{}]]) {
        for(const code of [
            'function($x){$x*2}', 'function($x)<n:n>{$x*2}',
            'function($x){$x>1 ? $x*2 : $x+1}', 'function($x){$x and true}',
            'function($x){$x}', 'function($x){{"n":$x,"s":"literal"}}',
            'function($x){function(){$x}}', 'function($x){$eval("$x")}',
            'function($x){{"n":$x+1,"b":$x>1,"s":"é😀","a":[{"s":"keep"}]}}',
            'function($x)<n:o>{{"n":$x+1}}',
        ]) await check({expr:code==='function($x){function(){$x}}' ? '($f:=$eval(code);$fs:=$map(a,$f);$map($fs,function($g){$g()}))' : '($f:=$eval(code);$map(a,$f))',data:{a,code}});
    }
    console.log(`Checked ${checked()} runtime-boundary evaluations against upstream`);
}
main().catch(e=>{console.error(e);process.exitCode=1});
