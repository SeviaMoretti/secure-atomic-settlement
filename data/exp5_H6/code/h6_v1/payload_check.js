'use strict';
const fs=require('fs'),path=require('path'),common=require('../scenarios/s314_live_common');
const {TypeRegistry,Metadata}=common.dep('@polkadot/types');const {Abi}=common.dep('@polkadot/api-contract');
const {cryptoWaitReady,signatureVerify}=common.dep('@polkadot/util-crypto');
const root=process.argv[2],load=n=>JSON.parse(fs.readFileSync(path.join(root,n)));
const assert=(x,m)=>{if(!x)throw new Error(m)};
async function main(){
 await cryptoWaitReady();const info=load('execution.json'),signed=load('canonical-order.json');
 const registry=new TypeRegistry(),metadata=new Metadata(registry,load('runtime-metadata.json').hex);registry.setMetadata(metadata);
 const doc=load('contract-metadata.json'),abi=new Abi(doc);assert(doc.source.hash===info.fixture.contract_code_hash,'deployed contract identity');
 const create=abi.findMessage('relayCreateOrder'),enc=(i,x)=>abi.registry.createType(create.args[i].type.lookupName||create.args[i].type.type,x).toU8a();
 assert(common.computeOrderId(info.fixture.genesis_hash,info.operation,enc(0,signed.order))===signed.orderId,'canonical order binding');
 const sig=typeof signed.sellerSignature==='string'?signed.sellerSignature:Uint8Array.from(Object.values(signed.sellerSignature));
 assert(signatureVerify(common.sellerSigningPayload(info.fixture.genesis_hash,info.operation,enc(2,signed.sellerCommitment)),sig,signed.order.seller).isValid,'seller signature');
 const methods={create:'relay_create_order',fund:'relay_fund_order',settle:'relay_settle',cancel:'relay_cancel_open',refund:'relay_refund_funded',inject:'h6_corrupt_projection',sync1:'sync_projection',sync2:'sync_projection','seed-open':'sync_projection','seed-terminal':'sync_projection'};
 const decoded=[];
 for(const row of load('transactions.json')){
  const ex=registry.createType('Extrinsic',row.signed_bytes),dest=ex.method.args[0];
  assert(ex.method.section==='contracts'&&ex.method.method==='call','target call');
  assert(common.accountHex((dest.isId?dest.asId:dest).toString())===common.accountHex(info.contract_address),'contract destination');
  assert(ex.hash.toHex()===row.tx_hash&&ex.nonce.toString()===row.nonce&&common.accountHex(ex.signer.toString())===common.accountHex(row.account),'extrinsic identity');
  const call=abi.decodeMessage(ex.method.args[4].toU8a());assert(call.message.identifier===methods[row.phase],'method mismatch '+call.message.identifier);
  assert(row.phase==='create'?common.computeOrderId(info.fixture.genesis_hash,info.operation,call.args[0].toU8a())===signed.orderId:call.args[0].toHex()===signed.orderId,'wrong order id');
  const contractEvents=row.events.filter(e=>e.section==='contracts'&&e.method==='ContractEmitted').map(e=>{
   const d=abi.decodeEvent({topics:e.topics.map(t=>registry.createType('Hash',t)),event:{data:[e.data[0],Buffer.from(e.data[1].slice(2),'hex')]}});return {name:d.event.identifier.split('::').pop(),args:d.args.map(a=>a.toJSON())};
  });
  assert(JSON.stringify(contractEvents)===JSON.stringify(row.contract_events),'contract event decode mismatch');
  decoded.push({phase:row.phase,method:call.message.identifier});
 }
 const pallet=metadata.asLatest.pallets.find(p=>p.name.toString()==='DataAssets');
 const storage=pallet.storage.unwrap().items;const orderType='Lookup'+storage.find(s=>s.name.toString()==='RuntimeOrders').type.asMap.value.toString();
 const recordType='Lookup'+storage.find(s=>s.name.toString()==='SettlementRecords').type.asMap.value.toString();
 for(const label of ['terminal','before','after1','after2']){
  const s=load('snapshot-'+label+'.json'),order=registry.createType(orderType,s.order_scale);
  assert(JSON.stringify(order.toJSON())===JSON.stringify(s.order),'order raw/json mismatch');
  const record=registry.createType('Option<'+recordType+'>',Buffer.from(s.settlement_option_scale.slice(2),'hex'));assert(JSON.stringify(record.toJSON())===JSON.stringify(s.settlement),'record raw/json mismatch');
  const ret=abi.findMessage('getProjection').returnType;
  const projection=abi.registry.createType(ret.lookupName||ret.type,s.projection_return_scale);
  assert(projection.toU8a().length===(s.projection_return_scale.length-2)/2,'projection decoding length');
 }
 console.log(JSON.stringify({status:'PASS',decoded}));
}
main().catch(e=>{console.log(JSON.stringify({status:'FAIL',error:e.stack}));process.exit(1)});
