'use strict';
const fs=require('fs'),path=require('path');
const common=require('../scenarios/s314_live_common');
const {TypeRegistry,Metadata}=common.dep('@polkadot/types');
const {Abi}=common.dep('@polkadot/api-contract');
const {cryptoWaitReady,signatureVerify}=common.dep('@polkadot/util-crypto');
const {hexToU8a,u8aToHex}=common.dep('@polkadot/util');
const root=process.argv[2];
const load=n=>JSON.parse(fs.readFileSync(path.join(root,n)));
const bytes=x=>typeof x==='string'?hexToU8a(x):Uint8Array.from(Array.isArray(x)?x:Object.values(x));
async function main(){
 await cryptoWaitReady();
 const info=load('execution.json'),cache=load('cached-intent.json'),registry=new TypeRegistry();
 const metadata=new Metadata(registry,load('runtime-metadata.json').hex);registry.setMetadata(metadata);
 let abi=null,encode;
 if(info.baseline==='B2'){
   const pallet=metadata.asLatest.pallets.find(p=>p.name.toString()==='MarketRuntime');
   const variants=registry.lookup.getSiType(pallet.calls.unwrap().type).def.asVariant.variants;
   const fields=variants.find(v=>v.name.toString()==='create_order').fields;
   encode=(i,x)=>registry.createType('Lookup'+fields[i].type.toString(),x).toU8a();
 }else{
   const doc=load('contract-metadata.json');abi=new Abi(doc);
   if(doc.source.hash!==info.fixture.contract_code_hash)throw new Error('ABI artifact code hash differs from deployed code');
   const method=abi.findMessage(info.baseline==='B1'?'createOrder':'relayCreateOrder');
   encode=(i,x)=>abi.registry.createType(method.args[i].type.lookupName||method.args[i].type.type,x).toU8a();
 }
 const signed=cache.signed,originalSig=bytes(signed.signature||signed.sellerSignature);
 const payload=common.sellerSigningPayload(info.fixture.genesis_hash,info.operation,encode(2,signed.sellerCommitment));
 if(!signatureVerify(payload,originalSig,signed.order.seller).isValid)throw new Error('original cached seller signature invalid');
 if(common.computeOrderId(info.fixture.genesis_hash,info.operation,encode(0,signed.order))!==signed.orderId)throw new Error('cached order id does not bind original canonical fields');
 const sets=fs.readdirSync(root).filter(n=>/^submission-set-\d+.json$/.test(n)).sort().map(load);
 const decoded=[];
 for(const row of sets.flatMap(s=>s.submissions)){
   const ex=registry.createType('Extrinsic',row.signed_bytes);
   if(ex.hash.toHex()!==row.tx_hash||ex.nonce.toString()!==row.nonce)throw new Error('outer transaction hash or nonce differs');
   if(common.accountHex(ex.signer.toString())!==common.accountHex(row.account))throw new Error('outer signer differs');
   let args=ex.method.args,method=ex.method.method;
   if(abi){const call=abi.decodeMessage(ex.method.args[4].toU8a());args=call.args;method=call.message.identifier;}
   if(info.variant==='V2'){
     if(u8aToHex(args[3].toU8a())!==u8aToHex(originalSig))throw new Error('field replacement changed original signature');
     if(u8aToHex(args[0].toU8a())===u8aToHex(encode(0,signed.order)))throw new Error('V2 did not change signed order bytes');
   }
   if(info.variant==='V4'){
     const m=load('composite-mutation.json');
     if(u8aToHex(args[2].toU8a())!==u8aToHex(bytes(m.signature)))throw new Error('V4 changed original buyer signature');
     if(args[1].get('price').toString()!==m.mutated)throw new Error('V4 actual submitted field differs from evidence');
   }
   const events=[];
   for(const e of row.events){
     if(e.section==='contracts'&&e.method==='ContractEmitted'&&abi){
       const d=abi.decodeEvent({topics:e.topics.map(t=>registry.createType('Hash',t)),event:{data:[e.data[0],hexToU8a(e.data[1])]}});events.push({method:d.event.identifier.split('::').pop(),qualified_identifier:d.event.identifier,args:d.args.map(x=>x.toJSON())});
     }else if(['dataAssets','marketRuntime'].includes(e.section))events.push({method:e.method,args:e.data});
   }
   decoded.push({tx_hash:row.tx_hash,method,events});
 }
 const settlements=decoded.flatMap(x=>x.events.filter(e=>e.method==='SettlementFinalized'));
 if(settlements.length!==( ['V0','V3','Q1'].includes(info.variant)?1:0))throw new Error('decoded settlement event cardinality: '+JSON.stringify(decoded));
 console.log(JSON.stringify({status:'PASS',original_signature_verified:true,decoded,settlements}));
}
main().catch(e=>{console.log(JSON.stringify({status:'FAIL',error:e.stack}));process.exit(1);});
