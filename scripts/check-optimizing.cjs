// Optional differential checks for capture, UTF-8 operations and canonical plans.
const {check, checked} = require('./differential.cjs');
async function main() {
    const rows = [undefined,null,false,0,'x',{},[],[{x:2,y:3}],
        [{x:2,y:3},{x:2,y:null}], [{x:2,y:3},[{x:4,y:5}]],
        [{x:[1],y:2}], [{x:2,y:3},{x:-1,y:null}]];
    for (const value of rows) for (const agg of ['sum','count','min','max']) {
        for (const body of ['rows[x>0].(x*y+x)','(rows[x>0]).(x*y+x)',
            '((payload.rows)[x>0][y>0]).(x*y+x)','(rows[x>0].(x*y+x))']) {
            await check({expr:`$${agg}(${body})`,data:{rows:value,payload:{rows:value}},streamingError:true});
        }
    }
    const atoms=[undefined,null,false,0,-0,2,-3.5,1e308,[],[1],[1,2],{},'x'];
    for(const x of atoms) for(const y of atoms) for(const expr of [
        '($a:=x;$b:=y;$a*$b+$a*$a+$b*$b)',
        '($a:=x+x;$b:=$a+y;$a*$b+$a*$a)',
        '($a:=x;$a>0?$a*$a+$a+y:y*y+y+y)',
        '($a:=x;($a:=y;$a*$a+$a+$a)+$a*$a)',
    ]) await check({expr,data:{x,y}});
    const strings=['','Ada Lovelace','é中😀😀','ΟΣ ΟΣΑ','ßİı','a/b','quote"slash\\tab\t','a\ud800b','a😀b'];
    for(const n of strings) {
        for(const expr of ["{'x':$uppercase(n),'y':$length(n),'z':$contains(n,'a')}",
            "$contains(n,'a') and $length(n)>0 and $contains(n,'b')",'$lowercase(n)']) await check({expr,data:{n}});
        for(const pattern of ['', 'a','😀','é中',' ','missing','\ud83d'])
            for(const fn of ['contains','substringBefore','substringAfter'])
                await check({expr:`$${fn}(n,pattern)`,data:{n,pattern}});
        for(const start of [-99,-2.5,-1,-0,0,0.5,1,2.5,99]) for(const len of [undefined,-1,0,0.5,1,3.5,99])
            await check({expr:len===undefined?'$substring(n,start)':'$substring(n,start,len)',data:{n,start,len}});
    }
    for(const input of ['{"n":"a","\\u006e":"b"}', '{"n":null}', '{"n":["ab"]}', '{}'])
        await check({expr:"{'x':$uppercase(n),'y':$length(n),'z':$contains(n,'a')}",input});
    for(const expr of ["$config.nested in [$config.nested]", "$config.rows in [$config.rows]", "$config in [$config]",
        "$lookup($config,'nested') in [$lookup($config,'nested')]", "id in $config.allowed", "$config.label in values"])
        await check({expr:`($config:={"nested":{"n":4},"rows":[{"n":5},{"n":6}],"allowed":["a","b"],"label":"a"};${expr})`,data:{id:'a',values:['a','b']}});
    console.log(`Checked ${checked()} optimizing evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
