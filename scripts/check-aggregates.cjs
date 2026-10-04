// Optional deterministic checks against the pinned JSONata implementation.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root, 'tests/semantics/aggregates.json'))))
        await check(test);
    const names = ['count', 'sum', 'min', 'max'];
    const shapes = [null, false, 3, '3', {}, [], [1], [1,2], [[1,2]], [[1],[2]],
        [null,1], [{b:1},{}], [{b:[1,2]},{b:[]}], [[{b:1}],[{b:2}]],
        [{b:[[1]]},{b:[2]}]];
    for (const a of shapes) for (const argument of [
        'a', 'a[true]', 'a.b', 'a.b[true]', '(a.b)[0]', 'a.b[0]',
        'a[0+0]', 'a[true][0+0]', 'a[-1]', 'a.(b+1)',
    ]) for (const name of names) for (const data of [{a}, [{a}], [[{a}], {a}]])
        await check({expr:`$${name}(${argument})`, data});
    let seed = 0x25a3168e;
    function random(n) { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return (seed >>> 0) % n; }
    function value(depth) {
        const choice = random(depth ? 8 : 5);
        if (choice === 0) return null;
        if (choice === 1) return !!random(2);
        if (choice === 2) return random(9)-4;
        if (choice === 3) return (random(9)-4)/3;
        if (choice === 4) return ['', 'a', 'é'][random(3)];
        if (choice === 5) return Array.from({length:random(5)}, () => value(depth-1));
        const object = {};
        for (const key of ['a','b','x']) if (random(2)) object[key] = value(depth-1);
        return object;
    }
    const args = ['a', 'a.b', 'a[true]', 'a[x].b', 'a[$ > 0]', 'a.(b*2)',
        'a[$count(b)>1].b', 'a[$sum(b)>0]', 'a.b[true]', '(a.b)[-1]',
        '$sum(a)', '$count(a)', 'a[$min(b) >= 0].b', 'a[$max(b) < 2].b'];
    for (let i=0; i<3000; i++) {
        let expr = `$${names[random(4)]}(${args[random(args.length)]})`;
        if (random(4) === 0) expr = `a.(${expr})`;
        if (random(4) === 0) expr += ' + 1';
        await check({expr, data:i%3 ? {a:value(3), b:value(3)} : value(4), streamingError:true});
    }
    for (const name of names) {
        for (const input of ['{"a":1e999}', '{"a":[1e999,-1e999]}', '{"a":[0,-0]}', '{"a":[-0,0]}',
            '{"a":[9007199254740993,1,-9007199254740992]}'])
            await check({expr:`$${name}(a)`, input});
        for (const arg of ['1/0','0/0','-0','1e308+1e308','()', 'a.(1/$)'])
            await check({expr:`$${name}(${arg})`,data:{a:[0,1,2]}});
        for (const expr of [`$${name}(a[x].v)`, `$${name}(a[x].v, null)`, `$${name}(null,a[x])`])
            await check({expr,input:'{"a":[{"x":true,"v":null},{"x":1e999}]}'});
    }
    console.log(`${checked()} aggregate evaluations match JSONata 2.2.0, including errors and sequence boundaries`);
}
main().catch(error => { console.error(error); process.exitCode = 1; });
