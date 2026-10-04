// Optional deterministic constructor comparisons against the pinned JSONata source.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root, 'tests/semantics/constructors.json')))) await check(test);
    const shapes = [null, false, 0, 'a', [], [1], [1,2], [[1],[2]], [{a:1},{a:2}], {}, {a:[]}, {a:[1]}, {a:[[1],[2]]}, {a:[{b:1},{b:2}]}, {a:[{b:[]},{b:[1]}]}, {a:[{}, {b:null}]}];
    const arguments = ['$', 'a', 'a.b', 'a.b[true]', 'a[$>1]', 'a.[b]', 'a.([b])', 'a.{"b":b}', '[a]', '[[a]]', '([a])', '[1,[2]]', '$sum(a.b)', '$count(a.b)'];
    for (const data of shapes) for (const argument of arguments) {
        for (const expr of [`[${argument}]`, `[[${argument}]]`, `[0,${argument},null]`, `{"x":${argument}}`, `{"x":${argument}}.x`, `$count([${argument}])`, `[${argument}][0]`, `[${argument}][0+0]`, `[${argument}].$`]) await check({expr,data,streamingError:true});
    }
    const constructed = ['[]','[1]','[1,2]','[[1],[2]]','[{"a":[1]},{"a":[2]}]','{"a":[1,2]}','{"a":[[1]]}'];
    for (const data of shapes.slice(0,9)) for (const lhs of constructed) for (const suffix of ['.$','.a','.a[true]','[0]','[0+0]','[-1]','[true]','.[a]','.[a].$','.([a])','.{"v":$}','.({"v":$})','.[$]','.([[$]])']) await check({expr:lhs+suffix,data,streamingError:true});
    let seed = 0x6a09e667;
    const random = n => { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return (seed >>> 0) % n; };
    const atoms = ['a','a.b','a.b[true]','$','missing','null','0','1','true','"text"'];
    function expression(depth) {
        if (!depth || random(3) === 0) return atoms[random(atoms.length)];
        const left = expression(depth-1), right = expression(depth-1);
        return [() => `[${left},${right}]`, () => `{"x":${left},"y":${right}}`,
            () => `(${left})[true]`, () => `(${left})[0]`, () => `(${left})[-1]`,
            () => `$count(${left})`, () => `$sum(${left})`, () => `(${left}) = (${right})`,
            () => `{"x":${left}}.x`, () => `(${left}) or (${right})`][random(10)]();
    }
    // The reference mutates an empty root array during object construction; jx's
    // immutable-input boundary deliberately excludes that host-side effect.
    const immutableShapes = shapes.filter(data => !Array.isArray(data) || data.length !== 0);
    for (let i=0; i<2000; i++) await check({expr:expression(3),data:immutableShapes[random(immutableShapes.length)],streamingError:true});
    console.log(`Checked ${checked()} constructor evaluations against upstream`);
}
main().catch(error => {console.error(error);process.exitCode=1;});
