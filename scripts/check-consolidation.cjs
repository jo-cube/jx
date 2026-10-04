// Traversal, pure-key retention, grouping and escaping-capture regressions.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/consolidation.json')))) await check(test);
    const shapes=[null,[],{}, {id:null}, {id:[]}, {id:[[1],[2]]}, [{id:1},{id:[2,3]}], [[],[null,{id:[[]]}]]];
    for (let depth of [0,1,4,16,64]) for(let data of shapes) {
        for(let i=0;i<depth;i++) data=[data];
        for(const expr of ['id','$.id','id[]','$lookup($,"id")','**.id','*.id','id[-1]','[id]','$count(id)']) await check({expr,data});
    }
    for(let n of [0,1,2,3,8,32,128]) {
        const data={rows:Array.from({length:n},(_,id)=>({id,k:id%3,label:['é','😀','a'][id%3],nested:{v:n-id}}))};
        for(const expr of ['rows^(nested.v).id','rows^(k,>nested.v).id','rows^(label,k).id','rows^(k,1/0).id','rows^(nested.v,$error("untaken")).id','rows#$i^(k,>$i).id','rows#$i^($v:=k,$v).id','($n:=0;rows^($n:=$n+1).id;$n)','($n:=0;rows^(k,$n:=$n+1).id;$n)','rows^($eval("k")).id','rows{k:$sum(id)}','rows{label:id}','rows{k&"":id,"0":id}','$map(rows,function($r){(function(){$r.id}())+0})','$map(rows,function($r){function($v){function(){$v+$r.id}}(3)}) ~> $map(function($f){$f()})']) await check({expr,data});
    }
    for(const n of [31,32,33,64,128]) {
        const rows=Array.from({length:n},(_,i)=>`{"k":"k${i}","v":${i}}`);
        rows.push('{"k":"k0","v":1000}','{"k":"\\u006b0","v":1001}','{"k":"😀","v":1002}','{"k":"\\ud83d\\ude00","v":1003}','{"k":"\\ud800","v":1004}','{"k":"\\ud800","v":1005}');
        const input='{"rows":['+rows.join(',')+']}';
        for(const expr of ['rows{k:v}','rows{k:$sum(v)}','rows#$i{k:{"v":v,"index":$i}}','rows{k:v,"k0":v}']) await check({expr,input});
    }
    console.log(`Checked ${checked()} consolidation evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
