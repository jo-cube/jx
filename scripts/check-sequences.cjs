// Optional differential check against the pinned official implementation.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '..');
const upstream = path.resolve(process.argv[2]);
const revision = spawnSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], {encoding:'utf8'});
assert.equal(revision.stdout.trim(), '8ee4476f8a228bfc7a62979ae0a9c13a4043cd03');
const jsonata = require(path.join(upstream, 'src/jsonata'));
const cases = JSON.parse(fs.readFileSync(path.join(root, 'tests/semantics/sequences.json')));
function items(result) {
    return result === undefined ? [] : Array.isArray(result) && result.sequence ? Array.from(result) : [result];
}
async function main() {
    for (const test of cases) {
        const result = await jsonata(test.expr).evaluate(test.data);
        assert.deepEqual(JSON.parse(JSON.stringify(items(result))), test.items, JSON.stringify(test));
    }
    console.log(`${cases.length} readable sequence cases match JSONata 2.2.0`);
    if (!process.argv[3]) return;
    let seed = 0x12345678;
    function random(n) {
        seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
        return (seed >>> 0) % n;
    }
    function value(depth) {
        const kind = random(depth ? 6 : 3);
        if (kind === 0) return null;
        if (kind === 1) return random(10);
        if (kind === 2) return 'text';
        if (kind === 3) return Array.from({length:random(4)}, () => value(depth - 1));
        const object = {};
        for (const key of ['a','b','c']) if (random(2)) object[key] = value(depth - 1);
        return object;
    }
    const data = cases.map(test => test.data).concat(Array.from({length:800}, () => value(5)));
    for (const source of ['$', 'a', '$.a', 'a.b', '$.a.b', 'a.b.c', 'a.a.b.c']) {
        const expression = jsonata(source);
        const expected = [];
        const framed = [];
        for (const [index, input] of data.entries()) {
            const marker = {jx_record:index};
            const guard = source === '$' ? marker : source.replace(/^\$\./, '').split('.')
                .reduceRight((value, field) => ({[field]:value}), marker);
            // Markers preserve record boundaries even when a path emits nothing.
            framed.push(input, guard);
            expected.push(...items(await expression.evaluate(input)), marker);
        }
        const child = spawnSync(path.resolve(process.argv[3]), [source], {
            input:framed.map(input => JSON.stringify(input)).join('\n'), encoding:'utf8', maxBuffer:16*1024*1024
        });
        assert.equal(child.status, 0, child.stderr);
        const actual = child.stdout.trim() ? child.stdout.trim().split('\n').map(line => JSON.parse(line)) : [];
        assert.deepEqual(actual, JSON.parse(JSON.stringify(expected)), source);
    }
    console.log(`${data.length * 7} generated/curated evaluations match the reference CLI stream`);
}
main().catch(error => { console.error(error); process.exitCode = 1; });
