"use strict";
const fs=require('fs'),path=require('path'),os=require('os');
const common=require('../scenarios/s314_live_common'),b1=require('../scenarios/s314_b1_live_smoke'),b2=require('../scenarios/s314_b2_live_smoke'),g3=require('../scenarios/s314_g3_live_smoke');
const base=require('./fixture_adapter'),timed=require('./timed_transport');
const {ContractPromise}=common.dep('@polkadot/api-contract');
const ROOT='/home/gz785/code/exp2';
const GAS={refTime:'1000000000000',proofSize:'3000000'};
const json=x=>JSON.parse(JSON.stringify(x,(_k,v)=>typeof v==='bigint'?v.toString():v));
const write=(root,name,j)=>fs.writeFileSync(path.join(root,name),JSON.stringify(json(j),null,2)+'\n',{flag:'wx'});
function resources(pid){const read=n=>{try{return fs.readFileSync('/proc/'+pid+'/'+n,'utf8')}catch(e){return null}};return {at_ns:timed.now(),load:os.loadavg(),free_memory:os.freemem(),total_memory:os.totalmem(),client_cpu:process.cpuUsage(),node_stat:read('stat'),node_io:read('io'),node_status:read('status'),meminfo:fs.readFileSync('/proc/meminfo','utf8'),diskstats:fs.readFileSync('/proc/diskstats','utf8')};}
async function main(){
 const spec=JSON.parse(fs.readFileSync(process.argv[2])),api=await common.connect(spec.rpc);let head=null;
 const unsubHead=await api.rpc.chain.subscribeNewHeads(h=>{head={number:h.number.toNumber(),hash:h.hash.toHex(),observed_ns:timed.now()}});
 write(spec.output_root,'runtime-metadata.json',{hex:api.runtimeMetadata.toHex()});
 if(spec.artifact_root)fs.copyFileSync(path.join(spec.artifact_root,(spec.baseline==='B1'?'market_pure_contract':'market_orderbook')+'.json'),path.join(spec.output_root,'contract-metadata.json'));
 let success=0;
 try{
  for(const input of spec.units){
   const root=input.output_root;fs.mkdirSync(root,{recursive:true});write(root,'input.json',input);
   const preparationStart=timed.now(),environmentBefore=resources(spec.node_pid);
   const id={TransferMeta:'LEG-META-01',IssueRight:'LEG-ISSUE-01',TransferRight:'LEG-RESALE-01'}[input.operation];
   const scenario=structuredClone(JSON.parse(fs.readFileSync(ROOT+'/configs/phase4/formal-scenarios-v1.json')).scenarios.find(s=>s.scenario_id===id));scenario.operation=input.operation;
   const baseline=JSON.parse(fs.readFileSync(ROOT+'/configs/baselines/'+input.baseline.toLowerCase()+'.json'));
   const context=base.contextFor(input,{run_id:input.run_id,created_at:new Date().toISOString(),experiment_family:'H4',baseline:{baseline_id:input.baseline,head:baseline.git.head,worktree:baseline.git.worktree,dirty:false},inputs:{seed:2026091904,workload:{path:'configs/h4_v1/config.json'},workload_id:'h4-reused-v1'}},scenario);
   context.api=api;context.cacheFixture=async()=>{};
   async function read(f,hash){
    context.snapshotBuyerRole=f.state.buyerRole;context.snapshotSellerRole=f.state.sellerRole;
    const b=input.baseline==='B1'?await base.b1Snapshot(context,f.state,hash):await base.runtimeSnapshot(context,input.baseline,f.state,hash);
    if(input.baseline==='B1' && /^[0-3]$/.test(b.snapshot.authorization.status))b.snapshot.authorization.status=['Active','Revoked','Expired','Consumed'][Number(b.snapshot.authorization.status)];
    const at=await api.at(hash),balances={};
    for(const [role,a] of Object.entries(f.accounts)){const x=await at.query.system.account(a.address);balances[role]={address:a.address,account_hex:common.accountHex(a),free:x.data.free.toString(),reserved:x.data.reserved.toString(),nonce:x.nonce.toString()};}
    if(f.contractAddress){const x=await at.query.system.account(f.contractAddress);balances.CONTRACT={address:f.contractAddress,account_hex:common.accountHex(f.contractAddress),free:x.data.free.toString(),reserved:x.data.reserved.toString(),nonce:x.nonce.toString()};}
    const h=await api.rpc.chain.getHeader(hash);return {block_hash:hash,block_number:h.number.toNumber(),business:b.snapshot,record:b.record||null,balances};
   }
   context.executeFixture=async f=>{
    const contract=f.contractAddress?new ContractPromise(api,JSON.parse(fs.readFileSync(path.join(spec.output_root,'contract-metadata.json'))),f.contractAddress):null;
    const pending=await api.rpc.author.pendingExtrinsics();if(pending.length)throw new Error('pending transactions before order');
    const startHash=(await api.rpc.chain.getHeader()).hash.toHex(),pre=await read(f,startHash);
    if(pre.business.order.exists || pre.record)throw new Error('order fixture not fresh');
    for(const key of ['active_orders','funded_orders','active_exposure_value'])if(BigInt(pre.business.market[key]||0)!==0n)throw new Error('market is not empty: '+key);
    write(root,'pre.json',pre);
    const genesis=api.genesisHash.toHex(),op=input.operation;
    function request(phase,role){
     const signed=f.signed;let args,value='0';
     if(phase==='create'){
      let encoded;
      if(contract){const message=contract.abi.findMessage(input.baseline==='B1'?'createOrder':'relayCreateOrder'),type=message.args[2].type;encoded=contract.abi.registry.createType(type.lookupName||type.type,signed.sellerCommitment).toU8a();}
      else encoded=api.registry.createType(api.tx.marketRuntime.createOrder.meta.args[2].type.toString(),signed.sellerCommitment).toU8a();
      const seller=f.accounts[f.state.sellerRole];const sig=seller.sign(common.sellerSigningPayload(genesis,op,encoded));
      if(input.baseline==='G3')signed.sellerSignature=sig;else signed.signature=sig;
      const witness=op==='TransferRight'?{SellerSnapshot:b2.pricePolicy(common.PRICES[op])}:'Authorization';
      args=[signed.order,witness,signed.sellerCommitment,sig];
     }else if(phase==='fund'){
      const x=input.baseline==='G3'?g3.buildSignedBuyer(api,genesis,f.accounts[role],op,signed.orderId,common.PRICES[op],signed.order.expiry,0):(input.baseline==='B1'?b1:b2).buildSignedBuyer(contract||api,genesis,f.accounts[role],signed,0);
      args=[signed.orderId,x.commitment||x.buyerCommitment,x.signature||x.buyerSignature];value=input.baseline==='B1'?signed.order.price:'0';
     }else args=[signed.orderId];
     const method=input.baseline==='G3'?{create:'relayCreateOrder',fund:'relayFundOrder',settle:'relaySettle'}[phase]:{create:'createOrder',fund:'fundOrder',settle:'settle'}[phase];
     const tx=contract?contract.tx[method]({gasLimit:api.registry.createType('WeightV2',GAS),storageDepositLimit:null,value},...args):api.tx.marketRuntime[method](...args);
     return {tx,signer:f.accounts[role]};
    }
    const handles=[];const fixtureReady=timed.now();
    try{
     for(const [phase,role] of [['create',f.roles.create],['fund',f.roles.fund],['settle',f.roles.settle]]){
      const h=await timed.submit(api,()=>request(phase,role),phase,head);handles.push(h);await h.included;
     }
     await Promise.all(handles.map(h=>h.finalized));await Promise.all(handles.map(h=>h.acked));
     const measurementEnd=timed.now();
     const rows=[];for(const h of handles)rows.push(await timed.evidence(api,h));
     const postHash=rows[2].block_hash,post=await read(f,postHash);write(root,'post.json',post);
     const blocks=[];for(let n=pre.block_number+1;n<=post.block_number;n++){
      const hash=await api.rpc.chain.getBlockHash(n),events=await api.query.system.events.at(hash);
      blocks.push({number:n,hash:hash.toHex(),events:events.map((x,i)=>({index:i,phase:x.phase.toJSON(),section:x.event.section,method:x.event.method,data:x.event.data.toJSON(),topics:x.topics.map(t=>t.toHex())}))});
     }
     write(root,'blocks.json',blocks);write(root,'canonical-order.json',f.signed);write(root,'transactions.json',rows);
     write(root,'execution.json',{...input,execution_complete:true,fixture:f.fixture,timing:{preparation_start:preparationStart,fixture_ready:fixtureReady,measurement_end:measurementEnd,audit_done:timed.now()},preparation_scope:'account funding,isolated contract/market,asset,authorization,outside c0',shared_accounts:['OWNER (development faucet/admin)'],market_policy:'isolated instance each order; pre-existing before measured transaction',resources_before:environmentBefore,resources_after:resources(spec.node_pid),node_execution_ms:null,node_execution_observation:input.instrumentation==='on'?'PENDING_LOG_CORRELATION':'INSTRUMENTATION_OFF',online_calibration:input.calibration_pair_id?'PAIRED':'NOT_THIS_RUN',runtime_metadata_path:path.join(spec.output_root,'runtime-metadata.json'),contract_metadata_path:contract?path.join(spec.output_root,'contract-metadata.json'):null});
     success++;write(root,'complete.json',{status:'EXECUTION_COMPLETE',completed_at:new Date().toISOString()});
     console.log(JSON.stringify({completed:success,planned:spec.units.length,unit:input.episode_id,kind:input.kind}),true);
    }finally{for(const h of handles)h.close();}
    return {completed:true};
   };
   await {B1:base.runB1,B2:base.runB2,G3:base.runG3}[input.baseline](context);
  }
 }finally{unsubHead();await api.disconnect();}
}
main().catch(e=>{console.error(e.stack);process.exit(1)});
