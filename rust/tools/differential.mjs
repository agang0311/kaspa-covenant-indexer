// Offline JS/Rust rule differential on representative historical transitions plus single-field mutations.
// Usage: node rust/tools/differential.mjs /path/to/snapshot.sqlite /path/to/evidence-directory
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {DatabaseSync} from 'node:sqlite';
import {execFileSync} from 'node:child_process';
import {isDeepStrictEqual} from 'node:util';
import {loadContracts,checkRound} from '../../src/contracts.mjs';
const root=fileURLToPath(new URL('../..',import.meta.url));
const [dbPath,outDir]=process.argv.slice(2);
if(!dbPath||!outDir)throw Error('snapshot and evidence directory required');
fs.mkdirSync(outDir,{recursive:true});
const db=new DatabaseSync(dbPath,{readOnly:true});
const network=JSON.parse(db.prepare("SELECT value FROM meta WHERE key='network'").get().value);
const registry=await loadContracts(path.join(root,'contracts/v202.json'),{network});
const p=registry.forGenesis()[0];
const rows=db.prepare("SELECT * FROM transitions WHERE contract=? AND status!='ROLLED_BACK' ORDER BY observed_at DESC").all(p.key);
const seen=new Set(),cases=[];
const mutations=[
 ['baseline',()=>{}],
 ['bad-witness',tx=>{if(tx.inputs?.length)tx.inputs[0].signatureScript='00';}],
 ['wrong-input',tx=>{if(tx.inputs?.length)tx.inputs[0].previousOutpoint.transactionId='00'.repeat(32);}],
 ['wrong-output-value',tx=>{if(tx.outputs?.length)tx.outputs[0].value=(BigInt(tx.outputs[0].value)+1n).toString();}],
 ['wrong-output-covenant',tx=>{if(tx.outputs?.length)tx.outputs[0].covenant={authorizingInput:0,covenantId:'00'.repeat(32)};}],
 ['bad-payload',tx=>{tx.payload='ff';}],
 ['zero-locktime',tx=>{tx.lockTime='0';}],
 ['missing-outputs',tx=>{tx.outputs=[];}],
];
for(const row of rows){
 if(seen.has(row.kind))continue;seen.add(row.kind);
 const next=JSON.parse(row.next),prev=row.previous?JSON.parse(row.previous):null;
 for(const [label,mutate] of mutations){
  const tx=structuredClone(row.kind==='GENESIS'?next.genesisTransaction:next.latestTransaction);
  mutate(tx);
  cases.push({name:row.kind+':'+label,network,kind:row.kind,previous:prev,tx,block:{hash:row.accepting,daa:row.accepting_daa}});
 }
}
const expected=cases.map(c=>{
 try{const r=c.kind==='GENESIS'?p.genesis(structuredClone(c.tx),c.block):p.advance(structuredClone(c.previous),structuredClone(c.tx),c.block);if(!r)throw Error('GENESIS_NOT_CLAIMED');return {name:c.name,accepted:true,result:checkRound(r,p.key)};}
 catch(e){return {name:c.name,accepted:false,error:e.message};}
});
const safe=x=>JSON.parse(JSON.stringify(x,(_,v)=>typeof v==='bigint'?v.toString():v));
const input=path.join(outDir,'cases.json');fs.writeFileSync(input,JSON.stringify(cases));
const actual=JSON.parse(execFileSync(path.join(root,'rust/target/release/kci'),['check-cases',input],{maxBuffer:32*1024*1024,timeout:30000}));
const failures=[],errorTextDifferences=[];
for(let i=0;i<cases.length;i++){
 const a=actual[i],b=safe(expected[i]);
 if(a.accepted!==b.accepted || (a.accepted&&!isDeepStrictEqual(a.result,b.result)))failures.push({name:a.name,rust:a,js:b});
 if(!a.accepted&&!b.accepted&&a.error!==b.error)errorTextDifferences.push({name:a.name,rust:a.error,js:b.error});
}
const report={network,cases:cases.length,kinds:[...seen],rejected:actual.filter(x=>!x.accepted).length,matched:cases.length-failures.length,errorTextDifferences,failures};
fs.writeFileSync(path.join(outDir,'differential.json'),JSON.stringify(report,null,2));
console.log(JSON.stringify({...report,failures:failures.map(f=>f.name)},null,2));
if(failures.length)process.exitCode=1;
