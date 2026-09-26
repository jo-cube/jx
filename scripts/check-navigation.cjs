// Path shape, global grouping/ordering and operator comparisons with pinned JSONata.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/navigation.json')))) await check(test);
    const shapes = [null, false, 0, 'x', [], [1], [1,2], [[1],[2]], {}, {a:null}, {a:[]}, {a:[1]}, {a:[1,2]}, {a:{b:1}}, {a:[{b:1},{b:2}]}, {a:[{}, {b:null}]}, {a:[{b:[]},{b:[1]}]}, {a:[{b:[[1]]},{b:[[2],[3]]}]}];
    for (const data of shapes) {
        for (const expr of ['*','**','*.a','**.b','a.*','a.**','a.*.b','a.**.b','a[]','a[].b','a.b[]','a[][0]','(a)[]','(a.b)[]','(a.b)[][0]','a[true][]','a[].b[true]','a.*[]','a.**[]','a[]^($)','a.b^($)','a^(b).b','a^(b)[0].b','(a.b)^(>$)','a^(b)[]','[a.b]^(>$)','a{"x":b}','a.b{"x":$}','(a{"x":b}).x','a{"x":b}.b','a{"x":b}[0]','a{"x":$count($),"sum":$sum(b)}','a ?? 9','a ?: 9','a.b ?? [1]','a.b ?: [1]','a in [a]','a.b in [a.b]','a.b{"x":$}[0]','a.b{"x":$}[true]','a{"x":b}^(b)','"a"[true].b','"a"[0].b','[][].a','[1][0][].$','[1][0+0][].$','[1][true][].$','a[][].b','a[].(b[])','[1,2][][0]','a.b in a.b','1 in a','[a] in [a]']) {
            // Upstream constructors mutate empty root arrays; jx keeps immutable input.
            if (Array.isArray(data) && data.length===0 && expr.includes('{')) continue;
            await check({expr,data,streamingError:true});
        }
    }
    for (let n=0;n<24;n++) {
        const data={a:Array.from({length:n},(_,i)=>({k:['x','y','z'][i%3], v:(i*13)%7,id:i}))};
        for (const expr of ['a^(v).id','a^(v,>id).id','a^(>v,k)[0].id','a[v>2]^(k,>v).id','a{k:v}','a{k:$sum(v)}','a^(v){k:{"sum":$sum(v),"ids":id[]}}','($v:=a[v>2];$v^(v).id[])','($f:=function($x){$x.v};a^($f($)).id)','a[$.v in [1..3]].id','[0..$count(a)]','($n:=0;a^($n:=$n+1);$n)']) await check({expr,data});
    }
    for (const input of ['{"a":1,"a":2,"b":3}','{"x":{"a":1},"\\u0078":{"a":2},"y":3}','{"10":10,"2":2,"x":0,"01":1,"0":4}']) for (const expr of ['*','**','*.a','**.a']) await check({expr,input});
    console.log(`Checked ${checked()} navigation evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
