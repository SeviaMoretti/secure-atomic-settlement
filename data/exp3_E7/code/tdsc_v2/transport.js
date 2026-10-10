'use strict';
const common=require('../scenarios/s314_live_common');
const now=()=>process.hrtime.bigint().toString();
const json=x=>JSON.parse(JSON.stringify(x,(_k,v)=>typeof v==='bigint'?v.toString():v));
async function signAll(api, requests) {
  const next=new Map();
  for(const r of requests) {
    if(!next.has(r.signer.address)) next.set(r.signer.address,BigInt((await api.rpc.system.accountNextIndex(r.signer.address)).toString()));
    r.nonce=next.get(r.signer.address); next.set(r.signer.address,r.nonce+1n);
    await r.tx.signAsync(r.signer,{nonce:r.nonce.toString()});
    r.signed_at_ns=now();r.signed_bytes=r.tx.toHex();r.tx_hash=r.tx.hash.toHex();
  }
}
async function broadcast(api,r,timeoutMs=180000) {
  const log={label:r.label,actor:r.actor,account:r.signer.address,nonce:r.nonce.toString(),signed_at_ns:r.signed_at_ns,signed_bytes:r.signed_bytes,tx_hash:r.tx_hash,rpc_started_ns:now(),rpc_ack_ns:null,in_block_ns:null,statuses:[]};
  return new Promise((resolve,reject)=>{
    let unsub,ended=false;
    const close=()=>{clearTimeout(timer);if(unsub)unsub();};
    const fail=e=>{if(!ended){ended=true;close();e.submission=log;reject(e);}};
    const timer=setTimeout(()=>fail(new Error('inclusion timeout')),timeoutMs);
    r.tx.send(async result=>{
      const at=now();log.statuses.push({status:result.status.type,at_ns:at});
      if(result.status.isInvalid||result.status.isDropped||result.status.isUsurped) return fail(new Error('delivery failed: '+result.status.type));
      if(ended||!result.status.isInBlock)return;
      ended=true;log.in_block_ns=at;close();
      try {
        const blockHash=result.status.asInBlock.toHex();
        const header=await api.rpc.chain.getHeader(blockHash);
        const block=await api.rpc.chain.getBlock(blockHash);
        const index=block.block.extrinsics.findIndex(x=>x.hash.toHex()===r.tx_hash);
        if(index<0)throw new Error('signed transaction absent from included block');
        const events=await api.query.system.events.at(blockHash);
        const all=events.map((x,i)=>({index:i,phase:x.phase.toJSON(),section:x.event.section,method:x.event.method,data:x.event.data.toJSON(),topics:x.topics.map(t=>t.toHex())}));
        const own=all.filter(x=>x.phase.applyExtrinsic===index);
        const terminal=own.filter(x=>x.section==='system'&&['ExtrinsicSuccess','ExtrinsicFailed'].includes(x.method));
        if(terminal.length!==1)throw new Error('missing or multiple terminal dispatch events');
        const failed=result.events.find(x=>x.event.section==='system'&&x.event.method==='ExtrinsicFailed');
        let error=null;
        if(failed){const e=failed.event.data[0];if(e.isModule){const m=api.registry.findMetaError(e.asModule);error={section:m.section,name:m.name};}else error={section:'system',name:e.type};}
        const fee=own.find(x=>x.section==='transactionPayment'&&x.method==='TransactionFeePaid');
        Object.assign(log,{block_hash:blockHash,parent_hash:header.parentHash.toHex(),block_number:header.number.toNumber(),extrinsic_index:index,dispatch_error:error,events:own,block_events:all,fee:fee?fee.data[1]:null});
        resolve(json(log));
      } catch(e){reject(e);}
    }).then(u=>{unsub=u;log.rpc_ack_ns=now();if(ended)unsub();}).catch(fail);
  });
}
async function submit(api,requests) {
  await signAll(api,requests);
  // All signatures/nonces prepared before any request is released.
  const barrier=now();
  const rows=await Promise.all(requests.map(r=>broadcast(api,r)));
  const latest=rows.map(x=>BigInt(x.rpc_started_ns)).reduce((a,b)=>a>b?a:b);
  const earliest=rows.map(x=>BigInt(x.in_block_ns)).reduce((a,b)=>a<b?a:b);
  return {barrier_released_ns:barrier,rpc_overlap:rows.length>1?latest<earliest:null,submissions:rows};
}
module.exports={submit,json,now};
