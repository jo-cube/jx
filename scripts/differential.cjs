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
const kinds = {D3120:"EvalSyntax",D3121:"EvalError",S0401:"SignatureError",S0402:"SignatureError",D3100:"NumericRange",D3110:"DateTimeError",D3080:"PictureError",D3081:"PictureError",D3082:"PictureError",D3083:"PictureError",D3084:"PictureError",D3085:"PictureError",D3086:"PictureError",D3087:"PictureError",D3088:"PictureError",D3089:"PictureError",D3090:"PictureError",D3091:"PictureError",D3092:"PictureError",D3093:"PictureError",D3130:"PictureError",D3131:"PictureError",D3132:"PictureError",D3133:"PictureError",D3134:"PictureError",D3135:"PictureError",D3136:"PictureError",D3137:"UserError",D3141:"AssertionFailed",D3138:"CardinalityError",D3139:"CardinalityError",D3140:"EncodingError",D3070:"TypeError",S0106:"UnsupportedExpression",S0209:"UnsupportedExpression",S0217:"UnsupportedExpression",T2011:"TypeError",T2012:"TypeError",T2013:"TypeError",D1004:"RegexError",D3010:"TypeError",D3011:"NumericRange",D3012:"TypeError",D3040:"NumericRange",T1010:"TypeError",D3001:"NumericRange", D3030:"TypeError", T2006:"TypeError", T2003:"TypeError", T2004:"TypeError", T2007:"TypeError", T2008:"TypeError", D2014:"NumericRange", T1005:'TypeError', T1006:'TypeError', T1007:'TypeError', T1008:'TypeError', T2001:'TypeError', T2002:'TypeError', T2009:'TypeError', T2010:'TypeError',
    T0411:'TypeError', D3020:'NumericRange', D3060:'NumericRange', D3061:'NumericRange', D3050:'TypeError', D1002:'TypeError', D1001:'NumericRange', T0410:'TypeError', T0412:'TypeError', T1003:'TypeError', D1009:'DuplicateKey'};
function items(value) {
    const result = value === undefined ? [] : Array.isArray(value) && value.sequence && !value.keepSingleton ? Array.from(value) : [value];
    return JSON.parse(JSON.stringify(result));
}
let checked = 0;
async function check(test) {
    const input = test.input ?? JSON.stringify(test.data);
    let expected;
    let compileError = false;
    try { expected = {items:items(await jsonata(test.expr).evaluate(JSON.parse(input)))}; }
    catch (error) {
        compileError = error.code?.startsWith("S") ?? false;
        const kind = kinds[error.code] ?? (!error.code && error instanceof TypeError ? 'TypeError' : undefined);
        assert.ok(kind, `unclassified upstream error ${error.code}: ${JSON.stringify(test)}`);
        expected = {error:kind};
    }
    if ('items' in test) assert.deepEqual(expected, {items:test.items}, JSON.stringify(test));
    if ('error' in test) assert.deepEqual(expected, {error:test.error}, JSON.stringify(test));
    const child = spawnSync(cli, [...(process.env.JX_DIFFERENTIAL_JIT ? ['--jit'] : []), '--', test.expr], {input, encoding:'utf8', maxBuffer:4*1024*1024});
    const context = JSON.stringify(test);
    if (expected.error) {
        assert.equal(child.status, compileError ? 2 : 1, context + child.stderr);
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
