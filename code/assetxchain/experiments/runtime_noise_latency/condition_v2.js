'use strict';
// v2: signed burst, fixed head-relative phase, two RPC connections, block audit.
const fs=require('fs'), path=require('path'), crypto=require('crypto');
const {createRequire}=require('module'), {performance}=require('perf_hooks');
const dep=createRequire(process.env.DRIVER_PACKAGE||'/home/gz785/code/exp2/scripts/phase4/node/package.json');
const {ApiPromise,WsProvider,Keyring}=dep('@polkadot/api');
const {cryptoWaitReady,blake2AsHex}=dep('@polkadot/util-crypto');
const [rpc,variant,noiseText,seedText,output]=process.argv.slice(2);
const noise=Number(noiseText),seed=Number(seedText);
const sleep=ms=>new Promise(r=>setTimeout(r,Math.max(0,ms)));
const must=(ok,msg)=>{if(!ok)throw Error(msg)};
const hex=b=>'0x'+Buffer.from(b).toString('hex');
const digest=s=>'0x'+crypto.createHash('sha256').update(s).digest('hex');
const rows=events=>events.map(({event,phase})=>({section:event.section,method:event.method,
 data:event.data.map(v=>v.toHex()),json:event.data.toJSON(),phase:phase.toJSON()}));
