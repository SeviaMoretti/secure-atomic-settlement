const assert=require('assert/strict');const {submit}=require('./timed_transport');
async function main(){
 const api={rpc:{system:{accountNextIndex:async()=>({toString:()=> '0'})}}};let callback,stopped=0;
 const tx={signAsync:async()=>{},toHex:()=> '0x00',hash:{toHex:()=> '0x01'},send:fn=>{callback=fn;return Promise.resolve(()=>stopped++)}};
 const h=await submit(api,()=>({tx,signer:{address:'test'}}),'create',null);
 callback({status:{type:'InBlock',isInBlock:true,asInBlock:{toHex:()=> '0x02'}}});
 await h.included;assert.equal(h.row.c_fin,null);assert.equal(stopped,0);
 callback({status:{type:'Finalized',isFinalized:true,asFinalized:{toHex:()=> '0x02'}}});await h.finalized;await h.acked;assert(h.row.c_fin);assert.equal(stopped,1);
 const bad=await submit(api,()=>({tx,signer:{address:'test'}}),'fund',null);
 callback({status:{type:'Finalized',isFinalized:true,asFinalized:{toHex:()=> '0x02'}}});await assert.rejects(bad.included,/without first inclusion/);await assert.rejects(bad.finalized,/without first inclusion/);
 const reorg=await submit(api,()=>({tx,signer:{address:'test'}}),'settle',null);
 callback({status:{type:'InBlock',isInBlock:true,asInBlock:{toHex:()=> '0x02'}}});await reorg.included;
 callback({status:{type:'Retracted'}});await assert.rejects(reorg.finalized,/unexpected transaction status/);
 console.log('PASS: inclusion resolves before finality; missing inclusion and retraction fail closed');
}
main().catch(e=>{console.error(e);process.exit(1)});
