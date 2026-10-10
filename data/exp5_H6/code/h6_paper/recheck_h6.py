"""Recheck archived H6 evidence without modifying raw files or starting a node."""
import collections
import hashlib
import json
from pathlib import Path
import sys
from datetime import datetime

ROOT = Path('/home/gz785/code/exp2')
RUN = ROOT/'results/raw/tdsc-h6-v1-formal-20260922T025046309066'
FREEZE = ROOT/'build/h6-v1-freeze-20260921-a'
OUT = ROOT/'results/processed/h6-paper-20260924'
MIRROR = Path('/mnt/c/Users/22004/Desktop/P2/TDSC/outputs/paper_h6_revision')
OUT.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, str(FREEZE/'source/scripts/h6_v1'))
from checker import check

def load(p):
    return json.loads(Path(p).read_text())

def sha(p):
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()

def save(name, data):
    txt = json.dumps(data, ensure_ascii=False, indent=2)+'\n'
    (OUT/name).write_text(txt)
    (MIRROR/name).write_text(txt, encoding='utf-8')

f = load(FREEZE/'freeze.json')
identity = []
for a in f['artifacts']:
    assert sha(a['path']) == a['sha256'], a['path']
    identity.append(a)
for a in f['sources']:
    p = FREEZE/'source'/Path(a['path']).relative_to(ROOT)
    assert sha(p) == a['sha256'], str(p)
manifest = load(RUN/'manifest.terminal.json')
assert manifest['status'] == 'COMPLETED_FORMAL'
assert len(manifest['units']) == 360
rows, units = [], []
counts = collections.Counter()
preparations = collections.Counter()
for n, u in enumerate(manifest['units'], 1):
    p = Path(u['path'])
    assert sha(p/'inventory.json') == u['inventory_sha256']
    for name, h in load(p/'inventory.json').items():
        assert sha(p/name) == h, str(p/name)
    result = check(p)
    assert result['status'] == 'PASS', (str(p), result['errors'])
    assert result['metrics'] == load(p/'checker.json')['metrics'], 'metric drift'
    e = load(p/'execution.json')
    counts[(e['operation'], e['terminal'], e['condition'])] += 1
    prep = e['preparation_signed_transactions']
    preparations['fixture_signed_transactions'] += prep
    preparations['all_signed_transactions'] += e['all_signed_transactions']
    txs = load(p/'transactions.json')
    for t in txs:
        preparations[t['phase']+'_signed_transactions'] += 1
    units.append({'episode':u['episode'], 'status':'PASS', 'inventory_sha256':u['inventory_sha256']})
    for metric in result['metrics']:
        phase = metric['phase']
        t = next(t for t in txs if t['phase']==phase)
        rb = load(p/('readback'+phase[-1]+'.json'))
        snap_a = load(p/('snapshot-before.json' if phase=='sync1' else 'snapshot-after1.json'))
        snap_z = load(p/('snapshot-after1.json' if phase=='sync1' else 'snapshot-after2.json'))
        assert rb['block_hash']==t['block_hash'] and rb['projection_return_scale']==snap_z['projection_return_scale']
        assert rb['read_done_ns']==snap_z['read_done_ns']
        row = {k:e[k] for k in ['operation','terminal','condition','repetition']}
        row.update(episode=u['episode'], phase=phase, status='PASS')
        for k,v in metric.items():
            if isinstance(v,(int,float,str)): row[k]=v
        row.update(post_dispatch_ref_time=metric['post_dispatch_weight']['refTime'], post_dispatch_proof_size=metric['post_dispatch_weight']['proofSize'])
        row['actual_fee']=int(row['actual_fee'])
        row['storage_deposit_delta']=sum(int(snap_z['contract_info'][k])-int(snap_a['contract_info'][k]) for k in ['storageByteDeposit','storageItemDeposit'])
        # These boundaries exclude initial node launch, and include fixture and lifecycle work.
        first_sync = next(x for x in txs if x['phase']=='sync1')
        row['pre_sync_setup_ms']=(int(first_sync['c0'])-int(e['started_ns']))/1e6
        row['fault_call_ms']=next(((int(x['c_fin'])-int(x['c1']))/1e6 for x in txs if x['phase']=='inject'),0)
        wait=p/'refund-wait.json'
        w=load(wait) if wait.exists() else None
        row['refund_wait_ms']=(int(w['end_ns'])-int(w['start_ns']))/1e6 if w else 0
        rows.append(row)
    if n%30==0: print(json.dumps({'rechecked':n,'planned':360}),flush=True)
cfg=load(FREEZE/'source/configs/h6_v1/config.json')
expected=collections.Counter({(o,t,c):10 for o in cfg['operations'] for t in cfg['terminals'] for c in cfg['conditions']})
assert counts==expected
assert preparations['sync1_signed_transactions']==preparations['sync2_signed_transactions']==360
assert preparations['all_signed_transactions']==sum(v for k,v in preparations.items() if k!='all_signed_transactions')
report={'checked_at':datetime.now().astimezone().isoformat(), 'status':'PASS','run_id':manifest['run_id'], 'validity_level':'paper-candidate', 'episodes':360,'transactions':720,'coverage':'36 cells x 10','freeze_sha256':sha(FREEZE/'freeze.json'),'manifest_sha256':sha(RUN/'manifest.terminal.json'),'artifacts':identity,'units':units,'preparation_counts':dict(preparations),'checker_source':'frozen checker rerun offline, including payload decoder; raw checksums and recorded metrics verified; no new chain runs','script_sha256':sha(__file__)}
save('h6-recheck.json',report)
save('h6-rows.json',rows)
print(json.dumps({'status':'PASS','rows':len(rows),'preparation_counts':dict(preparations)}),flush=True)
