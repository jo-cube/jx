// Demanded ancestry and selective updates against the pinned reference.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/structure.json')))) await check(test);
    for (const size of [0,1,2,8,32]) {
        const data = {groups:Array.from({length:size},(_,i)=>({id:i,rows:Array.from({length:3},(_,n)=>({n,kind:n%2?'odd':'even'}))})),keep:{text:'untouched'}};
        for (const expr of [
            'groups.rows.%.id', 'groups.rows.n.%.kind', 'groups.rows.n.%.%.id',
            'groups.rows[%.id % 2 = 0].n', '(groups.rows)[%.id>0].n',
            'groups.(rows.n).%.kind', 'groups.rows.n^(>%.%.id,%.n)',
            'groups.rows.n^(%.n)[%.kind="odd"].%.%.id',
            'groups.rows# $i.{"n":n,"id":%.id,"i":$i}',
            'groups.rows@$r[%.id>0].{"id":%.id,"n":$r.n}',
            'groups.rows.n.( $r:=%;function(){ $r.kind } )()',
            'groups.rows.%.id{"id":$sum($)}',
            '$ ~> |groups.rows[n>0]|{"n":n+1},"kind"|',
            '$ ~> |groups.rows[%.id>0]|{"n":n+1}|',
            '$ ~> |**[n>=0]|{"n":n+1}|',
            '$ ~> |groups|{"total":$sum(rows.n)}|',
            '$ ~> |(groups.rows)[0]|{"first":true}|',
            '$ ~> |groups.rows^(>n)[0]|{"first":true}|',
            '$ ~> |groups.rows|{"value":$string(n)},$keys($)[$="n"]|',
            '($t:=|groups.rows|{"n":n+1}|;$ ~> $t ~> $t)',
            '$clone($)', '($c:=$clone($);[$c=$,$c.groups in [groups]])'
        ]) await check({expr,data});
    }
    const rows = [{n:1},{n:2}];
    for(const a of [undefined,null,{},[],rows,[rows],{rows}])for(const b of [undefined,null,{},[],rows,[rows],{rows}]) {
        const data={a,b};
        for(const expr of [
            'a.rows.%.rows.n', 'a.rows.n.%', 'a.*.%', 'a.rows[%.rows.n>0].n',
            '$ ~> |a.rows|{"n":n+1}|', '$ ~> |a|{"extra":b}|', '$clone($)'
        ])await check({expr,data});
    }
    for(const keys of [['x','y'],['2','1','x'],['é','\\u00e9','x'],['__proto__','constructor']]) {
        const input='{'+keys.map((key,i)=>JSON.stringify(key)+':'+JSON.stringify({n:i})).join(',')+'}';
        for(const expr of ['* .%.($keys($))', '$clone($)', '$ ~> |*|{"n":n+1}|'])await check({expr,input});
    }
    for(const width of [1,2,8,32]) {
        const data={rows:Array.from({length:width},(_,n)=>({n,a:{x:n+1}}))};
        for(const expr of [
            '$ ~> |[rows[0],rows[0]]|{"n":n+1}|',
            '$ ~> |[rows[0],rows[0].a]|{"total":$sum(a.x),"x":x+1}|',
            '$ ~> |[rows[0].a,rows[0]]|{"total":$sum(a.x),"x":x+1}|',
            '($t:=|rows|{"n":n+1}|;[$t($),$t($),$])',
            '$ ~> |rows|{"a":{"x":a.x+1}},"n"|',
            '$clone([rows,rows])'
        ])await check({expr,data});
    }
    console.log(`Checked ${checked()} parent/clone/transform evaluations against upstream`);
}
main().catch(error => {console.error(error);process.exitCode=1;});
