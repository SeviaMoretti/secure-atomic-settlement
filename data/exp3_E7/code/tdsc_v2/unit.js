'use strict';
const fs=require('fs'),path=require('path');
const common=require('../scenarios/s314_live_common');
const b1=require('../scenarios/s314_b1_live_smoke');
const b2=require('../scenarios/s314_b2_live_smoke');
const g3=require('../scenarios/s314_g3_live_smoke');
const base=require('./fixture_adapter');
const transport=require('./transport');
const {ContractPromise}=common.dep('@polkadot/api-contract');
const ROOT='/home/gz785/code/exp2';
const GAS={refTime:'1000000000000',proofSize:'3000000'};
function write(root,name,value){fs.writeFileSync(path.join(root,name),JSON.stringify(transport.json(value),null,2)+'\n',{flag:'wx'});}
function scenario(input){
  const {variant:v,operation:o}=input;
  const id=v==='V0'?{TransferMeta:'LEG-META-01',IssueRight:'LEG-ISSUE-01',TransferRight:'LEG-RESALE-01'}[o]
    :['V1','V4'].includes(v)?o==='TransferRight'?'NEG-RIGHT-EXPIRED-01':'NEG-AUTH-REVOKED-01'
    :v==='V2'?'NEG-SIGN-01':v==='Q1'?'RACE-QUOTA-01':'RACE-RELAYER-01';
  const s=structuredClone(JSON.parse(fs.readFileSync(ROOT+'/configs/phase4/formal-scenarios-v1.json')).scenarios.find(s=>s.scenario_id===id));
  s.operation=o;
  if(['V1','V4'].includes(v))s.attempts[0].phase='fund_order';
  return s;
}
async function main(){
 const input=JSON.parse(fs.readFileSync(process.argv[2]));
 const root=input.output_root;fs.mkdirSync(root,{recursive:true});
 const baseline=JSON.parse(fs.readFileSync(ROOT+'/configs/baselines/'+input.baseline.toLowerCase()+'.json'));
 const context=base.contextFor({...input,repetition:input.repetition}, {
   run_id:input.run_id,created_at:new Date().toISOString(),experiment_family:'E7',
   baseline:{baseline_id:input.baseline,head:baseline.git.head,worktree:baseline.git.worktree,dirty:false},
   inputs:{seed:2026091907,e7_variant:input.variant,workload:{path:'configs/tdsc_v2/e7.json'},workload_id:'e7-reused-v2'}
 },scenario(input));
 let contract=null, cacheEvidence=null;
 const read=async (f,blockHash=null,state=f.state)=>{
   context.snapshotBuyerRole=state.buyerRole;context.snapshotSellerRole=state.sellerRole;
   const head=blockHash?await f.api.rpc.chain.getHeader(blockHash):await f.api.rpc.chain.getHeader();
   const hash=blockHash||head.hash.toHex();
   console.log(JSON.stringify({stage:'snapshot',hash,block:head.number.toNumber()}));
   const business=input.baseline==='B1'?await base.b1Snapshot(context,state,hash):await base.runtimeSnapshot(context,input.baseline,state,hash);
   const authorization_status_encoding=business.snapshot.authorization.status;
   if(input.baseline==='B1' && /^[0-3]$/.test(authorization_status_encoding)){
     business.snapshot.authorization.status=['Active','Revoked','Expired','Consumed'][Number(authorization_status_encoding)];
   }
   const at=await f.api.at(hash);
   const balances={};
   for(const [role,account] of Object.entries(f.accounts)){
     const a=await at.query.system.account(account.address);
     balances[role]={address:account.address,account_hex:common.accountHex(account),free:a.data.free.toString(),reserved:a.data.reserved.toString(),nonce:a.nonce.toString()};
   }
   if(f.contractAddress){
     const a=await at.query.system.account(f.contractAddress);
     balances.CONTRACT={address:f.contractAddress,account_hex:common.accountHex(f.contractAddress),free:a.data.free.toString(),reserved:a.data.reserved.toString(),nonce:a.nonce.toString()};
   }
   return {block_hash:hash,block_number:head.number.toNumber(),business:business.snapshot,authorization_status_encoding,record:business.record||null,balances};
 };
 const buyer=(f,signed,role)=>input.baseline==='G3'
   ?g3.buildSignedBuyer(f.api,f.api.genesisHash.toHex(),f.accounts[role],input.operation,signed.orderId,common.PRICES[input.operation],signed.order.expiry,0)
   :(input.baseline==='B1'?b1:b2).buildSignedBuyer(input.baseline==='B1'?f.state.contract:f.api,f.api.genesisHash.toHex(),f.accounts[role],signed,0);
 function request(f,phase,signed,role,label,mutate=false){
   let args,value='0',tx;
   const witness=input.operation==='TransferRight'?{SellerSnapshot:b2.pricePolicy(common.PRICES[input.operation])}:'Authorization';
   if(phase==='create')args=[signed.order,witness,signed.sellerCommitment,signed.signature||signed.sellerSignature];
   else if(phase==='fund'){
     if(mutate && !f.originalBuyer) f.originalBuyer=structuredClone(buyer(f,signed,role));
     const x=mutate?structuredClone(f.originalBuyer):buyer(f,signed,role);
     const commit=x.commitment||x.buyerCommitment;
     const before=structuredClone(commit);
     if(mutate)commit.price=(BigInt(commit.price)+1n).toString();
     args=[signed.orderId,commit,x.signature||x.buyerSignature];
     value=input.baseline==='B1'?signed.order.price:'0';
     if(mutate)f.mutation={field:'buyer.price',original:before.price,mutated:commit.price,signature_retained:true,original_commitment:before,mutated_commitment:structuredClone(commit),signature:x.signature||x.buyerSignature};
   }else args=[signed.orderId];
   if(input.baseline==='B2')tx=f.api.tx.marketRuntime[{create:'createOrder',fund:'fundOrder',settle:'settle'}[phase]](...args);
   else {
     // B1's existing messages are its authority entry; the experimental
     // variant adds an optimistic on-chain cache without duplicating them.
     const method=input.baseline==='B1'?{create:'createOrder',fund:'fundOrder',settle:'settle'}[phase]
       :{create:'e7AttackCreate',fund:'e7AttackFund',settle:'e7AttackSettle'}[phase];
     tx=contract.tx[method]({gasLimit:f.api.registry.createType('WeightV2',GAS),storageDepositLimit:null,value},...args);
   }
   return {tx,signer:f.accounts[role],actor:role,label,phase,args};
 }
 context.cacheFixture=async f=>{
   const before=await read(f);
   if(input.baseline!=='B2'){
     const name=input.baseline==='B1'?'market_pure_contract':'market_orderbook';
     contract=new ContractPromise(f.api,JSON.parse(fs.readFileSync(path.join(input.artifact_root,name+'.json'))),f.contractAddress);
     const signed_intent_digest='0x'+require('crypto').createHash('sha256').update(JSON.stringify(transport.json(f.signed))).digest('hex');
     const cacheArgs=input.baseline==='B1'?[signed_intent_digest]:[f.signed.orderId,signed_intent_digest];
     const tx=contract.tx.e7CacheIntent({gasLimit:f.api.registry.createType('WeightV2',GAS),storageDepositLimit:null},...cacheArgs);
     const receipt=await transport.submit(f.api,[{tx,signer:f.accounts.MARKET_OK,actor:'MARKET_OK',label:'cache_original_intent'}]);
     if(receipt.submissions[0].dispatch_error)throw new Error('cache write rejected');
     const q=input.baseline==='B1'
       ?await contract.query.e7CacheIntent(f.accounts.MARKET_OK.address,{gasLimit:f.api.registry.createType('WeightV2',GAS),storageDepositLimit:null},null)
       :await contract.query.e7GetCached(f.accounts.MARKET_OK.address,{gasLimit:f.api.registry.createType('WeightV2',GAS),storageDepositLimit:null},f.signed.orderId);
     if(q.result.isErr||!q.output||q.output.isNone)throw new Error('on-chain cached intent is missing');
     cacheEvidence={kind:'contract',address:f.contractAddress,signed_intent_digest,signed:f.signed,receipt,readback:q.output.toJSON(),authority_before:before};
   } else cacheEvidence={kind:'client',signed:f.signed,authority_before:before};
   write(root,'cached-intent.json',cacheEvidence);
 };
 context.executeFixture=async f=>{
   write(root,'runtime-metadata.json',{hex:f.api.runtimeMetadata.toHex()});
   if(input.baseline!=='B2')fs.copyFileSync(path.join(input.artifact_root,(input.baseline==='B1'?'market_pure_contract':'market_orderbook')+'.json'),path.join(root,'contract-metadata.json'));
   const {xxhashAsHex}=common.dep('@polkadot/util-crypto');
   const prefixes={};
   for(const pallet of f.api.runtimeMetadata.asLatest.pallets){
     if(pallet.storage.isSome){const name=pallet.storage.unwrap().prefix.toString();prefixes[xxhashAsHex(name,128).slice(2)]=name;}
   }
   write(root,'storage-prefixes.json',prefixes);
   const states=[f.state];
   if(input.variant==='Q1') states.push({...f.state,signed:f.secondSigned,rightId:f.secondRightId,buyerRole:'BUYER_2'});
   if(input.variant==='Q1') states[0]={...f.state,buyerRole:'BUYER_1'};
   const start=(await f.api.rpc.chain.getHeader()).hash.toHex();
   write(root,'canonical-orders.json',[f.signed,...(f.secondSigned?[f.secondSigned]:[])]);
   const pre=await Promise.all(states.map(s=>read(f,start,s)));
   write(root,'target-pre.json',pre);
   const sets=[];
   async function issue(reqs){
     // Persist intent before broadcasting. These bytes carry real signatures.
     const set=await transport.submit(f.api,reqs);sets.push(set);
     write(root,'submission-set-'+sets.length+'.json',set);return set;
   }
   const v=input.variant;
   if(v==='V0'){
     for(const [phase,role] of [['create',f.roles.create],['fund',f.roles.fund],['settle',f.roles.settle]]) await issue([request(f,phase,f.signed,role,phase)]);
   } else if(v==='V1')await issue([request(f,'fund',f.signed,f.roles.fund,'stale-fund')]);
   else if(v==='V2')await issue([request(f,'create',f.signed,f.roles.create,'field-replacement')]);
   else if(v==='V3')await issue(['RELAYER','OWNER','RIGHT_SELLER','MARKET_OK'].map((r,i)=>request(f,'settle',f.signed,r,'relay-'+i)));
   else if(v==='V4'){
     await issue(Array.from({length:4},(_,i)=>request(f,'fund',f.signed,f.roles.fund,'composite-'+i,true)));
     write(root,'composite-mutation.json',f.mutation);
   } else if(v==='Q1'){
     const race=await issue([request(f,'fund',f.signed,'BUYER_1','quota-1'),request(f,'fund',f.secondSigned,'BUYER_2','quota-2')]);
     const winners=race.submissions.filter(x=>!x.dispatch_error);
     if(winners.length===1)await issue([request(f,'settle',winners[0].actor==='BUYER_1'?f.signed:f.secondSigned,'RELAYER','quota-winner')]);
   }
   const end=(await f.api.rpc.chain.getHeader()).hash.toHex();
   const post=await Promise.all(states.map(s=>read(f,end,s)));
   write(root,'target-post.json',post);
   const blocks=[];
   for(let h=pre[0].block_number+1;h<=post[0].block_number;h++){
     const hash=await f.api.rpc.chain.getBlockHash(h);
     const events=await f.api.query.system.events.at(hash);
     blocks.push({number:h,hash:hash.toHex(),events:events.map((x,i)=>({index:i,phase:x.phase.toJSON(),section:x.event.section,method:x.event.method,data:x.event.data.toJSON()}))});
   }
   write(root,'target-blocks.json',blocks);
   if(contract){
     const q=input.baseline==='B1'
       ?await contract.query.e7CacheIntent(f.accounts.MARKET_OK.address,{gasLimit:f.api.registry.createType('WeightV2',GAS),storageDepositLimit:null},null)
       :await contract.query.e7GetCached(f.accounts.MARKET_OK.address,{gasLimit:f.api.registry.createType('WeightV2',GAS),storageDepositLimit:null},cacheEvidence.signed.orderId);
     write(root,'cached-intent-after.json',{readback:q.output?.toJSON()});
   }
   write(root,'execution.json',{...input,execution_complete:true,checks:'pending-independent-checker',fixture:f.fixture,submitted:sets.reduce((n,x)=>n+x.submissions.length,0),per_extrinsic_state_isolation:'NOT_OBSERVED',state_evidence_scope:'episode-net-effect'});
   if(['V3','V4','Q1'].includes(v)){
     for(const hash of new Set(sets.flatMap(s=>s.submissions.map(t=>t.block_hash)))){
       try { const tr=await f.api.rpc.state.traceBlock(hash,'','','');write(root,'trace-'+hash+'.json',tr.toJSON()); }
       catch(e){write(root,'trace-'+hash+'.error.json',{error:e.message});}
     }
   }
   await f.api.disconnect();
   return {completed:true};
 };
 const fn={B1:base.runB1,B2:base.runB2,G3:base.runG3}[input.baseline];
 await fn(context);
 console.log(JSON.stringify({execution_complete:true,episode_id:input.episode_id}));
}
main().catch(e=>{console.error(e.stack);process.exit(1);});
