"""Recompute episode net-effect checks from raw chain snapshots and receipts.

This does not claim intra-block, per-extrinsic storage isolation.
"""
import copy,json,sys,subprocess,os,re
from pathlib import Path


def load(p): return json.loads(p.read_text())
def check(root):
    root=Path(root); info=load(root/'execution.json'); pre=[load(root/'pre.json')]; post=[load(root/'post.json')]; info['variant']='V0'
    sets=[{'submissions':[{**t,'signed_at_ns':t['sign_end'],'rpc_started_ns':t['c1'],'in_block_ns':t['c_in']} for t in load(root/'transactions.json')]}]
    txs=[t for s in sets for t in s['submissions']]; errors=[]
    def require(x,msg):
        if not x:errors.append(msg)
    variant=info['variant']
    count={'V0':3,'V1':1,'V2':1,'V3':4,'V4':4,'Q1':3}[variant]
    require(len(txs)==count,'target submission cardinality')
    require(len({t['tx_hash'] for t in txs})==len(txs),'distinct signed transaction hashes')
    require(len({(t['block_hash'],t['extrinsic_index']) for t in txs})==len(txs),'unique inclusion positions')
    accepted=[t for t in txs if t['dispatch_error'] is None]
    require(len(accepted)=={'V0':3,'V1':0,'V2':0,'V3':1,'V4':0,'Q1':2}[variant],'dispatch cardinality')
    for t in txs:
        require(t['in_block_ns'] and t['block_hash'] and t['signed_bytes'],'included signed payload evidence')
        require(int(t['signed_at_ns'])<=int(t['rpc_started_ns'])<=int(t['in_block_ns']),'monotonic transport times')
        require(any(e['section']=='system' and e['method'] in ['ExtrinsicSuccess','ExtrinsicFailed'] for e in t['events']),'terminal chain event')
        if t['dispatch_error']:
            e=t['dispatch_error']
            if info['baseline']=='B2':
                allowed={'V1':{'AuthorizationRevoked','RightInvalid'},'V2':{'MalformedCommitment'},'V3':{'AlreadySettled'},'V4':{'MalformedCommitment','AuthorizationRevoked','RightInvalid'},'Q1':{'QuotaExceeded'}}.get(variant,set())
                require(e['section']=='marketRuntime' and e['name'] in allowed,'expected business rejection; not gas/pool/encoding failure')
            else:require(e=={'section':'contracts','name':'ContractReverted'},'contract business revert; not trap/out-of-gas')
    if variant in ['V3','V4','Q1']:
        s=sets[0]; rows=s['submissions']
        require(len(rows)==(2 if variant=='Q1' else 4),'race cardinality')
        require(max(int(t['rpc_started_ns']) for t in rows)<min(int(t['in_block_ns']) for t in rows),'actual broadcast overlap')
        require(max(int(t['signed_at_ns']) for t in rows)<=int(s['barrier_released_ns']),'signatures prepared before release')
    success_events=[e for t in txs for e in t['events'] if e['method']=='SettlementFinalized']
    # B1 events are decoded by contract ABI only in the extended evidence gate.
    if info['baseline']!='B1': require(len(success_events)==(1 if variant in ['V0','V3','Q1'] else 0),'runtime settlement event count')
    if variant in ['V1','V4']:
        if info['operation']=='TransferRight':
            stale=load(root/'stale-intent.json')
            require(stale['observed_block']>stale['cached_valid_until'],'actual expiry before request')
            require(cached['authority_before']['business']['right']['valid'],'right valid at cache time')
        else:
            require(cached['authority_before']['business']['authorization']['status']=='Active','authorization initially active')
            require(pre[0]['business']['authorization']['status']=='Revoked','authorization revoked before request')
    if variant=='V4':
        m=load(root/'composite-mutation.json');require(m['original']!=m['mutated'] and m['signature_retained'],'composite field mutation')
    blocks=load(root/'blocks.json')
    rewards={}
    for b in blocks:
        for e in b['events']:
            if e['section']=='rewards' and e['method']=='RewardPaid':
                rewards[e['data'][0]]=rewards.get(e['data'][0],0)+int(e['data'][1])
    fees={}
    for t in txs:
        require(t['fee'] is not None,'actual fee evidence')
        fees[t['account']]=fees.get(t['account'],0)+int(t['fee'] or 0)
    def business(x):
        z=copy.deepcopy(x['business']);z.pop('observed_at',None);z.pop('state_commitment',None)
        # Account balances are checked independently, without hiding escrow.
        for k in ['buyer_balance','seller_balance','market_balance','royalty_balance','safety_balance']:z['payment'].pop(k,None)
        return z
    if variant in ['V1','V2','V4']:
        require(business(pre[0])==business(post[0]),'rejected episode changed business state')
        for role,a in pre[0]['balances'].items():
            b=post[0]['balances'][role]
            require(int(b['free'])==int(a['free'])+rewards.get(a['address'],0)-fees.get(a['address'],0),f'{role} rejection free-balance conservation')
            require(b['reserved']==a['reserved'],f'{role} rejection reserve conservation')
        require(post[0]['record'] is None,'no success record after rejected episode')
    else:
        winners=[p for p in post if p['business']['order']['state']=='Settled']
        require(len(winners)==1,'one settled order')
        if winners:
            w=winners[0];r=w['record'];require(r is not None,'settlement record exists')
            require(w['business']['evidence']['settlement_record_count']==1,'one target settlement record')
            require(w['business']['payment']['state']=='Released','escrow released')
            if r:
                o=w['business']['order'];require(r['order_id']==o['order_id'],'record order id binding')
                target=w['business']['meta']['owner'] if info['operation']=='TransferMeta' else w['business']['right']['holder']
                require(target==r['buyer'],'authoritative beneficiary matches record')
                s=w['business'];o=s['order'];identity=s['identity']
                original=next(x['order'] for x in [load(root/'canonical-order.json')] if x['orderId']==o['order_id'])
                settle_tx=[t for t in txs if t['dispatch_error'] is None][-1]
                expected={
                    'settlement_id':s['evidence']['settlement_id'],'order_id':o['order_id'],'operation':info['operation'],
                    'asset_id':s['meta']['asset_id'],'right_id':None if info['operation']=='TransferMeta' else s['right']['right_id'],
                    'authorization_id':s['authorization']['authorization_id'],'market_id':s['market']['market_id'],
                    'policy_hash':original['policyHash'],'order_commitment_hash':identity['order_commitment_hash'],
                    'seller':o['seller'],'buyer':o['buyer'],'currency':'Native','gross_price':original['price'],
                    'seller_amount':original['price'],'royalty_recipient':None,'royalty_amount':'0',
                    'market_fee_recipient':w['balances']['MARKET_OK']['account_hex'],'market_fee_amount':'0','safety_fee_amount':'0',
                    'pre_state_version':0 if info['operation']=='IssueRight' else original['sellerStateVersion'],
                    'post_state_version':s['meta']['state_version'] if info['operation']=='TransferMeta' else s['right']['state_version'],
                    'pre_state_commitment':s['evidence']['pre_state_commitment'],'block_number':settle_tx['block_number'],
                    'settlement_protocol_version':1,
                }
                require(set(r)==set(expected),'exact 24-field record schema')
                for key,value in expected.items():require(r.get(key)==value,'record field '+key)
                require(bool(re.fullmatch(r'0x[0-9a-f]{64}',r['pre_state_commitment'] or '')),'pre-state commitment encoding')
                require(s['meta']['owner_index_matches'],'owner index consistency')
                if info['operation']!='TransferMeta':require(s['right']['holder_index_matches'] and s['right']['parent_index_matches'],'right indices consistent')
        # Include contract deposits; do not drop balance changes wholesale.
        total_pre=sum(int(a['free'])+int(a['reserved']) for a in pre[0]['balances'].values())
        total_post=sum(int(a['free'])+int(a['reserved']) for a in post[0]['balances'].values())
        observed_addresses={a['address'] for a in pre[0]['balances'].values()}
        require(total_post-total_pre==sum(v for k,v in rewards.items() if k in observed_addresses)-sum(fees.values()),'aggregate native balance conservation including contract account')
    if variant=='Q1':
        for z in post:
            a=z['business']['authorization'];require(a['issued_count']+a['reserved_count']<=1,'last quota conservation')
    env=os.environ.copy();env['POLKADOT_DRIVER_ROOT']='/home/gz785/code/exp2/scripts/phase4/node'
    payload_run=subprocess.run(['/home/gz785/.local/node-v24.15.0-linux-x64/bin/node',str(Path(__file__).with_name('payload_check.js')),str(root)],env=env,capture_output=True,text=True,timeout=30)
    payload=json.loads(payload_run.stdout)
    require(payload_run.returncode==0 and payload['status']=='PASS','payload/ABI audit: '+str(payload.get('error','')))
    trace_reports=[]
    if variant in ['V3','V4','Q1']:
        prefixes=load(root/'storage-prefixes.json')
        for bh in {t['block_hash'] for t in txs if t['dispatch_error']}:
            trace_file=root/('trace-'+bh+'.json')
            if trace_file.exists():
                report=check_trace(load(trace_file),[t for t in txs if t['block_hash']==bh],prefixes,info['baseline'])
                trace_reports.append(report)
                errors.extend(report['errors'])
            else:errors.append('missing replay trace for rejected concurrent extrinsics')
    return {'status':'FAIL' if errors else 'PASS','errors':errors,'payload_audit':payload}
