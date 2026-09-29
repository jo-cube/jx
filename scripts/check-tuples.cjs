// Scoped path bindings, joins, positional stages, sorting and grouping.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/tuples.json')))) await check(test);
    const suite = path.join(root,'tests/conformance');
    for (const row of JSON.parse(fs.readFileSync(path.join(suite,'manifest.json'))).cases) {
        if (!row.file.startsWith('groups/joins/') || row.status !== 'supported') continue;
        const file = path.join(suite,row.file);
        const cases = JSON.parse(fs.readFileSync(file));
        const spec = cases[row.index];
        const expr = spec.expr ?? fs.readFileSync(path.join(path.dirname(file),spec['expr-file']),'utf8');
        const data = 'data' in spec ? spec.data : JSON.parse(fs.readFileSync(path.join(suite,'datasets',spec.dataset+'.json')));
        await check({expr,data});
    }
    const shapes = [[{a:[1,2]},{a:[3]}], [{a:[]},{a:[{b:[1]},{b:[2]}]}], null,false,0,'x',[],[1],[1,2],[[1],[2]],{}, {a:null},{a:[]},{a:[1]},{a:[1,2]},{a:[[1],[2,3]]},{a:{b:1}},{a:[{b:1},{b:2}]},{a:[{}, {b:null}]},{a:[{b:[]},{b:[1]}]},{a:[{b:[[1]]},{b:[[2],[3]]}]}];
    for (const data of shapes) for (const expr of [
        'a#$i', 'a#$i.$i', 'a#$i[]', 'a[]#$i', 'a#$i[true]', 'a#$i[0]', 'a#$i[-1]',
        'a#$i[[0,0,-1]]', 'a#$i[$i=0]', 'a[true]#$i.$i', 'a#$i[true]#$j.[$i,$j]',
        'a#$i.b#$j.[$i,$j,$]', 'a.b#$i.$i', '(a.b)#$i.$i', 'a#$i.b[0].$i',
        'a@$v.$v', 'a@$v.[$,$v]', '$@$v', '$#$i.$i', '$.$#$i.$i',
        'a#$i^(>$i).$i', 'a#$i^($i)#$j.[$i,$j]',
        'a@$a.b@$b.[$a,$b]', 'a#$i.$i[]', '(a#$i).$i',
        '$count(a#$i)', '$sum(a#$i.$i)', 'a#$i.*', 'a#$i.**.b',
        'a#$i.($i+1)', 'a#$i.(function(){$i}).($())',
        '($i:=99;[a#$i.$i,$i])'
    ]) await check({expr,data,streamingError:true});
    // Upstream tuple reduction crashes on an empty tuple stream (uncoded JS error).
    // Empty groups have an explicit local regression instead.
    for (let n=1;n<20;n++) {
        const data={a:Array.from({length:n},(_,i)=>({k:['x','y','z'][i%3],v:(i*13)%7,id:i})),b:[{k:'x',v:10},{k:'y',v:20},{k:'x',v:30}]};
        for (const expr of [
            'a#$i[v>2]#$j.{"i":$i,"j":$j,"v":v}',
            'a#$i[v>2]^(>v)#$j.{"i":$i,"j":$j,"v":v}',
            'a@$a.b@$b[$a.k=$b.k].{"id":$a.id,"sum":$a.v+$b.v}',
            'a@$a.b[$a.k=k]{k:$sum($a.v)+$sum(v)}',
            'a#$i{k:{"ids":id[],"positions":$i,"sum":$sum(v)}}',
            'a@$a.b@$b[$a.k=$b.k]#$j.[$a.id,$b.v,$j]',
            '$sum(a#$i[v>2].(v*$i))',
            '($f:=function($x){$x*2};a#$i.($f(v)+$i))',
            'a#$i[$i in [1..3]].id',
            'a#$i^(v,>$i)[[0,-1]].id'
        ]) await check({expr,data,streamingError:true});
    }
    console.log(`Checked ${checked()} scoped path evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
