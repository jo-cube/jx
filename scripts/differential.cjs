// Shared optional checks against the pinned upstream source.
const assert = require('node:assert/strict');
const path = require('node:path');
const {spawnSync} = require('node:child_process');
const root = path.resolve(__dirname, '..');
const upstream = path.resolve(process.argv[2]);
const cli = path.resolve(process.argv[3]);
assert.equal(spawnSync('git', ['-C', upstream, 'rev-parse', 'HEAD'], {encoding:'utf8'}).stdout.trim(),
    '8ee4476f8a228bfc7a62979ae0a9c13a4043cd03');
const jsonata = require(path.join(upstream, 'src/jsonata'));
const kinds = {T2003:"TypeError", T2004:"TypeError", T2007:"TypeError", T2008:"TypeError", D2014:"NumericRange", T1005:'TypeError', T1006:'TypeError', T1007:'TypeError', T1008:'TypeError', T2001:'TypeError', T2002:'TypeError', T2009:'TypeError', T2010:'TypeError',
    D1002:'TypeError', D1001:'NumericRange', T0410:'TypeError', T0412:'TypeError', T1003:'TypeError', D1009:'DuplicateKey'};
function items(value) {
    const result = value === undefined ? [] : Array.isArray(value) && value.sequence && !value.keepSingleton ? Array.from(value) : [value];
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
        if (!test.streamingError) assert.equal(child.stdout, '', context);
    } else {
        assert.equal(child.status, 0, context + child.stderr);
        const actual = child.stdout.trim() ? child.stdout.trim().split('\n').map(line => JSON.parse(line)) : [];
        assert.deepEqual(actual, expected.items, context);
    }
    checked++;
}
module.exports = {check, root, checked: () => checked};
