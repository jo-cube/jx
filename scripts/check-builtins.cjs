// Fixed signatures, collection shape and callbacks against the pinned reference.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/builtins.json')))) await check(test);
    const suite = path.join(root,'tests/conformance');
    for(const row of JSON.parse(fs.readFileSync(path.join(suite,'manifest.json'))).cases) {
        if (!/^groups\/(function-|hof-)/.test(row.file) || !['supported','error'].includes(row.status) || row.phase==='compile') continue;
        const file=path.join(suite,row.file);
        const spec=JSON.parse(fs.readFileSync(file));
        const test=Array.isArray(spec)?spec[row.index]:spec;
        let expr=test.expr??fs.readFileSync(path.join(path.dirname(file),test['expr-file']),'utf8');
        if(test.bindings && Object.keys(test.bindings).length) expr='('+Object.entries(test.bindings).map(([k,v])=>`$${k}:=${JSON.stringify(v)};`).join('')+expr+')';
        let data=test.data;
        if(data===undefined && test.dataset!=null) data=JSON.parse(fs.readFileSync(path.join(suite,'datasets',test.dataset+'.json')));
        if(data===undefined) { expr=`($$:=();$__jx_result:=();$__jx_missing[$__jx_result:=(${expr})];$__jx_result)`;data=null; }
        await check({expr,data});
    }
    const values=[null,false,true,0,1,-2.5,'','a b','Straße 😀 ΣΟΣ',[],[1],[1,2],[1,1],[[1],[2]],{}, {a:1,b:2}];
    for (const v of values) for (const expr of [
        '$length(v)','$uppercase(v)','$lowercase(v)','$trim(v)','$substring(v,1,2)',
        '$substringBefore(v," ")','$substringAfter(v," ")','$contains(v," ")','$split(v," ")',
        '$join(v,"-")','$abs(v)','$floor(v)','$ceil(v)','$sqrt(v)','$power(v,2)',
        '$average(v)','$append(v,missing)','$append(v,[])','$reverse(v)','$distinct(v)',
        '$keys(v)','$spread(v)','$merge(v)','$type(v)',
        '$map(v,function($x){$x})','$filter(v,function($x){$boolean($x)})',
        '$map(v,function($x,$i,$a){[$x,$i,$count($a)]})',
        '$reduce(v,function($a,$b){$append($a,$b)},[])',
        '$each(v,function($x,$k){[$k,$x]})','$sift(v,function($x){$boolean($x)})'
    ]) await check({expr,data:{v}});
    for (const expr of ['$map(a.v,function($v){$v})','$reverse(a.v)','$distinct(a.v)','$filter(a.v,function(){true})','$append(a.v,[])','$map(a.v,function($v){[$v]})','$map(a.v,$type)']) {
        for (const v of [[],[1],[1,1],[1,2],[null],[[]],[[1],[2]]]) await check({expr,data:{a:v.map(v=>({v}))}});
    }
    // Explicit missing arguments, kept sequences, aliased builtins and nested callbacks.
    for (const expr of [
        '$length(missing)', '$substring("abc",missing)', '$substring("abc",missing,2)',
        '$substringBefore("abc",missing)', '$substringAfter("abc",missing)',
        '$power(2,missing)',
        '$map(missing,function($v){$v})', '$map(missing,missing)',
        '$each(missing,function($v){$v})', '$sift(missing,function($v){$v})',
        '$filter([1],function(){true})[]', '$keys({"a":1})[]', '$spread({"a":1})[]',
        '$distinct([1,1].$)[]', '($f:=$keys;$f({"a":1})[])',
        '$map([{"a":1}],$keys)', '$map([{"a":1},{"b":2}],$keys)',
        '$map([{"a":1}],function($v){$keys($v)})',
        '$map([1,2],function($v){$map([$v],function($x){$x})})',
        '$reduce([1,2,3],function($a,$b){missing},7)',
        '$map([1,2],function($v){$$.n})', '$each(function($v,$k){$k})',
        '$sift(function($v){true})',
        '$map([1,2],$append)', '$map([1,2],$power)',
        '$map([1],function($v,$i,$a,$unused){[$v,$i,$a,$unused]})'
    ]) await check({expr,data:{n:5}});
    for(const value of ['a😀b','\\ud800a\\udc00','ΣΟΣ','İß','a\\n\\t b','']) {
        const input='{"v":"'+value+'"}';
        for(const expr of ['$uppercase(v)','$lowercase(v)','$length(v)','$trim(v)','$split(v,"")','$substring(v,1,1)','$join($split(v,""),"")']) await check({expr,input});
    }
    for(const start of [-10,-4,-2.8,-0.8,0,0.8,1.2,4,10]) for(const length of [-2,0,0.8,1.5,5]) await check({expr:`$substring(v,${start},${length})`,data:{v:'a😀bc'}});
    for(const value of ['1/0','-1/0','0/0','missing']) for(const expr of ['$abs(V)','$floor(V)','$ceil(V)','$sqrt(V)','$power(V,2)','$append(V,[])','$type(V)','$average([V])']) await check({expr:expr.replaceAll('V',value),data:null});
    for(const expr of ['$sift({})','$sift(missing)']) await check({expr,data:null});
    for(const value of ['missing[true][]','$append(missing[true][],missing[true][])','$append(missing[true][],[null])']) for(const expr of ['$filter(V,function(){true})','$filter(V,function(){true})[]','$spread(V)','$reverse(V)','$distinct(V)','$keys(V)','$map(V,$keys)','$map(V,function($v){$type($v)})','$append(V,[])']) await check({expr:expr.replaceAll('V',value),data:null});
    for (let i=0;i<30;i++) {
        const data={rows:Array.from({length:i},(_,j)=>({n:j,category:j%2?'a':'b',name:' v '+j+' '})),factor:3};
        for(const expr of [
            '$join($map(rows[n>2]^(>n),function($r){$uppercase($trim($r.name))}),",")',
            '($k:=factor;$reduce(rows,function($a,$r){$a+$r.n*$k},0))',
            '$map(rows#$i.{"i":$i,"n":n},function($v){$v.n+$v.i})',
            'rows{category:$average(n)}',
            '$merge($map(rows,function($r){{$r.name:$r.n}}))',
            '$sift(rows{category:$sum(n)},function($v){$v>10})',
            '($f:=$map(rows,function($r){function(){$r.n}});$map($f,function($g){$g()}))'
        ]) await check({expr,data});
    }
    console.log(`Checked ${checked()} builtin evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
