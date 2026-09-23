// Optional deterministic differential check; ordinary tests require no Node checkout.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '..');
const upstream = path.resolve(process.argv[2]);
const cli = path.resolve(process.argv[3]);
assert.equal(spawnSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], {encoding:'utf8'}).stdout.trim(),
    '8ee4476f8a228bfc7a62979ae0a9c13a4043cd03');
const jsonata = require(path.join(upstream, 'src/jsonata'));
const kinds = {T2001:'TypeError', T2002:'TypeError', T2009:'TypeError', T2010:'TypeError',
    D1002:'TypeError', D1001:'NumericRange'};
function items(value) {
    const result = value === undefined ? [] : Array.isArray(value) && value.sequence ? Array.from(value) : [value];
    return JSON.parse(JSON.stringify(result));
}
let checked = 0;
async function check(test) {
    const input = test.input ?? JSON.stringify(test.data);
    let expected;
    try { expected = {items:items(await jsonata(test.expr).evaluate(JSON.parse(input)))}; }
    catch (error) {
        assert.ok(kinds[error.code], `unclassified upstream error ${error.code}: ${JSON.stringify(test)}`);
        expected = {error:kinds[error.code]};
    }
    if ('items' in test) assert.deepEqual(expected, {items:test.items}, JSON.stringify(test));
    if ('error' in test) assert.deepEqual(expected, {error:test.error}, JSON.stringify(test));
    const child = spawnSync(cli, ['--', test.expr], {input, encoding:'utf8', maxBuffer:4*1024*1024});
    const context = JSON.stringify(test);
    if (expected.error) {
        assert.equal(child.status, 1, context + child.stderr);
        assert.ok(child.stderr.includes(`(${expected.error})`), context + child.stderr);
        assert.equal(child.stdout, '', context);
    } else {
        assert.equal(child.status, 0, context + child.stderr);
        const actual = child.stdout.trim() ? child.stdout.trim().split('\n').map(line => JSON.parse(line)) : [];
        assert.deepEqual(actual, expected.items, context);
    }
    checked++;
}
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
    console.log(`${checked} scalar evaluations match JSONata 2.2.0, including error kinds and per-record output`);
}
main().catch(error => { console.error(error); process.exitCode = 1; });
