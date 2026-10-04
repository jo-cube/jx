// Matcher protocol, ECMAScript code units, cursor state and mixed expressions.
const fs=require('node:fs');
const path=require('node:path');
const {check,root,checked}=require('./differential.cjs');
async function main(){
    for(const c of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/matchers.json'))))await check(c);
    for(const pattern of ['a*','.*?','^','$','a?','(?=b)','(?<=a)','[a-z]*','(?:)','(?!)'])
        for(const text of ['', 'a', 'ab', 'bbb'])for(const expr of [
            `$contains(text,/${pattern}/)`, `$match(text,/${pattern}/)`, `$match(text,/${pattern}/,1)`,
            `$split(text,/${pattern}/)`, `$replace(text,/${pattern}/,"X")`
        ])await check({expr,data:{text}});
    const patterns=['[\\uD83D\\uDE00]', '\\uD83D\\uDE00+', '\\uDE00', '😀+', 'a','a+','a+?','(a)(b)?','(a)|(b)','[ab]+','\\w+','\\s+','\\d+','\\b[a-z]+\\b','a(?=b)','a(?!b)','(?<=a)b','(?<!a)b','(a)\\1','[é😀]','\\uD83D'];
    const strings=['','aba aa ab','ababbcc','12 a34 5','a\nb','a\tb','é😀a😀','漢字 abc','a\u2028b'];
    for(const pattern of patterns)for(const text of strings)for(const expr of [
        `$contains(text,/${pattern}/)`, `$string($match(text,/${pattern}/))`,
        `$string($split(text,/${pattern}/))`, `$string($replace(text,/${pattern}/,"<$0,$1,$2,$$>"))`,
        `$string($replace(text,/${pattern}/,function($m){$m.start & ":" & $m.match}))`
    ])await check({expr,data:{text}});
    for(const text of ['a\"b\na','a\\a','\ud800a\udfff','😀a😀'])for(const expr of [
        '$string($match(text,/./))', '$string($split(text,/a/))', '$string($replace(text,/./,"x"))',
        '($r:=/a/;$m:=$r(text);[$m.start,$m.end,$m.next().start])'
    ])await check({expr,data:{text}});
    // Legacy escapes, word/line boundaries and character classes beyond the
    // main workload matrix. Only valid reference literals are included.
    for(const pattern of ['\\W+','\\D+','\\S+','\\B','a$','[^a]+','[a-z]+','\\p{L}','\\cA','\\a','\\j','\\1','\\8','\\123','(a)?b\\1']) {
        for(const text of ['a','abc','a\n','p{L}','j','8','S','\u0001','é😀','漢字'])
            await check({expr:`$string($match(text,/${pattern}/))`,data:{text}});
    }
    const custom=fs.readFileSync(path.join(root,'tests/conformance/groups/matchers/case000.jsonata'),'utf8');
    await check({expr:custom,data:null});
    for(const count of [0,1,2,8,40]) {
        const data={rows:Array.from({length:count},(_,i)=>({n:i,s:i%2?'red Hat':'coat'}))};
        for(const expr of ['$sum(rows[s~>/hat/i].n)','rows{$replace(s,/hat/i,"cap"):$sum(n)}',
            '$map(rows,function($r){$match($r.s,/(h)(at)/i).groups})',
            '($r:=/hat/i;$map(rows,function($v){$contains($v.s,$r)}))',
            '$map(rows,function($v){$replace($v.s,/hat/i,function($m){$m.start & ":" & $m.match})})'
        ])await check({expr,data});
    }
    console.log(`Checked ${checked()} regex/matcher evaluations against upstream`);
}
main().catch(e=>{console.error(e);process.exitCode=1;});
