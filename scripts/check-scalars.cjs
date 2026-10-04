// Optional deterministic scalar differential check.
const fs = require("node:fs");
const path = require("node:path");
const {check, root, checked} = require("./differential.cjs");
async function main() {
    const cases = JSON.parse(fs.readFileSync(path.join(root, 'tests/semantics/scalars.json')));
    for (const test of cases) await check(test);
    let seed = 0x76a314f2;
    function random(n) { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return (seed >>> 0) % n; }
    function value(depth) {
        const choice = random(depth ? 8 : 5);
        if (choice === 0) return null;
        if (choice === 1) return !!random(2);
        if (choice === 2) return random(100) - 50;
        if (choice === 3) return (random(100) - 50) / 7;
        if (choice === 4) return ['', 'a', 'é', '😀', '\ud800', '\ue000'][random(6)];
        if (choice === 5) return Array.from({length:random(4)}, () => value(depth-1));
        const object = {};
        for (const key of ['a','b','x','y']) if (random(2)) object[key] = value(depth-1);
        return object;
    }
    const leaves = ['a', 'b', 'a.x', '$.a.x', 'b.y', 'missing', '()', '$', '0', '2.5', '-3', 'true', 'false', 'null', "'a'", '"\\ud800"', '(0/0)', '(1/0)'];
    const ops = ['+', '-', '*', '/', '%', '=', '!=', '<', '<=', '>', '>=', 'and', 'or'];
    function expression(depth) {
        if (!depth || random(3) === 0) return leaves[random(leaves.length)];
        if (random(5) === 0) return `-(${expression(depth-1)})`;
        return `(${expression(depth-1)} ${ops[random(ops.length)]} ${expression(depth-1)})`;
    }
    for (let i=0; i<1800; i++) {
        const data = i % 3 ? {a:value(3), b:value(3)} : value(4);
        await check({expr:expression(3), data});
    }
    for (const input of [
        '{"a":{"x":0,"\\u0078":1},"b":{"x":1}}',
        '{"a":1e999,"b":1e999}', '{"a":9007199254740993,"b":9007199254740992}',
        '{"a":1e-320,"b":1e-320}', '{"a":[true,1e999],"b":false}',
        '{"a":{"\\ud800":1},"b":{"\\ud800":1}}',
    ]) for (const expr of ['a = b', 'a != b', 'a < b', 'a + b', 'a or b', '-a']) await check({expr,input});
    console.log(`${checked()} scalar evaluations match JSONata 2.2.0, including error kinds and per-record output`);
}
main().catch(error => { console.error(error); process.exitCode = 1; });
