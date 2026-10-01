// Compiled signatures, partial lambdas and tail execution against JSONata 2.2.0.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/functions.json')))) if(!test.localOnly) await check(test);
    const values = ['missing','null','true','2','"2"','[]','[1]','[1,2]','[1,"2"]','[[1]]','{}','function($x){$x}'];
    const signatures = ['n','s','b','l','o','a','j','x','f','a<n>','a<s>','a<a<n>>','f<n:n>','(ns)','(af)','n?','n-'];
    for(const sig of signatures) for(const value of values) {
        for(const expr of [`function($x)<${sig}:x>{$x}(${value})`,`($f:=function($x)<${sig}:x>{$x};$p:=$f(?);$p(${value}))`]) {
            // Functions have no JSON encoding: inspect their type instead.
            await check({expr:value.startsWith('function') ? `$type(${expr})` : expr,data:7});
        }
    }
    for(const sig of ['n-n','s?n','n+n','n?s?','n+s-','a<n>+','(ns)+b?']) {
        for(const args of ['','1','1,2','1,2,3','"s",1','1,"s"','missing,2','[1],[2]','true,false']) {
            await check({expr:`function($a,$b,$c)<${sig}:a>{[$a,$b,$c]}(${args})`,data:'focus'});
        }
    }
    for(const n of [0,1,2,31,64,1000,10001]) {
        for(const expr of [
            `($f:=function($n,$a)<nn:n>{$n=0?$a:$f($n-1,$a+1)};$f(${n},0))`,
            `($f:=function($n){$n=0?false:$g($n-1)};$g:=function($n){$n=0?true:$f($n-1)};$f(${n}))`,
            `($f:=function($n,$g){$n=0?$g():$f($n-1,$n=3?function(){$n}:$g)};$f(${n},function(){0}))`,
        ]) await check({expr,data:null});
    }
    console.log(`Checked ${checked()} function-runtime evaluations against upstream`);
}
main().catch(e=>{console.error(e);process.exitCode=1;});
