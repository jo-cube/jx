// Constant data, indexed lookup and identity against pinned JSONata.
const {check, checked} = require('./differential.cjs');
async function main() {
    const atoms=['null','true','false','0','-0','7','(1/0)','(0/0)','"x"','"\\ud800"','()'];
    for (const left of atoms) for (const right of atoms) {
        for (const op of ['+','=','in','and','??']) await check({expr:`${left} ${op} ${right}`,data:null});
        await check({expr:`[${left},[${right}]]`,data:null});
    }
    const shapes=[null,{},[],[{},{}],{key:'a'},{key:'unknown'},{key:''},{key:'\ud800'},{key:3},{key:['a']},[{key:'a'}],[{key:'a'},{key:'b'}]];
    for (const data of shapes) {
        for (const expr of [
            '$lookup({"a":1,"b":[2],"":3,"\\ud800":4,"undefined":5},key)',
            '$lookup({"a":{"n":1},"b":[[2]]},key)',
            '($t:={"a":1,"b":2};$lookup($t,key))',
            '$lookup({"a":1},$.key)', '$lookup({"a":1},(key))',
            '$lookup([{"a":[]},{"a":[1]},{"a":[[2]]}],"a")',
            '($f:=$lookup;$f("a"))', '$lookup()', '$lookup({},true)',
            '($f:=function(){[1]};$f() in [$f()])',
            '($x:=[[1]];$x[0] in $x)',
            '($x:={"a":{"n":1}};$x.a in [$x.a])',
            '($f:=function(){$lookup({"a":{"n":1}},"a")};$f() in [$f()])',
            '($exists:=function($x){true};() ?? 7)',
            '[([1,2]),[3,4],true ? [5] : []]',
            'a[0+0][0]', 'a[0][0]',
        ]) await check({expr,data});
    }
    for (const width of [8,128,1024]) {
        const object=Object.fromEntries(Array.from({length:width},(_,i)=>[`k${i}`,{n:i}]));
        for(const key of ['k0',`k${width-1}`,'absent',null,3]) await check({expr:`$lookup(${JSON.stringify(object)},key)`,data:{key}});
    }
    for(const input of ['{"key":"a","key":"b"}','{"key":"a","\\u006bey":"b"}']) await check({expr:'$lookup({"a":1,"b":2},key)',input});
    console.log(`Checked ${checked()} compiler evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
