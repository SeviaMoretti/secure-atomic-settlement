"""Conservative proof of no business-storage write calls in rejected extrinsics.

If writes are observed, this checker does NOT assume they were rolled back.
"""
import re
INDEX='3a65787472696e7369635f696e646578'
READS={'Get','ChildGet','Exists','ChildExists','NextKey','ChildNextKey','Read','ChildRead'}
WRITES={'Put','ChildPut','Delete','ChildDelete','Append','ChildAppend','ClearPrefix','ChildClearPrefix','KillChildStorage'}

def check_trace(trace,txs,prefixes,baseline):
    trace=trace['blockTrace'];errors=[];rows=[];by_index={};active=None
    for event in trace['events']:
        if event.get('target')!='state':continue
        values=event.get('data',{}).get('stringValues',{})
        method=values.get('method');key=values.get('key','').removeprefix('0x').lower()
        if key==INDEX and method=='Put':
            value=values.get('value')
            if value=='None':active=None
            else:
                m=re.fullmatch(r'Some\(([0-9a-fA-F]{8})\)',value or '')
                if not m:errors.append('unrecognized extrinsic index');active=None
                else:active=int.from_bytes(bytes.fromhex(m[1]),'little')
            continue
        if active is not None:by_index.setdefault(active,[]).append(values)
    for tx in txs:
        if not tx['dispatch_error']:continue
        if trace['blockHash'].removeprefix('0x')!=tx['block_hash'].removeprefix('0x') or trace['parentHash'].removeprefix('0x')!=tx['parent_hash'].removeprefix('0x'):
            errors.append('trace block identity differs');continue
        events=by_index.get(tx['extrinsic_index'],[]);business_reads=0;business_writes=[]
        for v in events:
            m=v.get('method');k=v.get('key','').removeprefix('0x').lower()
            if m not in READS|WRITES:errors.append('unknown state trace method '+str(m));continue
            pallet=prefixes.get(k[:32],'')
            relevant=(m.startswith('Child') or v.get('child_id') or v.get('childId')) if baseline=='B1' else (pallet in {'MarketRuntime','DataAssets','Markets'} or m.startswith('Child'))
            if relevant and m in READS:business_reads+=1
            if relevant and m in WRITES:business_writes.append(v)
        if not events or business_reads==0:errors.append('missing business read trace for '+tx['tx_hash'])
        if business_writes:errors.append('business write calls observed in rejected extrinsic '+tx['tx_hash'])
        rows.append({'tx_hash':tx['tx_hash'],'extrinsic_index':tx['extrinsic_index'],'business_reads':business_reads,'business_write_calls':len(business_writes)})
    return {'status':'FAIL' if errors else 'PASS','errors':errors,'rows':rows,'scope':'no-business-write-calls-in-block-replay'}
