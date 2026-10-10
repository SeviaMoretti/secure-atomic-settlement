"""Deterministic cell summaries and figure, with episode-level resampling."""
from pathlib import Path
import csv
import hashlib
import json
import sys
import collections
sys.path.insert(0,str(Path(__file__).parent/'deps'))
import numpy as np
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt

HERE=Path(__file__).parent
rows=json.loads((HERE/'h6-rows.json').read_text(encoding='utf-8'))
report=json.loads((HERE/'h6-recheck.json').read_text(encoding='utf-8'))
assert report['status']=='PASS' and len(rows)==720
OPS=['TransferMeta','IssueRight','TransferRight']
TERMS=['Settled','Cancelled','Refunded']
CONDITIONS=['F0','F1','F2','F3']
METRICS=['submission_to_inclusion_ms','submission_to_finalization_ms','finalized_readback_ms','recovery_visible_ms','actual_fee','post_dispatch_ref_time','post_dispatch_proof_size','contract_storage_items_delta','contract_storage_bytes_delta','storage_deposit_delta','pre_sync_setup_ms','fault_call_ms','refund_wait_ms']
groups=collections.defaultdict(list)
for row in rows: groups[(row['operation'],row['terminal'],row['condition'],row['phase'])].append(row)
stats=[]
for key,rr in sorted(groups.items()):
    assert len(rr)==10 and {x['repetition'] for x in rr}==set(range(1,11))
    rr=sorted(rr,key=lambda r:r['repetition'])
    # Reuse indices across metrics and sync1/sync2 to preserve episode pairing.
    seed=2026091906+int(hashlib.sha256('|'.join(key[:3]).encode()).hexdigest()[:8],16)
    indices=np.random.default_rng(seed).integers(0,10,size=(50000,10))
    for metric in METRICS:
        values=np.array([r[metric] for r in rr],dtype=float)
        medians=np.median(values[indices],axis=1)
        ci=np.quantile(medians,[.025,.975])
        s=dict(zip(['operation','terminal','condition','phase'],key))
        s.update(metric=metric,n=10,mean=float(values.mean()),median=float(np.median(values)),p10=float(np.quantile(values,.1)),p90=float(np.quantile(values,.9)),min=float(values.min()),max=float(values.max()),median_ci95_low=float(ci[0]),median_ci95_high=float(ci[1]))
        stats.append(s)
pooled=[]
for cond in CONDITIONS:
    for phase in ['sync1','sync2']:
        rr=[r for r in rows if r['condition']==cond and r['phase']==phase]
        s={'condition':cond,'phase':phase,'n':len(rr),'status':'PASS'}
        for metric in METRICS:
            values=np.array([r[metric] for r in rr],dtype=float)
            s[metric]={'median':float(np.median(values)),'mean':float(np.mean(values)),'min':float(min(values)),'max':float(max(values))}
        pooled.append(s)
summary={'run_id':report['run_id'],'validity_level':'paper-candidate','episodes':360,'sync_transactions':720,'statistics_unit':'episode, 10 per operation-terminal-condition cell; sync1/sync2 kept separate','bootstrap_repetitions':50000,'bootstrap':'percentile 95% interval for within-cell median; same derived seed and episode resamples across phases','pooled_interpretation':'descriptive only; do not pool heterogeneous cells for reliability confidence or cross-method inference','statistics':stats,'condition_descriptive':pooled,'preparation_counts':report['preparation_counts'],'script_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
(HERE/'h6-summary.json').write_text(json.dumps(summary,ensure_ascii=False,indent=2)+'\n',encoding='utf-8')
for name,rs in [('h6-statistics.csv',stats),('h6-episodes.csv',rows)]:
    with (HERE/name).open('w',newline='',encoding='utf-8-sig') as f:
        w=csv.DictWriter(f,fieldnames=list(rs[0]));w.writeheader();w.writerows(rs)

plt.rcParams.update({'font.family':'DejaVu Sans','font.size':9,'axes.spines.top':False,'axes.spines.right':False,'svg.fonttype':'none'})
fig,axes=plt.subplots(3,1,figsize=(7.1,6.4),sharex=True,sharey=True,layout='constrained')
lookup={(s['operation'],s['terminal'],s['condition'],s['phase'],s['metric']):s for s in stats}
for ax,terminal in zip(axes,TERMS):
    for phase,offset,color,marker,label in [('sync1',-.13,'#127C80','o','First sync'),('sync2',.13,'#B35B34','s','Second sync')]:
        ss=[lookup[(o,terminal,c,phase,'recovery_visible_ms')] for o in OPS for c in CONDITIONS]
        med=np.array([s['median']/1000 for s in ss]);lo=np.array([s['median_ci95_low']/1000 for s in ss]);hi=np.array([s['median_ci95_high']/1000 for s in ss])
        ax.errorbar(np.arange(12)+offset,med,yerr=[med-lo,hi-med],fmt=marker,color=color,ms=3.5,elinewidth=1,capsize=2,label=label)
    ax.set_title(terminal,loc='left',fontsize=10,fontweight='bold')
    ax.set_ylabel('Visible time (s)')
    ax.set_ylim(12,20);ax.set_yticks([12,14,16,18,20]);ax.grid(axis='y',alpha=.22)
    for x in [3.5,7.5]:ax.axvline(x,color='#CCCCCC',linewidth=.8)
    ax.set_xlim(-.7,11.7)
axes[0].legend(ncol=2,loc='lower right',frameon=False,fontsize=8)
axes[-1].set_xticks(range(12),CONDITIONS*3)
for x,o in zip([1.5,5.5,9.5],OPS):axes[-1].text(x,-.22,o,ha='center',va='top',transform=axes[-1].get_xaxis_transform(),fontsize=9)
fig.savefig(HERE/'h6-visible-time.png',dpi=250)
fig.savefig(HERE/'h6-visible-time.pdf')
plt.close(fig)
print(json.dumps({'cells':len(groups)//2,'statistics':len(stats),'condition_descriptive':pooled,'preparation_counts':report['preparation_counts']},ensure_ascii=False))
