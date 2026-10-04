const fs=require('node:fs'),path=require('node:path');
const {check,root,checked}=require('./differential.cjs');
async function main(){
    for(const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/compatibility.json'))))await check(test);
    for(const pattern of ['é','[éè]','å','[α-ω]','(σ)\\1','\\u00e9','ß','K','İ'])
        for(const text of ['', 'ÉéÈèÅå','Σσς','Iİi','KKk','SSß','é\ud800'])
            for(const expr of [`$contains(text,/${pattern}/i)`,`$match(text,/${pattern}/i)`,`$replace(text,/${pattern}/i,'<$0>')`])await check({expr,data:{text}});
    for(const f of ['abs','floor','ceil','sqrt','power'])for(const value of [undefined,null,false,true,0,-2,'',' 2.5 ','+3','.5','0x10','0o10','0b10','Infinity','1e3','1x','\u00852\u0085','\uFEFF2\u00A0',[],[2],[true],[null],['3'],[[2]],[1,2]])await check({expr:`$${f}(?${f==='power'?',2':''})(value)`,data:{value}});
    for(const count of [1,2,8,32]) {
        const data={a:Array.from({length:count},(_,i)=>({v:count-i}))};
        for(const expr of ['a#$i^(v)[v>1].$i','a#$i^(v)[`@`.v>1].$i','a#$i^(v)[i>=1].$i','a#$i^(v)^(>`@`.v).$i','a#$i^(v){"p":i}','a#$i^(v).$[v>1].$i'])await check({expr,data});
    }
    // The pinned reference throws on empty tuple grouping; local {} policy is
    // covered separately, so keep this differential group nonempty.
    for(const count of [2,8,32])await check({expr:'a#$i^(v)[i>=1]{"total":$sum(`@`.v)}',data:{a:Array.from({length:count},(_,i)=>({v:count-i}))}});
    for(const picture of ['[Y]年[M]月[D]日','é [Y]-[M]-[D]','[Y]-[M]-[D] 😀','[Y]中[M]é[D]','[Y]é[M]É[D]'])for(const t of [0,-1,1526947200000])await check({expr:'$toMillis($fromMillis(t,picture),picture)',data:{t,picture}});
    console.log(`${checked()} residual compatibility comparisons passed`);
}
main().catch(e=>{console.error(e);process.exitCode=1;});
