// Deterministic scope, retention, call and context comparisons against the pinned source.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root, 'tests/semantics/lexical.json')))) await check(test);
    // Empty-root object construction mutates the JS input; jx preserves immutable input.
    const shapes = [null, false, 0, 'a', [1], [1,2], [[1],[2]], {}, {a:null}, {a:[]}, {a:[1]}, {a:[1,2]}, {a:[{b:1},{b:2}]}, {a:[{}, {b:null}]}, {a:[{b:[]},{b:[1]}]}];
    const values = ['$', 'a', 'a.b', 'a.b[true]', 'a[true]', 'a[0+0]', 'missing', 'null', '[a]', '{"x":a}', 'a.($)', 'a.(b)'];
    for (const data of shapes) for (const value of values) {
        for (const expr of [
            `($x:=${value};$x)`, `($x:=${value};[$x,$x])`, `($x:=${value};$count($x))`,
            `($x:=${value};$sum($x))`, `($f:=function($x){$x};$f(${value}))`,
            `($x:=${value};$f:=function(){$x};$f())`,
            `($f:=function(){$};(${value}).$f())`, `(${value}).$$`,
            `${value} ? ${value} : missing`, `$boolean(${value})`, `$not(${value})`, `$exists(${value})`,
            `($f:=$sum;$f(${value}))`, `($f:=$count;$f(${value}))`,
        ]) await check({expr,data,streamingError:true});
    }
    for (let n=0; n<32; n++) {
        const data = {a:Array.from({length:n}, (_,i)=>({b:i, c:i%3===0}))};
        for (const expr of [
            '($x:=a[c].b;{"sum":$sum($x),"count":$count($x),"again":$x})',
            '($f:=function($x){$x.b+1};a.$f($))',
            '($f:=function($x){function($y){$x+$y}};$a:=$f(1);$b:=$f(2);a.[$a(b),$b(b)])',
            '($n:=0;$x:=a[$n:=$n+1][-1];{"calls":$n,"x":$x,"again":$x})',
            '($n:=0;$f:=function($x){[$count($x),$count($x)]};$r:=$f(a[$n:=$n+1]);{"calls":$n,"r":$r})',
            '($sum:=function($x){$count($x)};$sum(a.b))',
        ]) await check({expr,data});
    }
    console.log(`Checked ${checked()} lexical evaluations against upstream`);
}
main().catch(error => {console.error(error);process.exitCode=1;});
