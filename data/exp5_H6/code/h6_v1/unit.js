'use strict';
const fs=require('fs'),path=require('path');
const common=require('../scenarios/s314_live_common'),g3=require('../scenarios/s314_g3_live_smoke');
const base=require('../h4_v1/fixture_adapter'),timed=require('../h4_v1/timed_transport');
const {scenario:makeScenario}=require('./scenario');
const {ContractPromise}=common.dep('@polkadot/api-contract');
const ROOT='/home/gz785/code/exp2';
const json=x=>JSON.parse(JSON.stringify(x,(_k,v)=>typeof v==='bigint'?v.toString():v));
const hex=x=>'0x'+Buffer.from(x).toString('hex');
const write=(p,x)=>fs.writeFileSync(p,JSON.stringify(json(x),null,2)+'\n',{flag:'wx'});
const assert=(x,msg)=>{if(!x)throw new Error(msg)};
async function main(){
 const input=JSON.parse(fs.readFileSync(process.argv[2])),root=input.output_root;
 const api=await common.connect(input.rpc),started=timed.now();let f,contract;
 const transactions=[],snapshots={},handles=[];
 const metadata=JSON.parse(fs.readFileSync(path.join(input.artifact_root,'market_orderbook.json')));
 write(path.join(root,'runtime-metadata.json'),{hex:api.runtimeMetadata.toHex()});
 write(path.join(root,'contract-metadata.json'),metadata);
 const scenario=structuredClone(JSON.parse(fs.readFileSync(ROOT+'/configs/phase4/formal-scenarios-v1.json')).scenarios.find(s=>s.scenario_id==={TransferMeta:'LEG-META-01',IssueRight:'LEG-ISSUE-01',TransferRight:'LEG-RESALE-01'}[input.operation]));
 const context=base.contextFor(input,{run_id:input.run_id,created_at:new Date().toISOString(),experiment_family:'H6',baseline:{baseline_id:'G3',head:input.normal_commit,worktree:'/home/gz785/code/assetxchain-g3-v2',dirty:false},inputs:{seed:2026091906,workload:{path:'configs/h6_v1/config.json'}}},scenario);
 context.api=api;context.cacheFixture=async()=>{};
 async function call(method,args,role,label){
  const h=await timed.submit(api,()=>({tx:contract.tx[method]({gasLimit:api.registry.createType('WeightV2',g3.GAS_LIMIT),storageDepositLimit:null},...args),signer:f.accounts[role]}),label);
  handles.push(h);await h.included;await h.finalized;await h.acked;
  const row=await timed.evidence(api,h);
  row.contract_events=row.events.filter(e=>e.section==='contracts'&&e.method==='ContractEmitted'&&e.data[0]===f.contractAddress).map(e=>{
   const decoded=contract.abi.decodeEvent({topics:e.topics.map(t=>api.registry.createType('Hash',t)),event:{data:[e.data[0],Buffer.from(e.data[1].slice(2),'hex')]}});return {name:decoded.event.identifier.split('::').pop(),args:decoded.args.map(a=>a.toJSON())};
  });
  transactions.push(row);write(path.join(root,'tx-'+label+'.json'),row);return row;
 }
 async function query(at,method){
  const data=g3.simpleHashCall(api,method,f.signed.orderId);
  const r=(await at.call.contractsApi.call(f.accounts.RELAYER.address,f.contractAddress,'0',g3.GAS_LIMIT,null,data)).toJSON();
  assert(r.result.ok&&r.result.ok.flags.bits===0,'projection/intent query failed '+JSON.stringify(r.result));
  return r.result.ok.data;
 }
 async function snapshot(label,hash){
  const start=timed.now(),at=await api.at(hash),h=await api.rpc.chain.getHeader(hash),q=at.query.dataAssets;
  const order=await q.runtimeOrders(f.signed.orderId),settlement=await q.settlementRecords(f.signed.orderId);
  assert(order.isSome,'missing authoritative order');
  const business={};
  // All top-level business storage, plus child-trie objects via authoritative APIs.
  // Per-block instrumentation counters are explicitly separate.
  const counters={};
  for(const [pallet,storage] of [['dataAssets',q],['markets',at.query.markets]]){
   for(const [name,fn] of Object.entries(storage)){
    const v=fn.meta.type.isMap?(await fn.entries()).map(([k,v])=>[k.toHex(),hex(v.toU8a())]).sort((a,b)=>a[0].localeCompare(b[0])):hex((await fn()).toU8a());
    (['trieModified','blockCertCount','blockPutCalls'].includes(name)?counters:business)[pallet+'.'+name]=v;
   }
  }
  business.asset=hex((await at.call.dataAssetsApi.getAsset(f.state.assetId)).toU8a());
  if(f.state.rightId)business.right=hex((await at.call.dataAssetsApi.getCertificate(f.state.assetId,f.state.rightId)).toU8a());
  const balances={};
  for(const [k,v] of await at.query.system.account.entries())balances[k.args[0].toString()]={free:v.data.free.toString(),reserved:v.data.reserved.toString(),nonce:v.nonce.toString(),frozen:v.data.frozen.toString()};
  const info=await at.query.contracts.contractInfoOf(f.contractAddress);
  const state={label,block_hash:hash,block_number:h.number.toNumber(),read_start_ns:start,read_done_ns:timed.now(),authority:business,counters,order:order.unwrap().toJSON(),order_scale:hex(order.unwrap().toU8a()),settlement:settlement.toJSON(),settlement_option_scale:hex(settlement.toU8a()),projection_return_scale:await query(at,'get_projection'),intent_return_scale:await query(at,'get_intent'),balances,contract_info:info.toJSON(),total_issuance:(await at.query.balances.totalIssuance()).toString()};
  state.read_done_ns=timed.now();snapshots[label]=state;write(path.join(root,'snapshot-'+label+'.json'),state);return state;
 }
 context.executeFixture=async fixture=>{
  f=fixture;contract=new ContractPromise(api,metadata,f.contractAddress);
  write(path.join(root,'canonical-order.json'),f.signed);
  const witness=input.operation==='TransferRight'?{SellerSnapshot:{referencePrice:f.signed.order.price,floorRatioBps:10000,signedMarketFeeCapBps:0,safetyFeeBps:0}}:'Authorization';
  const plan=makeScenario(input.operation,input.terminal,input.condition);write(path.join(root,'scenario.json'),plan);
  let last;
  for(const step of plan.preparation){
   let args=[f.signed.orderId];
   if(step.label==='create')args=[f.signed.order,witness,f.signed.sellerCommitment,f.signed.sellerSignature];
   if(step.label==='fund'){
    const b=g3.buildSignedBuyer(api,api.genesisHash.toHex(),f.accounts[step.role],input.operation,f.signed.orderId,BigInt(f.signed.order.price),f.signed.order.expiry,0);args=[f.signed.orderId,b.buyerCommitment,b.buyerSignature];
   }
   if(step.waitForGrace){
    const o=(await api.query.dataAssets.runtimeOrders(f.signed.orderId)).unwrap().toJSON();
    const deadline=o.graceDeadline??o.grace_deadline;assert(deadline!==null&&deadline!==undefined,'refund deadline missing');
    const start=timed.now();await base.waitUntilAfterBlock(api,Number(deadline));
    write(path.join(root,'refund-wait.json'),{deadline,start_ns:start,end_ns:timed.now(),observed_block:(await api.rpc.chain.getHeader()).number.toNumber()});
   }
   last=await call(step.method,args,step.role,step.label);
   if(step.label==='seed-open')await snapshot('open',last.block_hash);
  }
  await snapshot('terminal',last.block_hash);
  if(input.condition==='F3'){
   const rejected=await contract.query.h6CorruptProjection(f.accounts.RELAYER.address,{gasLimit:api.registry.createType('WeightV2',g3.GAS_LIMIT),storageDepositLimit:null},f.signed.orderId);
   write(path.join(root,'nonadmin-dryrun.json'),{result:rejected.result.toJSON(),output:rejected.output?.toJSON()});
   assert(JSON.stringify(rejected.output?.toJSON()).toLowerCase().includes('notadmin'),'fault entry allowed non-admin');
   last=await call(plan.injection.method,[f.signed.orderId],plan.injection.role,plan.injection.label);
  }
  await snapshot('before',last.block_hash);
  for(let i=1;i<=2;i++){
   const step=plan.recovery[i-1];last=await call(step.method,[f.signed.orderId],step.role,step.label);
   const readStart=timed.now(),s=await snapshot('after'+i,last.block_hash);
   write(path.join(root,'readback'+i+'.json'),{read_start_ns:readStart,read_done_ns:s.read_done_ns,block_hash:s.block_hash,projection_return_scale:s.projection_return_scale});
  }
  const blocks=[];
  for(let n=1;n<=last.block_number;n++){
   const hash=await api.rpc.chain.getBlockHash(n),block=await api.rpc.chain.getBlock(hash),events=await api.query.system.events.at(hash);
   blocks.push({number:n,hash:hash.toHex(),parent_hash:block.block.header.parentHash.toHex(),extrinsics:block.block.extrinsics.map((x,index)=>({index,hash:x.hash.toHex(),signed_bytes:x.toHex(),is_signed:x.isSigned,signer:x.isSigned?x.signer.toString():null,nonce:x.isSigned?x.nonce.toString():null,section:x.method.section,method:x.method.method})),events:events.map((x,index)=>({index,phase:x.phase.toJSON(),section:x.event.section,method:x.event.method,data:x.event.data.toJSON()}))});
  }
  write(path.join(root,'blocks.json'),blocks);write(path.join(root,'transactions.json'),transactions);
  write(path.join(root,'execution.json'),{...input,execution_complete:true,fixture:f.fixture,contract_address:f.contractAddress,accounts:Object.fromEntries(Object.entries(f.accounts).map(([r,a])=>[r,a.address])),started_ns:started,finished_ns:timed.now(),preparation_signed_transactions:blocks.flatMap(b=>b.extrinsics).filter(e=>e.is_signed&&!transactions.some(t=>t.tx_hash===e.hash)).length,all_signed_transactions:blocks.flatMap(b=>b.extrinsics).filter(e=>e.is_signed).length,projection_writes_scope:'one source-level Mapping.insert per successful sync; not a storage I/O trace'});
  return {completed:true};
 };
 try{await base.runG3(context);}finally{for(const h of handles)h.close();await api.disconnect();}
}
main().catch(e=>{console.error(e.stack);process.exit(1)});
