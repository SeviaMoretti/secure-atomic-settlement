"""Deterministic H4 summaries, session-cluster bootstrap and paired calibration."""
import json,sys,random,statistics,hashlib,csv
from pathlib import Path
SEED=2026091904;BOOTSTRAPS=50000

def quantile(a,q):
 a=sorted(a);p=(len(a)-1)*q;lo=int(p);hi=min(lo+1,len(a)-1);return a[lo]+(p-lo)*(a[hi]-a[lo])
def stats(a):return {'n':len(a),'mean':statistics.mean(a),'median':statistics.median(a),'p90':quantile(a,.9),'min':min(a),'max':max(a)}
def rng(key):return random.Random(SEED+int(hashlib.sha256(str(key).encode()).hexdigest()[:12],16))
def cluster_summary(groups,key):
 values=[v for g in groups for v in g];result=stats(values);result['sessions']=len(groups)
 r=rng(key);boot=[]
 for _ in range(BOOTSTRAPS):
  sampled=[v for g in r.choices(groups,k=len(groups)) for v in g];boot.append(statistics.mean(sampled))
 result['mean_ci95_session_bootstrap']=[quantile(boot,.025),quantile(boot,.975)];result['bootstrap_repetitions']=BOOTSTRAPS;return result

def calibration_summary(rows):
 pairs={};online=[]
 for p,i,c in rows:
  if i['kind']!='sample':continue
  pairs.setdefault((i['baseline'],i['operation'],i['repetition']),{})[i['instrumentation']]=c
  if i['instrumentation']=='on':
   for match in c['online_execution']['matches']:online.append({'baseline':i['baseline'],'operation':i['operation'],'round':i['repetition'],'phase':match['phase'],'runtime_call_ms':match['runtime_call_ms'],'tx_hash':match['tx_hash'],'block_hash':match['block_hash'],'extrinsic_index':match['extrinsic_index']})
 table=[]
 for b in ['B1','B2','G3']:
  for op in ['TransferMeta','IssueRight','TransferRight']:
   ps=[v for (bb,oo,rr),v in pairs.items() if bb==b and oo==op]
   for metric in ['order_inclusion_ms','order_finalization_ms']:
    off=[x['off'][metric] for x in ps];on=[x['on'][metric] for x in ps];diff=[a-z for a,z in zip(on,off)];rel=[100*(a/z-1) for a,z in zip(on,off)];r=rng((b,op,metric));boot=[statistics.median(r.choices(diff,k=len(diff))) for _ in range(BOOTSTRAPS)]
    table.append({'baseline':b,'operation':op,'metric':metric,'pairs':len(ps),'off':stats(off),'on':stats(on),'paired_difference_ms':stats(diff),'paired_relative_percent':stats(rel),'paired_median_difference_ci95':[quantile(boot,.025),quantile(boot,.975)],'material_shift_flag':abs(statistics.median(rel))>5})
 return {'schema':'h4-calibration-v1','status':'PASS','pairs':len(pairs),'samples':sum(len(v) for v in pairs.values()),'bootstrap_repetitions':BOOTSTRAPS,'seed':SEED,'comparisons':table,'online_execution_samples':online,'interpretation':'paired n=5 per cell estimates observable instrumentation perturbation; no claim of zero overhead; main latency uses instrumentation off; online Runtime duration is observed under instrumentation and is not a pure instruction time','clock_policy':'no node/client clock subtraction; no percentage stacking of different clock domains'}

def main(run):
 r=Path(run);m=json.loads((r/'manifest.terminal.json').read_text());assert m['status']=='COMPLETED_FORMAL' and m['mode']=='formal'
 rows=[]
 for u in m['units']:
  if u['kind']!='sample':continue
  p=Path(u['path']);c=json.loads((p/'checker.json').read_text());assert c['status']=='PASS';rows.append(c)
 assert len(rows)==270
 groups={}
 for c in rows:
  for metric in ['order_inclusion_ms','order_finalization_ms','sum_phase_inclusion_ms','fixture_ms','audit_after_measurement_ms']:
   groups.setdefault((c['baseline'],c['operation'],'order',metric),{}).setdefault(c['session_id'],[]).append(c[metric])
  for phase in c['timing_metrics']:
   for metric in ['preparation_ms','submission_to_inclusion_ms','inclusion_to_finalization_ms','evidence_rpc_ms']:
    groups.setdefault((c['baseline'],c['operation'],phase['phase'],metric),{}).setdefault(c['session_id'],[]).append(phase[metric])
 summary=[]
 for key,sessions in sorted(groups.items()):
  assert len(sessions)==10 and all(len(x)==3 for x in sessions.values());v=cluster_summary(list(sessions.values()),key);summary.append({'baseline':key[0],'operation':key[1],'phase':key[2],'metric':key[3],**v})
 out=Path('/home/gz785/code/exp2/results/processed')/m['run_id'];out.mkdir(exist_ok=False)
 (out/'summary.json').write_text(json.dumps({'run_id':m['run_id'],'samples':270,'warmups_excluded':90,'sessions':30,'seed':SEED,'statistics':summary,'validity_level':'paper-candidate','node_execution_source':'separate on/off calibration report'},indent=2))
 with (out/'orders.csv').open('w') as f:
  keys=['unit','baseline','operation','session_id','order_inclusion_ms','order_finalization_ms','sum_phase_inclusion_ms','fixture_ms','audit_after_measurement_ms'];w=csv.DictWriter(f,fieldnames=keys);w.writeheader();w.writerows({k:c[k] for k in keys} for c in rows)
 print(out)
if __name__=='__main__':main(sys.argv[1])
