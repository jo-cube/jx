// Branches, fused folds and fixed outputs against the pinned reference.
const {check, checked} = require('./differential.cjs');
async function main() {
    const atoms = [undefined,null,false,true,0,-0,2,-3.5,1e308,'x',[],[1],[1,2],{}];
    const scalar = ['x>y ? x*x+y*y : x-y', 'x and y>0 and (x+y<20 or y=0)',
        '(x>0 ? y+y : y*y)+y+y', '(x>0 ? (y>0 ? x+y : x-y) : x*y)+y',
        '{"sum":x+y,"product":x*y,"schema":{"v":1}}',
        '{"2":x+x,"1":y+y,"other":x>y ? x+y : x-y}',
        'x>0 ? $lookup({"x":3,"y":7,"undefined":9},key)*y+y*y+y : 0'];
    for (const x of atoms) for (const y of atoms) {
        for (const expr of scalar) await check({expr,data:{x,y,key:'x'}});
        for (const agg of ['sum','count','min','max']) {
            for (const body of ['rows[x>0].(x*y+1)', 'rows[x>0 and y>0].(x*y+1)', 'rows[x>0][y>0].(x*y+1)', 'rows.(x*x+x+1)']) {
                await check({expr:`$${agg}(${body})`,data:{rows:[{x,y},{x:2,y:3}]}});
            }
        }
    }
    for (const rows of [undefined,null,[],{},[[]],[[{x:2,y:3}]], [{x:2,y:3},[{x:4,y:5}]], [{x:2,y:3},{x:4,y:null}]]) {
        for (const expr of ['$sum(payload.rows[x>0].(x*y+1))', '$sum(payload.rows[x>0].x)',
            'payload.rows.{"sum":x+y,"value":x*x+x}',
            '($f:=function(){$sum(payload.rows[x>0].(x*y+1))};$f())']) {
            await check({expr,data:{payload:{rows}},streamingError:true});
        }
    }
    for (const input of ['{"x":1,"\\u0078":7,"y":3}', '{"x":1e999,"y":3}', '{"x":-0,"y":0}']) {
        for (const expr of scalar) await check({expr,input});
    }
    console.log(`Checked ${checked()} expanded-plan evaluations against upstream`);
}
main().catch(error=>{console.error(error);process.exitCode=1;});
