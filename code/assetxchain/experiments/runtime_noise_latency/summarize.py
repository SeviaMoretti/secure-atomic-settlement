import json, statistics, math
from pathlib import Path
root=Path('/home/gz785/code/assetxchain/experiments/runtime_noise_latency/runtime-noise-formal-20260923')
rows=[]
for p in sorted(root.glob('*.json')):
    if '.node.' in p.name or p.name == 'summary.json':
        continue
    data=json.loads(p.read_text())
    vals=[x['target_latency_ms'] for x in data['rows']]
    rows.append({'variant':data['variant'],'noise_count':data['noise_count'],'repetitions':data['repetitions'],'values':vals,'mean_ms':statistics.fmean(vals),'median_ms':statistics.median(vals),'p95_ms':sorted(vals)[min(len(vals)-1,math.ceil(.95*len(vals))-1)],'completion_blocks':[x['target_completion_blocks'] for x in data['rows']],'noise_success':[x['noise_success_count'] for x in data['rows']]})
rows.sort(key=lambda x:(x['variant'],x['noise_count']))
for r in rows:
    print(r)
out={'schema_version':'assetxchain-runtime-noise-summary-v1','run_dir':str(root),'rows':rows,'claim_boundary':'In-block observation latency on fresh local dev chains under concurrent registerAssetCore background load; not execution-only, RPC-only, or finality latency.'}
(root/'summary.json').write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n')
