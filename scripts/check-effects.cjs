// Dynamic compilation, lexical inheritance and effectful standard functions.
const fs=require('node:fs'),path=require('node:path');
const {check,root,checked}=require('./differential.cjs');
async function main(){
 for(const t of JSON.parse(fs.readFileSync(path.join(root,'tests/semantics/effects.json')))) await check(t);
 const programs=['missing','null','true','[1]','[1,2].$','{"s":"a\\nb"}','x','$.x','$$.x','$sum(a)','$x+1','function($v){$v+$x}','($y:=3;function($v){$v+$y})'];
 for(const data of [null,[],[1,2],{x:7,a:[1,2,3]}]) for(const program of programs){
  const p=JSON.stringify(program);
  for(const expr of [`($x:=2;$eval(${p}))`,`($x:=2;$eval(code))`,`($x:=2;$eval(${p},a))`]) {
   const d=expr.includes('code')?{code:program,x:7,a:[1,2,3]}:data;
   await check({expr:program.includes('function')?`$type(${expr})`:expr,data:d});
  }
 }
 for(const n of [0,1,2,8,32]) {
  await check({expr:`$sort($shuffle([1..${n}]))`,data:null});
  await check({expr:`($f:=$eval(code);$map([1..${n}],$f))`,data:{code:'function($x){$x*2}'}});
 }
 console.log(`Checked ${checked()} dynamic/effect evaluations against upstream`);
}
main().catch(e=>{console.error(e);process.exitCode=1});
