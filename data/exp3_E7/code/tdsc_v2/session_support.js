'use strict';
const crypto = require('crypto');
const common = require('../scenarios/s314_live_common');
const { Keyring } = common.dep('@polkadot/api');
function key(context, label) {
  return '0x' + crypto.createHash('sha256').update(context.args.episode_id + '/' + label).digest('hex');
}
function accounts(context) {
  const k = new Keyring({type:'sr25519'});
  const roles = ['RIGHT_SELLER','BUYER_1','BUYER_2','MARKET_OK','MARKET_DENIED','RELAYER'];
  return Object.fromEntries([['OWNER', k.addFromUri('//Alice')], ...roles.map(r => [r,k.addFromUri('//Alice//e7v2//' + key(context,r).slice(2))])]);
}
async function fund(api, a, evidence) {
  const requests = Object.entries(a).filter(([r])=>r!=='OWNER').map(([role,p])=>({
    tx:api.tx.balances.transferAllowDeath(p.address,(100000n*common.UNIT).toString()),
    signer:a.OWNER,actor:'OWNER',label:'prepare_'+role,
  }));
  const set=await require('./transport').submit(api,requests);
  if(set.submissions.some(x=>x.dispatch_error))throw new Error('episode funding failed');
  evidence.transactions.push(...set.submissions);
}
async function registerG3Asset(context, api, owner, label, evidence) {
  const tx = await common.sendTx(api,'prepare_episode_asset',api.tx.dataAssets.registerAssetCore(
    [...Buffer.from(label)], [...Buffer.from(context.args.episode_id)], key(context,'asset-raw'),1024
  ),owner);
  common.recordTx(evidence,tx);
  const event = common.findEvent(tx.events,'dataAssets','AssetRegistered');
  if(!event) throw new Error('AssetRegistered missing');
  return event.event.data[0].toHex();
}
module.exports={key,accounts,fund,registerG3Asset};
