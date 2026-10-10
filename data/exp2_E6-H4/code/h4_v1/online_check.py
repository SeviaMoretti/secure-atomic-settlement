"""Match live block-construction Runtime calls to exact included transactions."""
import json
from pathlib import Path

def check(info,rows):
 p=Path(info['node_log']);spans=[]
 for number,line in enumerate(p.read_text().splitlines(),1):
  if line.startswith('H4_SPAN '):
   j=json.loads(line[len('H4_SPAN '):]);j['log_line']=number;spans.append(j)
 errors=[];matches=[];enabled=info['instrumentation']=='on'
 if not enabled and spans:errors.append('instrumentation-off node emitted timing spans')
 for tx in rows:
  candidates=[s for s in spans if s['method']=='BlockBuilder_apply_extrinsic' and s['payload_hash']==tx['tx_hash']]
  if enabled:
   exact=[s for s in candidates if s['block_number']==tx['block_number'] and s['parent_hash']==tx['parent_hash'] and s['extrinsic_index']==tx['extrinsic_index']]
   if len(exact)!=1:errors.append('expected one exact live execution span: '+tx['phase'])
   for s in exact:
    if s['context']!='Onchain' or not s['host_call_ok'] or s['native']:errors.append('unexpected online execution context')
    if s['end_ns']-s['start_ns']!=s['duration_ns'] or s['duration_ns']<=0:errors.append('invalid online monotonic interval')
   matches.append({'phase':tx['phase'],'tx_hash':tx['tx_hash'],'block_hash':tx['block_hash'],'extrinsic_index':tx['extrinsic_index'],'matched_spans':exact,'all_same_payload_attempts':candidates,'runtime_call_ms':exact[0]['duration_ns']/1e6 if len(exact)==1 else None})
 return {'status':'FAIL' if errors else 'PASS','errors':errors,'instrumentation':info['instrumentation'],'log_path':str(p),'matches':matches,'boundary':'live block construction WasmExecutor::call(BlockBuilder_apply_extrinsic), including host/runtime invocation overhead; not replay, not pure Wasm instructions','clock':'node-local monotonic; never subtracted from client clock'}
