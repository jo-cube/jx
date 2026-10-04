// Validation-time argument capture, canonical static keys and computed strings.
const fs=require('node:fs');
const path=require('node:path');
const {check,root,checked}=require('./differential.cjs');
async function main(){
    for(const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/acquisition.json')))) await check(test);
    const atoms=[undefined,null,false,true,0,2,-3.5,"x",[],[1],[1,2],{}, {x:2}];
    for(const a of atoms) for(const b of atoms) for(const expr of [
        "a&':'&$string(b)&':'&a",
        "{'name':a&':'&a,'n':$string(b+b),'raw':b}",
        "a?($string(a)&b&b):($string(b)&a&a)",
        '[a,b,a,[a,b]]',
        'function($a,$b){$a+$b}(a,b)',
        'function($a,$b)<nn:n>{$a+$b}(a,b)',
        'function($a,$b){[$a,$b]}(a,b)',
        'function($a,$b){$a?$a:$b}(nested.a,nested.b)',
        'function($a,$b){$a+$b}(a.x,b.x)',
        'function($x){$x}(a)',
        '$lookup({"x":1,"undefined":2,"é😀":3},a)',
    ]) await check({expr,data:{a,b,nested:{a,b}}});
    for(const width of [4,32,128,4096]) {
        const object=Object.fromEntries(Array.from({length:width},(_,i)=>[`é😀-${i}`,i]));
        for(const key of ['é😀-0',`é😀-${width-1}`,`é😀-${width}`,undefined]) {
            await check({expr:`$lookup(${JSON.stringify(object)},key)`,data:{key}});
            await check({expr:`($table:=${JSON.stringify(object)};$lookup($table,key))`,data:{key}});
        }
    }
    console.log(`Checked ${checked()} acquisition/lookup/string evaluations against upstream`);
}
main().catch(e=>{console.error(e);process.exitCode=1});
