// Complete vendored upstream inventory, with explicit new-engine coverage.
// No downloads, skipped cases or infinite reference execution for rejected IR.
const assert=require('node:assert/strict');
const fs=require('node:fs');const path=require('node:path');const {spawnSync}=require('node:child_process');
const root=path.resolve('tests/conformance');const manifest=JSON.parse(fs.readFileSync(path.join(root,'manifest.json')));
assert.equal(manifest.revision,'8ee4476f8a228bfc7a62979ae0a9c13a4043cd03');
const tests=manifest.cases.map(row=>{
 const filename=path.join(root,row.file);const spec=JSON.parse(fs.readFileSync(filename));const c=Array.isArray(spec)?spec[row.index]:spec;
 let expr=c.expr===undefined?fs.readFileSync(path.join(path.dirname(filename),c['expr-file']),'utf8'):c.expr;
 if(c.bindings&&Object.keys(c.bindings).length)expr='('+Object.entries(c.bindings).map(([k,v])=>'$'+k+':='+JSON.stringify(v)+';').join('')+expr+' )';
 const absent_input=c.data===undefined&&c.dataset==null;
 if(row.phase==='compile')expr=c.expr===undefined?fs.readFileSync(path.join(path.dirname(filename),c['expr-file']),'utf8'):c.expr;
 const input=c.dataset?fs.readFileSync(path.join(root,'datasets',c.dataset+'.json'),'utf8'):JSON.stringify(c.data===undefined?null:c.data);
 return {id:row.file+'#'+row.index,row,spec:c,request:{expr,input,absent_input,coverage_only:true}};
});
const child=spawnSync(path.resolve(process.argv[2]),[],{input:tests.map(t=>JSON.stringify(t.request)).join('\n')+'\n',encoding:'utf8',maxBuffer:128*1024*1024});assert.equal(child.status,0,child.stderr);
const answers=child.stdout.trim().split('\n').map(s=>JSON.parse(s));assert.equal(answers.length,tests.length);
const counts={supported:0,shape_rejected:0,compile_rejected:0,syntax_error:0};const inventory=[];
for(let i=0;i<tests.length;i++) {
 const {id,row,spec:c,request}=tests[i],a=answers[i];let status;
 if(a.unsupported){status=a.kind==='UnsupportedExpression'?'compile_rejected':'syntax_error';if(status==='syntax_error'){assert.equal(row.phase,'compile',id+' unexpected compilation failure '+JSON.stringify(a));assert.equal(a.kind,row.kind,id+' compilation kind');}}
 else if(a.compile_error){status='syntax_error';assert.equal(row.phase,'compile',id);}
 else if(a.prototype.some(p=>p.error==='UnsupportedExpression')) {status='shape_rejected';assert.ok(a.prototype.every(p=>p.error==='UnsupportedExpression'),id+' mode-dependent rejection');}
 else {
  status='supported';
  for(const result of a.prototype) {
   if(result.error){assert.deepEqual(result,a.engine,id+' exact engine error');assert.equal(result.error,row.kind,id+' upstream kind');}
   else {
    assert.equal(row.status,'supported',id+' unexpected success');
    let items=JSON.parse(result.items_json);
    if(c.undefinedResult)assert.deepEqual(items,[],id);
    else {
     assert.ok(items.length,id+' missing');let actual=items.length===1?items[0]:items;let expected=c.result;
     if(c.unordered){actual=[...actual].sort((a,b)=>JSON.stringify(a).localeCompare(JSON.stringify(b)));expected=[...expected].sort((a,b)=>JSON.stringify(a).localeCompare(JSON.stringify(b)));}
     assert.deepEqual(actual,expected,id+' '+request.expr);
    }
   }
  }
 }
 if(status==='supported') {
  if(a.engine_complete.error)assert.deepEqual(a.complete,a.engine_complete,id+' complete exact error');
  else {assert.equal(a.complete.present,a.engine_complete.present,id+' complete presence');if(a.complete.present)assert.deepEqual(JSON.parse(a.complete.json),JSON.parse(a.engine_complete.json),id+' complete JSON');}
 }
 counts[status]++;inventory.push({id,input_absent:request.absent_input,status,...(a.unsupported?{reason:a.unsupported}:{}),expected_status:row.status});
}
assert.equal(inventory.length,manifest.corpus_cases);
fs.writeFileSync(process.argv[3],JSON.stringify({revision:manifest.revision,counts,cases:inventory},null,2)+'\n');console.log(JSON.stringify({total:tests.length,...counts,absent_input:tests.filter(t=>t.request.absent_input).length,present_input_modes:2,absent_input_modes:1}));
