"use strict";
const now=()=>process.hrtime.bigint().toString();
function deferred(){let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b});promise.catch(()=>{});return {promise,resolve,reject};}
async function submit(api,build,phase,head){
 const row={phase,c0:now(),c1:null,c_ack:null,c_in:null,c_fin:null,c_done:null,statuses:[],head_at_start:head?{...head}:null};
 row.encode_start=now();const {tx,signer}=await build();row.encode_end=now();
 row.nonce_query_start=now();const nonce=await api.rpc.system.accountNextIndex(signer.address);row.nonce_query_end=now();
 row.sign_start=now();await tx.signAsync(signer,{nonce:nonce.toString()});row.sign_end=now();
 Object.assign(row,{account:signer.address,nonce:nonce.toString(),signed_bytes:tx.toHex(),tx_hash:tx.hash.toHex()});
 const included=deferred(),finalized=deferred(),acked=deferred();let unsub,closed=false;
 const close=()=>{closed=true;clearTimeout(timer);if(unsub)unsub();};
 const fail=e=>{close();included.reject(e);finalized.reject(e);};
 const timer=setTimeout(()=>fail(new Error('finality timeout: '+phase)),180000);
 row.c1=now();
 tx.send(result=>{
  const time=now();row.statuses.push({status:result.status.type,at_ns:time});
  if(['Invalid','Dropped','Usurped','Retracted','FinalityTimeout'].includes(result.status.type))return fail(new Error('unexpected transaction status '+result.status.type));
  if(result.status.isInBlock && !row.c_in){
   row.c_in=time;row.block_hash=result.status.asInBlock.toHex();
   if(result.dispatchError)return fail(new Error('target dispatch failed: '+result.dispatchError.toString()));
   included.resolve();
  }
  if(result.status.isFinalized){
   row.c_fin=time;row.finalized_block_hash=result.status.asFinalized.toHex();
   if(!row.c_in)return fail(new Error('finalized without first inclusion'));
   if(row.finalized_block_hash!==row.block_hash)return fail(new Error('finalized inclusion changed'));
   close();finalized.resolve();
  }
 }).then(u=>{unsub=u;row.c_ack=now();acked.resolve();if(closed)unsub();}).catch(e=>{acked.reject(e);fail(e);});
 return {row,included:included.promise,finalized:finalized.promise,acked:acked.promise,close};
}
async function evidence(api,handle){
 const row=handle.row;row.evidence_read_start=now();
 const [header,block,events]=await Promise.all([api.rpc.chain.getHeader(row.block_hash),api.rpc.chain.getBlock(row.block_hash),api.query.system.events.at(row.block_hash)]);
 const index=block.block.extrinsics.findIndex(x=>x.hash.toHex()===row.tx_hash);if(index<0)throw new Error('signed tx missing in included block');
 const all=events.map((x,i)=>({index:i,phase:x.phase.toJSON(),section:x.event.section,method:x.event.method,data:x.event.data.toJSON(),topics:x.topics.map(t=>t.toHex())}));
 const own=all.filter(x=>x.phase.applyExtrinsic===index),terminal=own.filter(x=>x.section==='system'&&['ExtrinsicSuccess','ExtrinsicFailed'].includes(x.method));
 if(terminal.length!==1||terminal[0].method!=='ExtrinsicSuccess')throw new Error('target not successful');
 const fee=own.find(x=>x.section==='transactionPayment'&&x.method==='TransactionFeePaid');if(!fee)throw new Error('missing fee');
 Object.assign(row,{block_number:header.number.toNumber(),parent_hash:header.parentHash.toHex(),extrinsic_index:index,events:own,fee:fee.data[1],dispatch_error:null,c_done:now()});
 return row;
}
module.exports={submit,evidence,now};
