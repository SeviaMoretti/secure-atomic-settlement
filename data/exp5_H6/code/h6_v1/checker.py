"""Offline checks on saved block/transaction/state evidence. No live RPC."""
import json,sys,subprocess,os,copy,collections
from pathlib import Path
NODE='/home/gz785/.local/node-v24.15.0-linux-x64/bin/node'
HERE=Path(__file__).resolve().parent
def load(p):return json.loads(Path(p).read_text())
def need(ok,msg):
 if not ok:raise AssertionError(msg)
def expected(s):return '0x0001'+s['order_scale'][2:]+s['settlement_option_scale'][2:]
def ledger(before,after,blocks):
 delta=collections.defaultdict(lambda:[0,0,0])
 allowed={'Withdraw','Deposit','Transfer','Reserved','Unreserved','ReserveRepatriated','Endowed','BalanceSet','DustLost','Slashed','Minted','Burned','Issued','Rescinded','Suspended','Restored','Upgraded','Locked','Unlocked','Frozen','Thawed'}
 for b in blocks:
  if not before['block_number']<b['number']<=after['block_number']:continue
  for e in b['events']:
   if e['section']=='contracts' and e['method']=='StorageDepositTransferredAndHeld':
    src,dst,amount=e['data'];delta[src][0]-=int(amount);delta[dst][1]+=int(amount);continue
   if e['section']=='contracts' and e['method']=='StorageDepositTransferredAndReleased':
    src,dst,amount=e['data'];delta[src][1]-=int(amount);delta[dst][0]+=int(amount);continue
   if e['section']!='balances':continue
   name=e['method'];d=e['data'];need(name in allowed,'unknown balance event '+name)
   if name in ['Withdraw','Deposit']:
    delta[d[0]][0]+=(1 if name=='Deposit' else -1)*int(d[1])
   elif name=='Transfer':delta[d[0]][0]-=int(d[2]);delta[d[1]][0]+=int(d[2])
   elif name in ['Reserved','Unreserved']:
    amount=int(d[1])*(1 if name=='Reserved' else -1);delta[d[0]][0]-=amount;delta[d[0]][1]+=amount
   elif name in ['Minted','Burned','DustLost','Slashed','BalanceSet','ReserveRepatriated']:
    raise AssertionError('unexpected environmental balance mutation '+name)
  for x in b['extrinsics']:
   if x['is_signed']:delta[x['signer']][2]+=1
 keys=set(before['balances'])|set(after['balances'])|set(delta)
 for a in keys:
  pre=before['balances'].get(a,{'free':'0','reserved':'0','nonce':'0','frozen':'0'});post=after['balances'].get(a,{'free':'0','reserved':'0','nonce':'0','frozen':'0'})
  for n,k in enumerate(['free','reserved','nonce']):need(int(post[k])-int(pre[k])==delta[a][n],f'unexplained {k} delta {a}: {int(post[k])-int(pre[k])} != {delta[a][n]}')
  need(pre['frozen']==post['frozen'],'frozen balance changed')
 return {a:dict(zip(['free','reserved','nonce'],v)) for a,v in delta.items() if any(v)}
