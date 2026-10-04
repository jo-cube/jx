// Static and dynamic pictures share semantics, including invalid-picture timing.
const fs=require('node:fs');
const path=require('node:path');
const {check,root,checked}=require('./differential.cjs');
async function main() {
    for(const test of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/formatting.json')))) if(!test.localOnly) await check(test);
    const pictures=['0','0000','0.00','#.##','#,##0.00','0.0e0','00.00e00','#.e9','0%','0.00‰','0.00;(0.00)','9,9,99.99','0.000,000'];
    for(const n of [-123456.789,-12.5,-0.005,-0,0,0.005,0.125,4.525,999.999,1e20,1e-200]) {
        for(const p of pictures) {
            if(Math.abs(n)>=1e19&&(p.includes("%")||p.includes("‰"))) continue; // Explicit fixed-output boundary.
            await check({expr:'$formatNumber(n,p)',data:{n,p}});
            await check({expr:`$formatNumber(n,${JSON.stringify(p)})`,data:{n}});
        }
    }
    for(const n of ['0/0','1/0','-1/0']) for(const p of ['0','0.00','0%','0000','#,##0.0']) await check({expr:`$formatNumber(${n},${JSON.stringify(p)})`,data:null});
    for(const p of ['£0.0e0','£0.0e00','😀0.0e00','£0.0e000']) await check({expr:'$formatNumber(n,p)',data:{n:1234,p}});
    await check({expr:"$formatNumber(1234,'£٠.٠e٠٠',{'zero-digit':'٠'})",data:null});
    let seed=0x20;
    function random(){seed=(Math.imul(seed,1664525)+1013904223)>>>0;return seed/2**32;}
    for(let i=0;i<250;i++) {
        const n=(random()-.5)*10**Math.floor(random()*16-8),p=pictures[Math.floor(random()*pictures.length)];
        await check({expr:'$formatNumber(n,p)',data:{n,p}});
        await check({expr:`$formatNumber(${n},${JSON.stringify(p)})`,data:null});
    }
    for(const n of [0,1,11,21,99,555,1234,123456,-123.7,9007199254740992,1000000000000000100,100000000000000020000]) for(const p of ['1','0000','#,##0','1,00,000','A','a','I','i','Ww','w','W;o','1;o','٠٠٠٠']) {
        if(n>1000000&&['I','i'].includes(p)) continue; // Explicit Roman output guard.
        await check({expr:'$formatInteger(n,p)',data:{n,p}});
        await check({expr:`$formatInteger(n,${JSON.stringify(p)})`,data:{n}});
        if(n>=0) await check({expr:'$parseInteger($formatInteger(n,p),p)',data:{n,p}});
    }
    for(const n of [-123.7,-.5,0,1,2.5,3.5,12345678]) for(const radix of [2,8,10,16,36,1,37,2.5,35.5]) await check({expr:'$formatBase(n,radix)',data:{n,radix}});
    const dates=[0,-1,951782400123,1526947200000,1419940800000,1104537600000,1451606400000,-12219292800000,253402300799999];
    const datePictures=['[Y0001]-[M01]-[D01]','[Y0001]-[M01]-[D01]T[H01]:[m01]:[s01].[f001][Z01:01t]','[D1o] [MNn] [Y] [FNn]','[Y] [d] [X] [W] [x] [w] [F1]','[Y0001][M01][D01][H01][m01][s01][f001]','[Y0001,2-2] [MN,3-3] [DN]','[YI]-[MI]-[DI]','[[[Y]]] [h01]:[m] [PN]'];
    for(const t of dates) for(const p of datePictures) for(const tz of ['+0000','+0530','-0500','-0530']) {
        await check({expr:'$fromMillis(t,p,tz)',data:{t,p,tz}});
        await check({expr:`$fromMillis(t,${JSON.stringify(p)},tz)`,data:{t,tz}});
    }
    for(const t of dates) for(const p of [datePictures[0],datePictures[2],datePictures[4],datePictures[6],'[Y]-[d]','[Y]-[M]-[D] [H]:[m]:[s].[f]']) await check({expr:'$toMillis($fromMillis(t,p),p)',data:{t,p}});
    for(const text of ['2018','2018-05','2018-05-22','2018-02-31','2018-00-00','2018-05-22T12:00:00.123456Z','2018-05-22T24:00:00Z','2018-05-22T24:00:01Z','2018-05-22T29:00:00Z','2018-05-22T12:00:00-05:30','2018+0100','2018Z','2018-05-22+0100','2018-05-22T12:00:00+2900junk','foo']) await check({expr:'$toMillis(text)',data:{text}});
    for(const n of [undefined,null,'bad',0,42]) for(const p of [undefined,null,'000','invalid','[Y','[q]','[YN]']) for(const name of ['formatInteger','parseInteger','formatNumber','fromMillis','toMillis']) await check({expr:`$${name}(n,p)`,data:{n,p}});
    for(const opts of [{'zero-digit':'A'},{'decimal-separator':',','grouping-separator':'.'},{'percent':'pc'},{'minus-sign':'NEG'}]) for(const n of [-1234.5,0,.14]) for(const p of ['AAA.AAA','#.##0,00','0pc','0.00']) await check({expr:'$formatNumber(n,p,opts)',data:{n,p,opts}});
    for(const expr of ["$formatNumber(n,'0.0') ?? 'none'","$fromMillis(t)[]","($f:=$fromMillis(?,'[Y]');$map([0,t],$f))","a[n>0]{kind:$formatNumber($sum(n),'0.00')}","($f:=function($v){$fromMillis($v.t,'[Y]-[M]-[D]')};$map(a,$f))"]) await check({expr,data:{n:1.25,t:1526947200000,a:[{n:1.25,t:1526947200000,kind:'a'},{n:4.525,t:0,kind:'a'}]}});
    console.log(`${checked()} formatting differential comparisons passed`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
