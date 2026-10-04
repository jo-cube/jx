// Everyday helpers: decimal ties, UTF-16, stable callbacks and sequence shape.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/helpers.json')))) if (!test.localOnly) await check(test);
    for (const n of [0,0.1,0.5,1.25,1.35,2.345,4.525,-4.525,12450,6.022e-23,1e21,1e-200,1e200]) {
        for (const p of [-400,-25,-2,-1,0,1,2,24,100,400,10001,-10001,0.5]) {
            await check({expr:'$round(n,p)',data:{n,p}});
            await check({expr:`$round(${n},${p})`,data:null});
        }
    }
    let seed = 0x19;
    function random() { seed = (Math.imul(seed,1664525)+1013904223)>>>0; return seed/2**32; }
    for(let i=0;i<500;i++) {
        const n=(random()-0.5)*10**Math.floor(random()*620-320);
        const p=Math.floor(random()*50-25);
        await check({expr:'$round(n,p)',data:{n,p}});
        await check({expr:`$round(${n},${p})`,data:null});
    }
    for(const n of ['1/0','0/0','-1/0']) for(const p of [-3,0,3]) await check({expr:`$boolean($round(${n},${p}))`,data:null});
    for (const text of ['', 'abc', 'a😀', '𝄞💩', 'a\ud800b', 'a\n"b']) {
        for (const pattern of ['', ' ', '-+', '💩', 'a💩', '\udc00']) for (const width of [-8,-5.7,0,1,5.7,8]) {
            await check({expr:'$pad(text,width,pattern)',data:{text,width,pattern}});
        }
        for(const name of ['encodeUrl','encodeUrlComponent','decodeUrl','decodeUrlComponent','base64encode','base64decode']) if (!(name === "base64decode" && [...text].some(ch => ch.charCodeAt(0)>255))) await check({expr:`$${name}(text)`,data:{text}});
    }
    for(const v of [null,false,0,'a',[],[null],[3,1,2,1],['z','a','😀','￿'],[false,true],[[1],[2]], [{k:1,id:'a'},{k:0,id:'b'},{k:1,id:'c'}]]) {
        for(const expr of ['$sort(v)','$sort(v,function(){[]})','$sort(v,function(){missing})','$sort(v,function($a,$b){$a.k>$b.k})','$single(v)','$single(v,$boolean)','$single(v,function($v,$i,$a){$i=$count($a)-1})','$zip(v,1,2,3)','$zip(v,missing)','$string($zip(v,v))']) await check({expr,data:{v}});
    }
    for(const text of ['%', '%2f%3F%23%20%C3%A9', '%C0%AF', '%ED%A0%80', '%F0%9F', '%F4%90%80%80', 'abc+def', 'Zh', 'A', 'YQ==Yg==', ' Z g ! ', '_w', 'a=b']) {
        for(const expr of ['$decodeUrl(text)','$decodeUrlComponent(text)','$base64decode(text)']) await check({expr,data:{text}});
    }
    for(const expr of [
        "($n:=0;$zip($n:=$n+1,$n:=$n+1,$n:=$n+1))",
        "($n:=0;$single([1,2],function($v){($n:=$n+1;$v=2)});$n)",
        "($key:='n';$sort(a,function($a,$b){$lookup($a,$key)>$lookup($b,$key)}))",
        "($f:=$pad(?,-3,'0');$map(a.n,function($n){$f($string($n))}))",
        "a[n>0]{kind:$round($sum(n),1)}",
        "($assert($count(a)>0,'empty');$single(a,nothing))",
        "/* comment */ $map(a,function($r){$pad($string($round($r.n,2)),-5,'0')})",
        "unknown(function)","[1,2,3]{'num':$}[true]", "/* unterminated"
    ]) await check({expr,data:{a:[{n:1.25,kind:'a'},{n:4.525,kind:'b'}]}});
    console.log(`${checked()} helper differential comparisons passed`);
}
main().catch(error => {console.error(error);process.exitCode=1;});
