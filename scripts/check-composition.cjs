// Conversions and shared invocation semantics against the pinned reference.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/composition.json')))) await check(test);
    const values = [null,true,false,0,-2.5,'','3','03','0x12','1e500','plain',[],[1],[1,2],{n:2}];
    for(const v of values) for(const expr of [
        '$string(v)','$string(v,true)','$number(v)','v & v',
        'v ~> $string()', 'v ~> $string', 'v ~> $number()', 'v ~> $number',
        '($f:=$string;$g:=function($v){$v};$f(v) & "!")',
        '($f:=$string ~> $uppercase;$f(v))',
        '($f:=function($x,$y){[$x,$y]};$f(?,v)(v))',
        '($f:=function($x,$y){[$x,$y]};$f(v,?)(v))',
        '($f:=function($x,$y){[$x,$y]};$f(?,?)(v,missing))',
        '($p:=$number(?);$p(v))',
        '$map([v,v],$string)',
        '$map([v,v],function($x){$x ~> $string})',
        '($f:=function($x){$x};v ~> $f ~> $f)',
        '($f:=function($x){$x};v ~> $f())'
    ]) await check({expr,data:{v}});
    for(const count of [0,1,2,8]) for(const values of [[1,2],['3','4'],[null],[[]],[{x:1}]]) {
        const data={rows:Array.from({length:count},(_,i)=>({v:values[i%values.length],k:i%2})),n:2};
        for(const expr of [
            '$string(rows.v)','$string(rows.v[])','$string(rows.v[true])','rows.v & ":"',
            'rows{$string(k):$string(v)}',
            'rows.{$string(k):v}',
            'rows["v=" & v = "v=1"].v',
            '$map(rows,function($r){"v=" & $r.v}) ~> $join(",")',
            '($f:=function($x,$y){[$x,$y]};$p:=$f(?,rows.v);[$p(1),$p(2)])',
            '($f:=function($x){$string($x)}~>$uppercase;$map(rows.v,$f))',
            '$lookup({"0":"zero","1":"one"},$string(rows[0].k))',
            'rows.v ~> $map($string)[]',
            'rows.v ~> $map($string)[0]',
            'rows.v ~> ($map($string))'
        ]) await check({expr,data});
    }
    for (const left of ['a[]','(a[])','rows.v[]','(rows.v[])','[1].$[]','a[]^(<$)','a^(<$)[]','a[true][]','a[]{"k":$}','a{"k":$}[]']) {
        for (const right of ['$map($string)','$map($string)[]','$map($string)[0]'])
            await check({expr:left+' ~> '+right,data:{a:[1],rows:[{v:1}]}});
    }
    // Deterministic numeric formatting, including threshold and rounding boundaries.
    let state=0x16;
    const random=()=>{state=(Math.imul(state,1664525)+1013904223)>>>0;return state/2**32;};
    for(let i=0;i<600;i++) {
        const v=(random()-0.5)*10**Math.floor(random()*620-310);
        if(!Number.isFinite(v))continue;
        for(const expr of ['$string(v)','$string([v])','v & ":"']) await check({expr,data:{v}});
    }
    for(const v of [0,Number.MIN_VALUE,1e-7,1e-6,1e20,1e21,5890840712243076,1.2345678901234567,0.9999999999999999]) {
        for(const expr of ['$string(v)','$string({"n":v},true)']) await check({expr,data:{v}});
    }
    for(const input of ['{"v":"\\u0031\\u0032.5"}','{"v":{"n":1.200,"s":"\\u0061\\/\\n\\ud800"}}','{"v":{"x":1,"\\u0078":2,"2":2,"1":1}}']) {
        for(const expr of ['$string(v)','$string(v,true)','v & ":"']) await check({expr,input});
    }
    for(const source of ['0x','0X','0o','0O','0b','0B']) for(const suffix of ['0','001','1f','9','1z','1e3','1'.repeat(80),'f'.repeat(300)]) await check({expr:'$number(v)',data:{v:source+suffix}});
    for(const expr of [
        '($f:=function($x,$y){$x+$y};$f(?,?)(?,2)(3))',
        '($f:=function($x,$y,$z){[$x,$y,$z]};$f(?,2,?)(?,3)(1))',
        '($n:=0;$f:=function($x,$y){[$x,$y]};$p:=$f(?,($n:=$n+1));[$p(2),$p(3),$n])',
        '($f:=function($x,$y){$x};$p:=$f(?,$f:=function($x,$y){"new"});$p(7))',
        '($x:=1;$f:=function($v){$v+$x};$g:=$f~>$f;$x:=2;$g(3))',
        '($p:=$keys(?);$map([{"x":1},{"y":2}],$p))',
        '($p:=$map(?,function($v){$v});$p([1])[])',
        '($p:=$map(?,function($v){$v});$p([1]))',
        '($p:=$count(?);[$p(missing),$p([1,2])])',
        '($p:=$number(?);$p("03"))',
        '($p:=$substring(?,0,?);$p(?,2)("abcd"))'
    ]) await check({expr,data:null});
    console.log(`Checked ${checked()} conversion/composition evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
