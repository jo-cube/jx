// Numeric regions, shared loads and fallback behavior against pinned JSONata.
const {check, checked} = require('./differential.cjs');
async function main() {
    const numeric = '((x+y)*(x-y)+(x*x+y*y))/(x+1)-y*3';
    const expressions = [numeric, `(${numeric})>0`, `x>y ? (${numeric}) : 0`,
        `{"n":${numeric}}`, 'x+x+x+x', '-(x*x+x)-x', '(x*x+x)%y',
        '(x+x+x)/0', '(x-x+x-x)/0', '((x+x)+x)/0+x',
        '((x-x)+(y-y))/0+x', '(x+x+x)<y', '(x+x+x)>=y', '(x+x+x)/0<y', '(x-x+x-x)/0<y',
        '((x<y)<x)<y', 'missing+missing+missing+missing+x'];
    const atoms = [undefined, null, false, true, 0, -0, 1, -2, 3.5, 1e308, 1e-300, 'x', [], [1], [1,2], {}];
    for (const x of atoms) for (const y of atoms) {
        const data = {x,y};
        for (const expr of expressions) await check({expr,data});
    }
    for (const data of [null, [], [{x:2,y:3}], [{x:2,y:3},{x:4,y:1}], {x:[1],y:2}]) {
        for (const expr of [numeric, numeric.replaceAll('x','$.x'), 'x+x+x+x']) await check({expr,data});
    }
    for (const rows of [[],[{}],[{x:2,y:3}],[{x:2,y:3},{x:4,y:1}],[{x:2,y:3},{x:null,y:1}]]) {
        for (const expr of [`rows.(${numeric})`, `rows.{"n":${numeric}}`,
            '$sum(rows[(x*2+x+1)>12].x)', '$sum(rows.(x*x+x+x))',
            '($f:=function(){rows.(x*x+x+x)};$f())']) await check({expr,data:{rows},streamingError:true});
    }
    for (const input of ['{"x":1,"\\u0078":7,"y":3}', '{"x":1e999,"y":3}', '{"x":-0,"y":0}']) {
        for (const expr of expressions) await check({expr,input});
    }
    console.log(`Checked ${checked()} execution evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
