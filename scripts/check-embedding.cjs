const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const {check, root, checked} = require('./differential.cjs');
const jsonata = require(path.join(path.resolve(process.argv[2]), 'src/jsonata'));
(async () => {
    const cases = JSON.parse(fs.readFileSync(path.join(root, 'tests/semantics/embedding.json')));
    for (const test of cases) {
        try {
            const value = await jsonata(test.expr).evaluate(test.data, test.bindings);
            const items = value === undefined ? [] : Array.isArray(value) && value.sequence && !value.keepSingleton ? Array.from(value) : [value];
            assert.deepEqual(JSON.parse(JSON.stringify(items)), test.items, JSON.stringify(test));
        } catch (error) {
            if (!test.error) throw error;
            assert.ok(['T1006','T2001','T0410','T0412'].includes(error.code) || (!error.code && error instanceof TypeError), JSON.stringify(test));
        }
        const declarations = Object.entries(test.bindings).map(([name,value])=>`$${name}:=${JSON.stringify(value)}`).join(';');
        await check({...test, expr:`(${declarations};${test.expr})`});
    }
    console.log(`Checked ${checked()} external-binding/lexical equivalences against upstream; Rust embedding tests assert the public API`);
})().catch(e=>{console.error(e);process.exit(1);});
