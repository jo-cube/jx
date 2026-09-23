// Optional deterministic filter differential check; ordinary tests use the corpus.
const fs = require('node:fs');
const path = require('node:path');
const {check, root, checked} = require('./differential.cjs');
async function main() {
    for (const test of JSON.parse(fs.readFileSync(path.join(root, 'tests/semantics/filters.json'))))
        await check({...test, streamingError:true});
    let seed = 0x613abcf2;
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
    const predicates = ['0','1','-1','-2.5','(0)','0+0','true','false','null','a','b','x','()',
        '$','a = b','$ > 0','a or b','a + 1','a[0]','a[-1]','a.b[true]','1/0','0/0'];
    const heads = ['a','a.b','(a.b)','$','(a)','().a','$.a'];
    for (let i=0; i<3000; i++) {
        let expr = heads[random(heads.length)];
        const count = 1+random(3);
        for (let k=0;k<count;k++) expr += `[${predicates[random(predicates.length)]}]`;
        if (random(3)===0) expr += '.b';
        if (random(5)===0) expr = `(${expr}) = a`;
        const data = i%3 ? {a:value(3),b:value(3)} : value(4);
        await check({expr,data,streamingError:true});
    }
    const arrays = [[], [1], [null], [false,true], [[1,2],[3],[]], [{b:[1,2]},{b:[3]}],
        [{}, {b:[]}, {b:[null]}], [[{b:[[1],[2]]}],[{b:[3]}]],
        [{x:true,b:1},{x:false,b:2}], [{x:[0,0]},{x:[-1,-1]}]];
    for (const a of arrays) for (const expr of [
        'a[0][0]', 'a[0+0][0]', '(a[0+0])[0]', 'a[true].b', 'a.b[true]',
        'a[true].b[0]', 'a.b[-1]', '(a.b)[-1]', 'a[true][-1][0]',
        'a[true].(b+1)', 'a.(b)[0]', '$[a[0]]', 'a[x].b', 'a[x][-1]',
        '(a[true]) = a', 'a[b[true]]', 'a[(b[true]) = b]', 'a[true].$',
    ]) for (const data of [{a}, [{a}], [{a}, {a}], [[{a}], [{a}]]])
        await check({expr,data,streamingError:true});
    for (const input of [
        '{"a":[{"x":true,"y":null,"b":{"y":null}},{"x":1e999}]}',
        '{"a":[{"x":[false,1e999]},{"x":true}]}',
        '{"a":[{"x":0,"\\u0078":1},{"x":1}]}',
    ]) for (const expr of ['a[x]', 'a[x][y+1]', 'a[x].b[y+1]', 'a[x][-1]'])
        await check({expr,input,streamingError:true});
    console.log(`${checked()} filter evaluations match JSONata 2.2.0, including errors and sequence boundaries`);
}
main().catch(error => { console.error(error); process.exitCode = 1; });
