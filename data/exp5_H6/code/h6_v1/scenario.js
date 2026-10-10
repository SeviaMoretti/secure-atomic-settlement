'use strict';
// Shared driver specification. Its transitions are checked against the plan
// and real ABIs by preflight.js before any pilot batch.
const OPS=['TransferMeta','IssueRight','TransferRight'];
const TERMS=['Settled','Cancelled','Refunded'];
const CONDITIONS=['F0','F1','F2','F3'];
function scenario(operation,terminal,condition){
 if(!OPS.includes(operation)||!TERMS.includes(terminal)||!CONDITIONS.includes(condition))throw Error('unknown H6 cell');
 const seller=operation==='TransferRight'?'RIGHT_SELLER':'OWNER';
 const buyer=operation==='TransferRight'?'BUYER_2':'BUYER_1';
 const preparation=[{method:'relayCreateOrder',role:'RELAYER',label:'create'}];
 if(condition==='F2')preparation.push({method:'syncProjection',role:'RELAYER',label:'seed-open'});
 if(terminal==='Cancelled')preparation.push({method:'relayCancelOpen',role:seller,label:'cancel'});
 else {
  preparation.push({method:'relayFundOrder',role:buyer,label:'fund'});
  preparation.push(terminal==='Settled'?{method:'relaySettle',role:'RELAYER',label:'settle'}:{method:'relayRefundFunded',role:buyer,label:'refund',waitForGrace:true});
 }
 if(['F0','F3'].includes(condition))preparation.push({method:'syncProjection',role:'RELAYER',label:'seed-terminal'});
 return {operation,terminal,condition,seller,buyer,artifact:condition==='F3'?'fault':'normal',preparation,injection:condition==='F3'?{method:'h6CorruptProjection',role:'MARKET_OK',label:'inject'}:null,recovery:[{method:'syncProjection',role:'RELAYER',label:'sync1'},{method:'syncProjection',role:'RELAYER',label:'sync2'}]};
}
module.exports={OPS,TERMS,CONDITIONS,scenario};