async function bounded(p,ms,label){
 let t;try{return await Promise.race([p,new Promise((_,bad)=>{t=setTimeout(()=>bad(Error(label+' timeout')),ms)})])}
 finally{clearTimeout(t)}
}
async function main(){
 must(Number.isInteger(noise)&&noise>=0&&['dual_prefix','single_tree'].includes(variant),'arguments');
 must(!fs.existsSync(output),'refusing output overwrite');
 await cryptoWaitReady();
 const provider=new WsProvider(rpc),noiseProvider=new WsProvider(rpc);
 const api=await ApiPromise.create({provider,noInitWarn:true});
 await noiseProvider.isReady;
 const k=new Keyring({type:'sr25519'});
 const alice=k.addFromUri('//Alice'),bob=k.addFromUri('//Bob'),buyer=k.addFromUri('//Charlie');
 const workers=Array.from({length:6},(_,i)=>k.addFromUri('//Alice//noise-v2/'+i));
 const nonce=new Map(),heads=[];
 const result={schema:'runtime-noise-v2',variant,noise_offered:noise,seed,
  metric:'presigned send to first isInBlock callback; excludes subsequent block/header queries',
  started_utc:new Date().toISOString(),setup:[],noise:[],heads,checks:{},status:'running'};
 const save=()=>fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n');
 fs.mkdirSync(path.dirname(output),{recursive:true});
 let unsubscribeHeads;
 async function signed(tx,account){
  if(!nonce.has(account.address))nonce.set(account.address,(await api.rpc.system.accountNextIndex(account.address)).toNumber());
  const n=nonce.get(account.address);nonce.set(account.address,n+1);
  await tx.signAsync(account,{nonce:n,era:0,tip:0});return tx;
 }
 function watch(tx,label){
  const started=performance.now();let unsub,done=false;
  return bounded(new Promise((resolve,reject)=>{
   const finish=(err,row)=>{if(done)return;done=true;if(unsub)unsub();err?reject(err):resolve(row)};
   tx.send(r=>{
    const observed=performance.now(); // Freeze clock before any asynchronous evidence RPC.
    if(r.status.isInBlock){
     const events=rows(r.events);
     finish(null,{label,hash:tx.hash.toHex(),nonce:tx.nonce.toString(),signed_hex:tx.toHex(),
      sent_ms:started,observed_ms:observed,latency_ms:observed-started,
      block_hash:r.status.asInBlock.toHex(),events,
      success:events.some(e=>e.section==='system'&&e.method==='ExtrinsicSuccess')&&!r.dispatchError});
    } else if(r.status.isInvalid||r.status.isDropped||r.status.isUsurped){
     finish(Error(label+': '+r.status.type));
    }
   }).then(u=>{unsub=u;if(done)u()}).catch(e=>finish(e));
  }),90000,label).finally(()=>{done=true;if(unsub)unsub()});
 }
 async function setup(tx,account,label){
  const row=await watch(await signed(tx,account),label);
  result.setup.push(row);must(row.success,label+' failed');return row;
 }
 try{
  result.genesis_hash=api.genesisHash.toHex();
  result.runtime=api.runtimeVersion.toJSON();
  result.runtime_code_blake2=blake2AsHex((await api.rpc.state.getStorage(':code')).unwrap().toU8a(true));
  result.key_mode=(await api.query.dataAssets.useHashScatteredKeys()).toJSON();
  result.zero_weight=(await api.query.dataAssets.useZeroWeight()).toJSON();
  must(result.key_mode===false&&result.zero_weight===false,'normal weights and prefix keys required');
  unsubscribeHeads=await api.rpc.chain.subscribeNewHeads(h=>{
   const n=h.number.toNumber();if(!heads.some(x=>x.number===n)) heads.push({number:n,hash:h.hash.toHex(),seen_ms:performance.now()});
  });
  const funding=[];
  for(const account of [...workers,bob,buyer]){
   funding.push(await signed(api.tx.balances.transferAllowDeath(account.address,'10000000000000000'),alice));
  }
  const fundRows=await Promise.all(funding.map((tx,i)=>watch(tx,'fund-'+i)));
  result.setup.push(...fundRows);must(fundRows.every(x=>x.success),'funding');
  const reg=await setup(api.tx.dataAssets.registerAssetCore('probe','noise-v2',digest('probe/'+seed),1024),alice,'register');
  const aid=reg.events.find(e=>e.section==='dataAssets'&&e.method==='AssetRegistered').data[0];
  const issue=await setup(api.tx.dataAssets.issueCertificate(aid,bob.address,1,null),alice,'issue');
  const cid=issue.events.find(e=>e.section==='dataAssets'&&e.method==='CertificateIssued').data[1];
  const assetChild=hex(':child_storage:default::asset_trie:');
  const rightChild=hex(':child_storage:default::certificate_trie:');
  const assetKey=hex(Buffer.concat([Buffer.from('assets/'),Buffer.from(aid.slice(2),'hex')]));
  const certKey=aid+cid.slice(2);
  const at=issue.block_hash;
  const [assetValue,certAsset,certRight]=await Promise.all([
   provider.send('childstate_getStorage',[assetChild,assetKey,at]),
   provider.send('childstate_getStorage',[assetChild,certKey,at]),
   provider.send('childstate_getStorage',[rightChild,certKey,at])
  ]);
  result.layout={at,asset_id:aid,certificate_id:cid,assetChild,rightChild,assetKey,certKey,
   asset_in_asset:assetValue!==null,certificate_in_asset:certAsset!==null,certificate_in_right:certRight!==null};
  must(result.layout.asset_in_asset,'asset missing');
  must(variant==='dual_prefix'?certAsset===null&&certRight!==null:certAsset!==null&&certRight===null,'on-chain layout mismatch');
  const target=await signed(api.tx.dataAssets.transferCertificate(aid,cid,buyer.address),bob);
  const transactions=[];
  for(let i=0;i<noise;i++){
   const owner=i%workers.length;
   const tx=await signed(api.tx.dataAssets.registerAssetCore('background-'+i,'noise-v2',digest(seed+'/background/'+i),256),workers[owner]);
   transactions.push({tx,owner});
  }
  result.fixture={value_description:'noise-v2',noise_hashes_digest:digest(transactions.map(x=>x.tx.method.toHex()).join('')),
   target_method:target.method.toHex(),rights_per_probe:1,target_holder:bob.address,target_buyer:buyer.address};
  const prior=heads.at(-1).number;
  await bounded((async()=>{while(heads.at(-1).number<=prior)await sleep(10)})(),20000,'next head');
  const anchor={...heads.at(-1)};
  result.anchor=anchor;
  await sleep(anchor.seen_ms+200-performance.now());
  // Six ordered nonce lanes. No per-noise subscription, rejection handled immediately.
  const noisePromise=Promise.all(workers.map(async(_,owner)=>{
   for(let i=owner;i<transactions.length;i+=workers.length){
    const tx=transactions[i].tx, sent=performance.now();
    const row={index:i,hash:tx.hash.toHex(),nonce:tx.nonce.toString(),signer:workers[owner].address,signed_hex:tx.toHex(),sent_ms:sent};
    result.noise.push(row);
    try{row.rpc_hash=await noiseProvider.send('author_submitExtrinsic',[tx.toHex()]);
     row.accepted=true;row.accepted_ms=performance.now()}
    catch(e){row.accepted=false;row.error=String(e);row.accepted_ms=performance.now()}
   }
  }));
  await sleep(anchor.seen_ms+4000-performance.now());
  result.target=await watch(target,'target');
  result.target.phase_ms=result.target.sent_ms-anchor.seen_ms;
  must(result.target.success,'target dispatch failed');
  await noisePromise;
  result.noise.sort((a,b)=>a.index-b.index);
  // Audit full blocks after the timed callback; map inclusion/success to exact tx hash and index.
  const pending=new Set([target.hash.toHex(),...result.noise.filter(x=>x.accepted).map(x=>x.hash)]);
  result.blocks=[];
  for(let number=anchor.number+1;pending.size&&number<=anchor.number+20;number++){
   await bounded((async()=>{while(heads.at(-1).number<number)await sleep(30)})(),90000,'drain block');
   const bh=await api.rpc.chain.getBlockHash(number);
   const block=await api.rpc.chain.getBlock(bh);
   const a=await api.at(bh),events=rows(await a.query.system.events());
   const extrinsics=block.block.extrinsics.map((x,index)=>({hash:x.hash.toHex(),index,section:x.method.section,method:x.method.method}));
   result.blocks.push({number,hash:bh.toHex(),extrinsics,events});
   extrinsics.forEach(x=>pending.delete(x.hash));
  }
  must(pending.size===0,'accepted extrinsics not included');
  for(const row of [...result.noise,result.target]){
   const b=result.blocks.find(b=>b.extrinsics.some(e=>e.hash===row.hash));
   if(!b)continue;
   const e=b.extrinsics.find(e=>e.hash===row.hash);
   row.inclusion={number:b.number,hash:b.hash,index:e.index,block_extrinsic_count:b.extrinsics.length};
   row.dispatch_success=b.events.some(v=>v.section==='system'&&v.method==='ExtrinsicSuccess'&&v.phase.applyExtrinsic===e.index);
  }
  const after=await api.at(result.target.block_hash);
  const cert=(await after.call.dataAssetsApi.getCertificate(aid,cid)).unwrap();
  result.post_certificate=cert.toJSON();
  must(cert.owner.toString()===buyer.address,'certificate owner mismatch');
  result.checks={
   layout:true,normal_weight:true,prefix:true,target_success:true,holder:true,
   attempted:result.noise.length,
   accepted:result.noise.filter(x=>x.accepted).length,
   in_block:result.noise.filter(x=>x.inclusion).length,
   dispatch_success:result.noise.filter(x=>x.dispatch_success).length,
   accepted_before_target:result.noise.filter(x=>x.accepted&&x.accepted_ms<result.target.sent_ms).length,
   same_block_as_target:result.noise.filter(x=>x.inclusion?.hash===result.target.block_hash).length,
   earlier_in_target_block:result.noise.filter(x=>x.inclusion?.hash===result.target.block_hash&&x.inclusion.index<result.target.inclusion.index).length,
   target_phase_ok:Math.abs(result.target.phase_ms-4000)<100
  };
  must(result.checks.attempted===noise&&result.checks.dispatch_success===noise,'noise coverage failed');
  result.target.completion_blocks=result.target.inclusion.number-anchor.number;
  result.status='complete';result.finished_utc=new Date().toISOString();save();
  console.log(JSON.stringify({variant,noise,latency_ms:result.target.latency_ms,checks:result.checks}));
 } catch(e){result.status='failed';result.error=String(e);save();throw e}
 finally{if(unsubscribeHeads)unsubscribeHeads();await api.disconnect();await noiseProvider.disconnect()}
}
main().catch(e=>{console.error(e.stack);process.exitCode=1});
