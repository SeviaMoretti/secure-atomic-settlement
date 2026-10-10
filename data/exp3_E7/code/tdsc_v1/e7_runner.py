#!/usr/bin/env python3
from __future__ import annotations
import argparse,datetime,hashlib,json,os,signal,socket,subprocess,time
from pathlib import Path

ROOT=Path('/home/gz785/code/exp2')
NODE=Path('/home/gz785/.local/node-v24.15.0-linux-x64/bin/node')
BASELINES={
 'B1': {'worktree':'/home/gz785/code/assetxchain-b1','commit':'8514bf0720ec9b5ed3c8d3ec1501264ae7bf1642','node':'/home/gz785/code/exp2/build/r3-x1-formal-artifacts-20260808-v1/b1-control/node/assetxchain-node','artifact_root':'/home/gz785/code/exp2/build/r3-x1-formal-artifacts-20260808-v1/b1-control/ink'},
 'B2': {'worktree':'/home/gz785/code/assetxchain-b2','commit':'dc10ea68ffddacf93e6d57d91c9acfa35a4b1bfa','node':'/home/gz785/code/exp2/build/r3-x1-formal-artifacts-20260808-v1/b2-control/node/assetxchain-node'},
 'G3': {'worktree':'/home/gz785/code/assetxchain-g3-v2','commit':'47ebd794c4ef1264bb8265c49d8e92c5b44e6323','node':'/home/gz785/code/exp2/build/r3-x1-formal-artifacts-20260808-v1/g3-control/node/assetxchain-node','artifact_root':'/home/gz785/code/exp2/build/r3-x1-formal-artifacts-20260808-v1/g3-control/ink/market_orderbook'},
}
OPS=('TransferMeta','IssueRight','TransferRight')

def now():return datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=8))).isoformat()
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def port_open(port):
 with socket.socket() as s:
  s.settimeout(.15);return s.connect_ex(('127.0.0.1',port))==0
def wait_rpc(proc,timeout=90):
 end=time.time()+timeout
 while time.time()<end:
  if proc.poll() is not None:raise RuntimeError(f'node exited {proc.returncode}')
  if port_open(9944):return
  time.sleep(.25)
 raise RuntimeError('RPC readiness timeout')
def stop(proc):
 if proc.poll() is None:
  proc.send_signal(signal.SIGINT)
  try:proc.wait(20)
  except subprocess.TimeoutExpired:
   proc.terminate()
   try:proc.wait(10)
   except subprocess.TimeoutExpired:proc.kill();proc.wait(5)
 end=time.time()+15
 while time.time()<end and any(port_open(p) for p in (9944,30333,9615)):time.sleep(.25)
 if any(port_open(p) for p in (9944,30333,9615)):raise RuntimeError('owned node ports did not close')

