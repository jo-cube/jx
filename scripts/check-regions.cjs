// Pure-region lowering, tree fallback, callback scopes and staged error outcomes.
const fs = require('node:fs');
const path = require('node:path');
const {check,root,checked} = require('./differential.cjs');
async function main() {
    for(const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/regions.json')))) await check(test);
    const atoms=[undefined,null,false,true,0,-0,2,-3.5,"x",[],[1],[1,2],{}];
    for(const a of atoms) for(const b of atoms) {
        const data={rows:[{a,b},{a:2,b:3}],a,b};
        for(const expr of [
            '$map(rows,function($r){$r.a+$r.b+$r.a})',
            '$map(rows,function($r,$i){$r.a*$i+$r.b+$i})',
            '$reduce(rows,function($acc,$r){$acc+$r.a+$r.b},0)',
            '$map(rows,function($r){$r.a>0 ? $r.a*$r.a : $r.b+$r.b})',
            '$map(rows,function($r){(($r.a and $r.b) or $r.a) and $r.a})',
            '$map(rows,function($r){ {"v":$r.a+$r.b,"s":($r.a+$r.b)*($r.a+$r.b)} })',
            '$sum($map(rows,function($r){$r.a*$r.b+$r.a}))',
            '$sum($map($filter(rows,function($v){$v.a>0 and $v.b>0}),function($r){$r.a*$r.b+$r.a}))',
            '$count($map($filter(rows,function($v){$v.a>0}),function($r){$r.a+$r.b}))',
            'function($r){a+a+$r.b}($)',
            'rows^(a*a+a+b*b+b).a',
        ]) await check({expr,data});
    }
    for(const rows of [undefined,null,[],{},[{}],[[{a:2,b:3}],{a:4,b:5}],[{a:2,b:3},{a:4,b:5}]]) {
        for(const expr of [
            '$sum($map(rows,function($r){$r.a*$r.b+$r.a}))',
            '$min($map(rows,function($r){$r.a*$r.b+$r.a}))',
            '$max($map($filter(rows,function($r){$r.a>0}),function($v){$v.a+$v.b}))',
            '$average($map(rows,function($r){$r.a+$r.b}))',
            '$count($map(rows,function($r){$r.a+$r.b}))',
            '$map(rows,function($r)<o:n>{$r.a+$r.b+$r.a})',
            '$map(rows,function($r){($r.a+$r.b+$r.a)})',
            '($f:=function($r,$s){$r.a+$r.b+$s};$map(rows,$f(?,2)))',
            '$map(rows,function($r){$eval("$r.a")+$r.a+$r.a})',
            '$map(rows,function($r){$error("stop")+$r.a+$r.a})',
            '$map(rows,function($r){($assert($r.a>0,"stop");$r.a+$r.a+$r.a)})',
        ]) await check({expr,data:{rows}});
    }
    console.log(`Checked ${checked()} pure-region evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1});