def check(root,payload=True):
 root=Path(root);errors=[];metrics=[];effects=[]
 try:
  i=load(root/'input.json');e=load(root/'execution.json');need(e['execution_complete'],'incomplete execution')
  s={l:load(root/('snapshot-'+l+'.json')) for l in ['terminal','before','after1','after2']};tx=load(root/'transactions.json');blocks=load(root/'blocks.json');bmap={b['hash']:b for b in blocks}
  need(s['terminal']['order']['state']==i['terminal'],'wrong terminal')
  for l,a in s.items():
   need(all(k in a['authority'] for k in ['dataAssets.runtimeOrders','dataAssets.runtimeEscrows','dataAssets.settlementRecords','dataAssets.issueRightAuthorizations','dataAssets.transferMetaAuthorizations','markets.marketRegistrations','asset']),'missing authority coverage')
   need(a['authority']==s['terminal']['authority'],'authority changed at '+l)
   need(a['intent_return_scale']==s['terminal']['intent_return_scale'],'intent changed at '+l)
   need(a['order']['state']==i['terminal'],'order terminal changed')
   need(a['block_hash'] in bmap and bmap[a['block_hash']]['number']==a['block_number'],'snapshot block identity')
   need((a['settlement'] is not None)==(i['terminal']=='Settled'),'settlement presence mismatch')
   need(a['contract_info']['codeHash']==e['fixture']['contract_code_hash'],'contract code changed')
   need(a['contract_info']['trieId']==s['terminal']['contract_info']['trieId'],'contract trie identity changed')
  before=s['before'];fault=i['condition']
  if fault=='F0':need(before['projection_return_scale']==expected(before),'F0 projection not current')
  elif fault=='F1':need(before['projection_return_scale']=='0x0000','F1 projection not absent')
  elif fault=='F2':
   old=load(root/'snapshot-open.json');need(old['order']['state']=='Open','F2 not seeded Open');need(old['projection_return_scale']==expected(old),'F2 seed not faithful');need(before['projection_return_scale']==old['projection_return_scale'] and before['projection_return_scale']!=expected(before),'F2 not stale')
  elif fault=='F3':
   need(s['terminal']['projection_return_scale']==expected(s['terminal']),'F3 seed invalid');need(before['projection_return_scale'] not in ['0x0000',expected(before)],'F3 did not corrupt');need(i['artifact_mode']=='fault','F3 wrong build')
  need(len([t for t in tx if t['phase'] in ['sync1','sync2']])==2,'sync cardinality')
  for l in ['after1','after2']:need(s[l]['projection_return_scale']==expected(s[l]),'projection not equal to authority '+l)
  need(s['after1']['projection_return_scale']==s['after2']['projection_return_scale'],'not idempotent')
  for row in tx:
   need(row['c_in'] and row['c_fin'],'missing timing')
   times=[int(row[k]) for k in ['c0','c1','c_in','c_fin','c_done']];need(times==sorted(times),'backward timing')
   need(row['block_hash']==row['finalized_block_hash'],'finality block differs')
   need(any(x['status']=='InBlock' and x['at_ns']==row['c_in'] for x in row['statuses']),'missing first inclusion callback')
   need(any(x['status']=='Finalized' and x['at_ns']==row['c_fin'] for x in row['statuses']),'missing finalization callback')
   need(not any(x['status'] in ['Invalid','Dropped','Usurped','Retracted','FinalityTimeout'] for x in row['statuses']),'abnormal transaction status')
   b=bmap[row['block_hash']];x=b['extrinsics'][row['extrinsic_index']]
   need(x['hash']==row['tx_hash'] and x['signed_bytes']==row['signed_bytes'],'tx block binding')
   events=[z for z in b['events'] if z['phase'].get('applyExtrinsic')==row['extrinsic_index']]
   need([{k:z[k] for k in ['index','phase','section','method','data']} for z in row['events']]==events,'saved events differ from block')
   need(sum(z['section']=='system' and z['method']=='ExtrinsicSuccess' for z in events)==1,'dispatch not successful')
   if row['phase'] in ['sync1','sync2']:
    own=[z for z in row['contract_events'] if z['name']=='ProjectionSynchronized'];need(len(own)==1,'missing sync event')
    need(own[0]['args']==[e['fixture']['order_id'],i['terminal'],i['terminal']=='Settled'],'sync event projection identity')
    need(not any(z['section'] in ['dataAssets','markets'] for z in events),'sync emitted business mutation event')
    fee=[z for z in events if z['section']=='transactionPayment' and z['method']=='TransactionFeePaid'];need(len(fee)==1 and str(fee[0]['data'][1])==str(row['fee']),'actual fee mismatch')
    phase=row['phase'];a=s['before' if phase=='sync1' else 'after1'];z=s['after1' if phase=='sync1' else 'after2'];need(z['block_hash']==row['block_hash'],'readback not at sync block')
    effects.append({'phase':phase,'balance_event_reconciliation':ledger(a,z,blocks),'contract_info_before':a['contract_info'],'contract_info_after':z['contract_info']})
    rb=load(root/('readback'+phase[-1]+'.json'));need(int(rb['read_start_ns'])>=int(row['c_fin']) and int(rb['read_done_ns'])>=int(rb['read_start_ns']),'readback timer')
    weight=next(z['data'][0]['weight'] for z in events if z['section']=='system' and z['method']=='ExtrinsicSuccess')
    metrics.append({'phase':phase,'submission_to_inclusion_ms':(int(row['c_in'])-int(row['c1']))/1e6,'submission_to_finalization_ms':(int(row['c_fin'])-int(row['c1']))/1e6,'finalized_readback_ms':(int(rb['read_done_ns'])-int(rb['read_start_ns']))/1e6,'recovery_visible_ms':(int(rb['read_done_ns'])-int(row['c1']))/1e6,'actual_fee':str(row['fee']),'post_dispatch_weight':weight,'projection_insert_calls_source_derived':1})
    metrics[-1].update(contract_storage_items_delta=int(z['contract_info']['storageItems'])-int(a['contract_info']['storageItems']),contract_storage_bytes_delta=int(z['contract_info']['storageBytes'])-int(a['contract_info']['storageBytes']),storage_deposit_events=[v for v in events if v['section']=='contracts' and v['method'].startswith('StorageDeposit')])
  recovery_blocks=[b for b in blocks if before['block_number']<b['number']<=s['after2']['block_number']]
  allowed_hashes={t['tx_hash'] for t in tx if t['phase'] in ['sync1','sync2']}
  need({x['hash'] for b in recovery_blocks for x in b['extrinsics'] if x['is_signed']}==allowed_hashes,'uncontrolled business tx during recovery')
  if fault=='F3':effects.append({'phase':'inject','balance_event_reconciliation':ledger(s['terminal'],before,blocks)})
  if i['terminal']=='Refunded':
   wait=load(root/'refund-wait.json');need(wait['observed_block']>wait['deadline'],'refund did not wait for deadline')
  if payload:
   env=os.environ.copy();env['POLKADOT_DRIVER_ROOT']='/home/gz785/code/exp2/scripts/phase4/node'
   r=subprocess.run([NODE,str(HERE/'payload_check.js'),str(root)],capture_output=True,text=True,env=env);need(r.returncode==0,'payload verification '+r.stdout[-2500:]+r.stderr[-500:])
 except Exception as ex:errors.append(str(ex))
 return {'schema':'h6-check-v1','status':'FAIL' if errors else 'PASS','errors':errors,'metrics':metrics,'environmental_effects':effects,'scope':'complete order/settlement SCALE equality; all authority pallet storage plus target child-trie objects; fees/balances/nonces reconciled from events; I11 excluded'}
if __name__=='__main__':
 p=Path(sys.argv[1]);r=check(p);(p/'checker.json').write_text(json.dumps(r,indent=2)+'\n');print(json.dumps({'status':r['status'],'errors':r['errors']}));sys.exit(r['status']!='PASS')