def main():
 ap=argparse.ArgumentParser()
 ap.add_argument('--mode',choices=['smoke','pilot','formal'],required=True)
 ap.add_argument('--baselines',nargs='+',choices=BASELINES,default=list(BASELINES))
 ap.add_argument('--variants',nargs='+',choices=['V0','V1','V2','V3','V4','Q1'],default=['V0','V1','V2','V3','V4','Q1'])
 ap.add_argument('--operations',nargs='+',choices=OPS,default=list(OPS))
 a=ap.parse_args()
 config=ROOT/'configs/tdsc_v1/e7-v1.json'
 config_data=json.loads(config.read_text())
 reps={'smoke':1,'pilot':config_data['pilot_repetitions'],'formal':config_data['formal_repetitions']}[a.mode]
 for b in a.baselines:
  spec=BASELINES[b]; worktree=spec['worktree']; commit=spec['commit']
  actual=subprocess.check_output(['git','-C',worktree,'rev-parse','HEAD'],text=True).strip()
  dirty=subprocess.check_output(['git','-C',worktree,'status','--porcelain'],text=True).strip()
  if actual!=commit or dirty:raise RuntimeError(f'{b} source identity/cleanliness failed')
 units=[]
 for b in a.baselines:
  for op in a.operations:
   for v in a.variants:
    if v=='Q1' and op!='IssueRight':continue
    for rep in range(1,reps+1):units.append((b,op,v,rep))
 stamp=datetime.datetime.now(datetime.timezone(datetime.timedelta(hours=8))).strftime('%Y%m%dT%H%M%S+0800')
 batch=f'tdsc-e7-{a.mode}-{stamp}-{sha(config)[:8]}'
 raw=ROOT/'results/raw'/batch;logs=ROOT/'logs'/batch
 raw.mkdir(parents=True);logs.mkdir(parents=True)
 manifest={'schema_id':'assetxchain-tdsc-e7-batch','schema_version':1,'batch_id':batch,'mode':a.mode,'created_at':now(),'config':str(config),'config_sha256':sha(config),'repetitions':reps,'selection':{'baselines':a.baselines,'operations':a.operations,'variants':a.variants},'planned_units':len(units),'units':[],'status':'RUNNING'}
 (raw/'manifest.preflight.json').write_text(json.dumps(manifest,indent=2)+'\n')
 try:
  for seq,(b,op,v,rep) in enumerate(units,1):
   spec=BASELINES[b]; worktree=spec['worktree']; node=Path(spec['node'])
   unit_id=f'{seq:04d}-{b.lower()}-{op.lower()}-{v.lower()}-r{rep:02d}'
   out=raw/'units'/unit_id;out.mkdir(parents=True)
   node_log=(logs/f'{unit_id}.node.log').open('xb');exec_log=logs/f'{unit_id}.executor.log'
   proc=subprocess.Popen([str(node),'--dev','--tmp','--rpc-cors','all','--rpc-methods','unsafe'],cwd=worktree,stdout=node_log,stderr=subprocess.STDOUT,start_new_session=True)
   started=now()
   try:
    wait_rpc(proc)
    cmd=[str(NODE),str(ROOT/'scripts/tdsc_v1/e7_unit_executor.js'),'--baseline',b,'--operation',op,'--variant',v,'--repetition',str(rep),'--output-root',str(out),'--rpc','ws://127.0.0.1:9944','--run-id',batch]
    if spec.get('artifact_root'): cmd += ['--artifact-root', spec['artifact_root']]
    env=os.environ.copy();env['POLKADOT_DRIVER_ROOT']=str(ROOT/'scripts/phase4/node')
    completed=subprocess.run(cmd,cwd=worktree,env=env,text=True,capture_output=True)
    exec_log.write_text('stdout:\n'+completed.stdout+'\nstderr:\n'+completed.stderr)
    if completed.returncode:raise RuntimeError(f'{unit_id} executor failed: {completed.stderr[-1000:]}')
    unit=json.loads((out/'e7-unit.json').read_text())
    manifest['units'].append({'sequence':seq,'unit_id':unit_id,'baseline':b,'operation':op,'variant':v,'repetition':rep,'started_at':started,'finished_at':now(),'status':'PASS','result':str(out/'e7-unit.json'),'result_sha256':sha(out/'e7-unit.json')})
   finally:
    stop(proc);node_log.close()
   (raw/'manifest.progress.json').write_text(json.dumps(manifest,indent=2)+'\n')
   print(json.dumps({'progress':f'{seq}/{len(units)}','unit':unit_id,'status':'PASS'}),flush=True)
  manifest['status']='COMPLETED';manifest['finished_at']=now()
 except BaseException as exc:
  manifest['status']='FAILED';manifest['finished_at']=now()
  manifest['error']={'type':type(exc).__name__,'message':str(exc)}
  raise
 finally:
  (raw/'manifest.completed.json').write_text(json.dumps(manifest,indent=2)+'\n')
 print(json.dumps({'status':manifest['status'],'batch_id':batch,'units':len(manifest['units']),'raw':str(raw)}))
if __name__=='__main__':main()
