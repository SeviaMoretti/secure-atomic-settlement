'use strict';
// Exercise all 36 cell plans against frozen ABIs and saved Runtime metadata.
// This is offline coverage of driver inputs, not evidence of on-chain success.
const fs=require('fs'),path=require('path'),assert=require('assert/strict');
const common=require('../scenarios/s314_live_common'),g3=require('../scenarios/s314_g3_live_smoke');
const {Abi}=common.dep('@polkadot/api-contract'),{TypeRegistry,Metadata}=common.dep('@polkadot/types');
const {OPS,TERMS,CONDITIONS,scenario}=require('./scenario');
const ROOT='/home/gz785/code/exp2',load=p=>JSON.parse(fs.readFileSync(p));
const sample=process.argv[2],output=process.argv[3],saved=load(path.join(sample,'canonical-order.json'));
const reg=new TypeRegistry();reg.setMetadata(new Metadata(reg,load(path.join(sample,'runtime-metadata.json')).hex));
const control=load(ROOT+'/reports/r3/x1-e0-v1/g3-control.json');
const abis={normal:new Abi(load(control.artifacts.contract_metadata.path)),fault:new Abi(load(ROOT+'/build/h6-v1-contracts/fault/market_orderbook.json'))};
const normal=abis.normal.messages.map(m=>m.identifier),fault=abis.fault.messages.map(m=>m.identifier);
assert.deepEqual(fault.filter(x=>!normal.includes(x)),['h6_corrupt_projection']);assert(!normal.includes('h6_corrupt_projection'));
for(const name of normal)assert.equal(abis.normal.findMessage(name).selector.toHex(),abis.fault.findMessage(name).selector.toHex());
const rows=[];
const savedSnapshot=load(path.join(sample,'snapshot-terminal.json'));
const orderLookup='Lookup'+reg.metadata.pallets.find(p=>p.name.toString()==='DataAssets').storage.unwrap().items.find(s=>s.name.toString()==='RuntimeOrders').type.asMap.value;
for(const op of OPS)for(const terminal of TERMS)for(const condition of CONDITIONS){
 const cell=scenario(op,terminal,condition),abi=abis[cell.artifact],errors=[],calls=[];
 try{
  const order=structuredClone(saved.order);order.operation=op;order.objectType=op==='TransferRight'?'RightToken':'MetaToken';
  order.parentAssetId=op==='TransferRight'?'0x'+'23'.repeat(32):null;
  order.rightParametersHash=op==='TransferMeta'?null:'0x'+'34'.repeat(32);
  const seller=structuredClone(saved.sellerCommitment);seller.operation=op;seller.rightParametersHash=order.rightParametersHash;
  const buyer={orderId:saved.orderId,buyer:order.seller,price:order.price,currency:'Native',expiresAt:order.expiry,buyerNonce:0};
  const policy={referencePrice:order.price,floorRatioBps:10000,signedMarketFeeCapBps:0,safetyFeeBps:0};
  const signature=new Uint8Array(64).fill(7),steps=[...cell.preparation,...(cell.injection?[cell.injection]:[]),...cell.recovery];
  let state=null,projection=null,record=false,seenRecovery=0;
  for(const step of steps){
   const args=step.label==='create'?[order,op==='TransferRight'?{SellerSnapshot:policy}:'Authorization',seller,signature]:step.label==='fund'?[saved.orderId,buyer,signature]:[saved.orderId];
   const msg=abi.findMessage(step.method),bytes=msg.toU8a(args),decoded=abi.decodeMessage(bytes);
   assert.equal(decoded.message.identifier,msg.identifier);assert.equal(decoded.args.length,args.length);
   // Exercise the exact contracts.call WeightV2 encoding path offline.
   const weight=reg.createType('WeightV2',g3.GAS_LIMIT);assert.equal(weight.refTime.toBn().toString(),g3.GAS_LIMIT.refTime);
   if(step.label==='create'){assert.equal(state,null);state='Open';}
   if(step.label==='fund'){assert.equal(state,'Open');assert.equal(step.role,cell.buyer);state='Funded';}
   if(step.label==='cancel'){assert.equal(state,'Open');assert.equal(step.role,cell.seller);state='Cancelled';}
   if(step.label==='settle'){assert.equal(state,'Funded');state='Settled';record=true;}
   if(step.label==='refund'){assert.equal(state,'Funded');assert.equal(step.role,cell.buyer);assert.equal(step.waitForGrace,true);state='Refunded';}
   if(step.label==='seed-open'){assert.equal(state,'Open');projection={state,record};}
   if(step.label==='seed-terminal'){assert.equal(state,terminal);projection={state,record};}
   if(step.label==='inject'){assert.equal(cell.artifact,'fault');assert.equal(step.role,'MARKET_OK');assert.equal(state,terminal);projection={state:'Open',record:false};}
   if(step.label==='sync1'){
    assert.equal(state,terminal);
    if(condition==='F0')assert.deepEqual(projection,{state,record});
    if(condition==='F1')assert.equal(projection,null);
    if(['F2','F3'].includes(condition)){assert.equal(projection.state,'Open');assert.notEqual(projection.state,state);}
   }
   if(step.label==='sync1'||step.label==='sync2'){projection={state,record};seenRecovery++;}
   calls.push({method:msg.identifier,role:step.role,label:step.label,wait_for_real_grace:!!step.waitForGrace});
  }
  assert.equal(seenRecovery,2);assert.equal(state,terminal);assert.deepEqual(projection,{state:terminal,record:terminal==='Settled'});
  for(const m of ['getProjection','getIntent'])assert.equal(abi.decodeMessage(abi.findMessage(m).toU8a([saved.orderId])).args[0].toHex(),saved.orderId);
  // Test both Option branches and every terminal through the contract's actual
  // return type, using Runtime SCALE as the independent expected encoding.
  const returnType=abi.findMessage('getProjection').returnType;
  const terminalOrder=structuredClone(savedSnapshot.order);terminalOrder.state=terminal;
  const orderBytes=Buffer.from(reg.createType(orderLookup,terminalOrder).toU8a()).toString('hex');
  const recordBytes=terminal==='Settled'?savedSnapshot.settlement_option_scale.slice(2):'00';
  for(const raw of ['0x0000','0x0001'+orderBytes+recordBytes]){
   const decoded=abi.registry.createType(returnType.lookupName||returnType.type,Buffer.from(raw.slice(2),'hex'));assert.equal('0x'+Buffer.from(decoded.toU8a()).toString('hex'),raw);
  }
 }catch(e){errors.push(e.message);}
 rows.push({...cell,status:errors.length?'FAIL':'PASS',calls,errors});
}
const result={schema:'h6-offline-cell-audit-v1',created_at:new Date().toISOString(),scope:'offline ABI, argument encoding, role authorization, phase dependencies, fault preconditions and recovery cardinality; not on-chain coverage',cells:rows,passed:rows.filter(x=>x.status==='PASS').length,failed:rows.filter(x=>x.status==='FAIL').length};
fs.writeFileSync(output,JSON.stringify(result,null,2)+'\n',{flag:'wx'});console.log(JSON.stringify({passed:result.passed,failed:result.failed,errors:rows.filter(x=>x.errors.length).map(x=>({operation:x.operation,terminal:x.terminal,condition:x.condition,errors:x.errors}))}));process.exitCode=result.failed?1:0;
