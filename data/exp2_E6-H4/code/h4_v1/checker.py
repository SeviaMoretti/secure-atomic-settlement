"""Independent H4 business and monotonic timing validation."""
import json,sys
from pathlib import Path
from business_check import check as business_check
from online_check import check as online_check

def load(p):return json.loads(Path(p).read_text())
def check(root):
 root=Path(root);i=load(root/'execution.json');rows=load(root/'transactions.json');errors=[]
 def require(x,msg):
  if not x:errors.append(msg)
 b=business_check(root);errors.extend(b['errors']);online=online_check(i,rows);errors.extend(online['errors'])
 require([t['phase'] for t in rows]==['create','fund','settle'],'three ordered stages')
 require(i['execution_complete'],'execution incomplete')
 metrics=[]
 for t in rows:
  fields=['c0','encode_start','encode_end','nonce_query_start','nonce_query_end','sign_start','sign_end','c1','c_in','c_fin','evidence_read_start','c_done']
  ts=[int(t[k]) for k in fields];require(all(a<=b for a,b in zip(ts,ts[1:])),'nonmonotonic stage '+t['phase'])
  require(int(t['c_ack'])>=int(t['c1']),'ack before send')
  require(t['block_hash']==t['finalized_block_hash'],'finalized hash differs')
  statuses=t['statuses'];require(any(s['status']=='InBlock' and s['at_ns']==t['c_in'] for s in statuses),'missing first inclusion callback')
  require(any(s['status']=='Finalized' and s['at_ns']==t['c_fin'] for s in statuses),'missing finalization callback')
  require(not any(s['status'] in ['Retracted','Invalid','Dropped','Usurped','FinalityTimeout'] for s in statuses),'unexpected lifecycle status')
  h=t['head_at_start'];require(h is not None and int(h['observed_ns'])<=int(t['c0']),'missing initial block phase observation')
  def ms(a,z):return (int(t[z])-int(t[a]))/1e6
  metrics.append({'phase':t['phase'],'preparation_ms':ms('c0','c1'),'encoding_and_inner_signature_ms':ms('encode_start','encode_end'),'nonce_rpc_ms':ms('nonce_query_start','nonce_query_end'),'outer_signing_ms':ms('sign_start','sign_end'),'submission_to_inclusion_ms':ms('c1','c_in'),'inclusion_to_finalization_ms':ms('c_in','c_fin'),'evidence_after_inclusion_ms':ms('c_in','c_done'),'evidence_rpc_ms':ms('evidence_read_start','c_done'),'c0_to_inclusion_ms':ms('c0','c_in'),'c0_to_finalization_ms':ms('c0','c_fin'),'last_head_observation_age_ms':(int(t['c0'])-int(h['observed_ns']))/1e6,'block_number':t['block_number'],'completion_blocks':t['block_number']-h['number']})
 for a,z in zip(rows,rows[1:]):require(int(a['c_in'])<=int(z['c0']),'next stage before prior inclusion')
 order_in=(int(rows[-1]['c_in'])-int(rows[0]['c0']))/1e6;order_fin=(int(rows[-1]['c_fin'])-int(rows[0]['c0']))/1e6
 require(order_in>=sum(x['c0_to_inclusion_ms'] for x in metrics)-1e-5,'invalid wall-clock decomposition')
 require(int(i['timing']['fixture_ready'])<=int(rows[0]['c0']),'fixture within measurement')
 require(all(int(t['evidence_read_start'])>=int(rows[-1]['c_fin']) for t in rows),'heavy evidence read during order timing')
 pre=load(root/'pre.json');post=load(root/'post.json')
 for state in [pre,post]:
  for k in ['active_orders','funded_orders','active_exposure_value']:require(int(state['business']['market'].get(k,0))==0,'nonzero market '+k)
 return {'status':'PASS' if not errors else 'FAIL','errors':errors,'unit':i['episode_id'],'baseline':i['baseline'],'operation':i['operation'],'session_id':i['session_id'],'kind':i['kind'],'node_mode':i['node_mode'],'timing_metrics':metrics,'order_inclusion_ms':order_in,'order_finalization_ms':order_fin,'sum_phase_inclusion_ms':sum(x['c0_to_inclusion_ms'] for x in metrics),'sum_phase_finalization_ms':sum(x['c0_to_finalization_ms'] for x in metrics),'fixture_ms':(int(i['timing']['fixture_ready'])-int(i['timing']['preparation_start']))/1e6,'audit_after_measurement_ms':(int(i['timing']['audit_done'])-int(i['timing']['measurement_end']))/1e6,'payload_audit':b['payload_audit'],'online_execution':online,'instrumentation':i['instrumentation'],'calibration_pair_id':i.get('calibration_pair_id'),'online_calibration':'PAIRED' if i.get('calibration_pair_id') else 'NOT_THIS_RUN','paper_final':False}
if __name__=='__main__':
 p=Path(sys.argv[1])
 try:r=check(p)
 except Exception as e:r={'status':'ERROR','errors':[type(e).__name__+': '+str(e)]}
 with (p/'checker.json').open('x') as f:json.dump(r,f,indent=2)
 print(json.dumps(r));sys.exit(0 if r['status']=='PASS' else 1)
