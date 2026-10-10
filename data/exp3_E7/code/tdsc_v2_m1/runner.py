"""Session-based E7 test runner; formal entry requires an independent gate."""
import argparse,datetime,json,subprocess,socket,time,signal,hashlib,os
from pathlib import Path
ROOT=Path('/home/gz785/code/exp2'); HERE=ROOT/'scripts/tdsc_v2'
NODE='/home/gz785/.local/node-v24.15.0-linux-x64/bin/node'
BASE={
 'B1':{'node':'b1-control','artifact':'/home/gz785/code/assetxchain-e7v2-b1/contracts/market_pure_contract/target/ink'},
 'B2':{'node':'b2-control','artifact':None},
 'G3':{'node':'g3-control','artifact':'/home/gz785/code/assetxchain-e7v2-g3/target/ink/market_orderbook'},
}
def now():return datetime.datetime.now().astimezone().isoformat()
def write(p,j):
 t=p.with_suffix('.new');t.write_text(json.dumps(j,indent=2)+'\n');t.replace(p)
def open_port():
 with socket.socket() as s:s.settimeout(.3);return s.connect_ex(('127.0.0.1',9944))==0
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--baselines',nargs='+',default=list(BASE));ap.add_argument('--variants',nargs='+',default=['V0','V1','V2','V3','V4','Q1']);ap.add_argument('--operations',nargs='+',default=['TransferMeta','IssueRight','TransferRight']);ap.add_argument('--mode',choices=['test','formal'],default='test');ap.add_argument('--node-mode',choices=['reused','fresh'],default='reused');a=ap.parse_args()
 if a.mode=='formal':
  gate=ROOT/'results/processed/tdsc-e7-v2-gate.json'
  if not gate.exists() or not json.loads(gate.read_text()).get('formal_ready'):raise RuntimeError('formal gate has not passed')
  import sys
  sys.path.insert(0,str(ROOT/'scripts/tdsc_v2_support'))
  from gate import identity,sha
  gate_data=json.loads(gate.read_text()); frozen_path=Path(gate_data['freeze']['path'])
  if sha(frozen_path)!=gate_data['freeze']['sha256']:raise RuntimeError('freeze manifest drift')
  identity(json.loads(frozen_path.read_text()))
 if open_port():raise RuntimeError('RPC 9944 occupied; not attaching to an unidentified node')
 run='tdsc-e7-v2-'+a.mode+'-'+datetime.datetime.now().strftime('%Y%m%dT%H%M%S')
 out=ROOT/'results/raw'/run;logs=ROOT/'logs'/run;out.mkdir();logs.mkdir()
 spec=[(b,r,o,v) for r in range(1,6 if a.mode=='formal' else 2) for b in a.baselines for o in a.operations for v in a.variants if v!='Q1' or o=='IssueRight']
 m={'run_id':run,'status':'RUNNING','created_at':now(),'planned_units':len(spec),'node_mode':a.node_mode,'mode':a.mode,'pid':os.getpid(),'units':[], 'script_sha256':{p.name:hashlib.sha256(p.read_bytes()).hexdigest() for p in HERE.iterdir() if p.is_file()}}
 if a.mode=='formal':
  m['formal_gate']=str(gate);m['formal_gate_sha256']=sha(gate);m['validity_level']='paper-candidate'
  write(out/'formal-gate.json',gate_data);write(out/'frozen-identity.json',json.loads(frozen_path.read_text()))
 write(out/'manifest.preflight.json',m);write(out/'manifest.progress.json',m)
 proc=None;node_log=None;session=None
 def stop():
  nonlocal proc,node_log
  if proc is not None and proc.poll() is None:
   proc.send_signal(signal.SIGINT)
   try:proc.wait(20)
   except subprocess.TimeoutExpired:proc.terminate();proc.wait(10)
  if node_log:node_log.close()
  proc=None;node_log=None
 try:
  for i,(b,r,o,v) in enumerate(spec,1):
   sid=f'{b}-r{r}' if a.node_mode=='reused' else f'{b}-r{r}-e{i}'
   if sid!=session:
    stop();session=sid;node_log=(logs/(sid+'.node.log')).open('xb')
    binary=Path('/home/gz785/code/assetxchain-g3-v2/target/release/assetxchain-node')
    proc=subprocess.Popen([str(binary),'--dev','--tmp','--rpc-cors','all','--rpc-methods','unsafe'],stdout=node_log,stderr=subprocess.STDOUT,start_new_session=True)
    until=time.monotonic()+90
    while not open_port():
     if proc.poll() is not None or time.monotonic()>until:raise RuntimeError('node readiness failed')
     time.sleep(.25)
   episode=f'{i:04d}-{b}-{o}-{v}-r{r}';unit=out/'units'/episode;unit.mkdir(parents=True)
   inp={'baseline':b,'operation':o,'variant':v,'repetition':r,'run_id':run,'episode_id':run+'/'+episode,'session_id':sid,'output_root':str(unit),'rpc':'ws://127.0.0.1:9944','artifact_root':BASE[b]['artifact']}
   write(unit/'input.json',inp)
   m['current_episode']=episode;m['heartbeat_at']=now();write(out/'manifest.progress.json',m)
   with (logs/(episode+'.executor.log')).open('xb') as h:
    env=os.environ.copy();env['POLKADOT_DRIVER_ROOT']=str(ROOT/'scripts/phase4/node')
    child=subprocess.Popen([NODE,str(HERE/'unit.js'),str(unit/'input.json')],env=env,stdout=h,stderr=subprocess.STDOUT)
    deadline=time.monotonic()+600
    while child.poll() is None:
     if proc.poll() is not None:child.terminate();raise RuntimeError('owned node exited')
     if time.monotonic()>deadline:child.terminate();child.wait(10);raise RuntimeError('episode timeout')
     m['heartbeat_at']=now();m['executor_pid']=child.pid;write(out/'manifest.progress.json',m);time.sleep(1)
    if child.returncode:raise RuntimeError(episode+' executor failed; see '+str(h.name))
   result=subprocess.run(['python3',str(HERE/'checker.py'),str(unit)],capture_output=True,text=True)
   report=json.loads((unit/'checker.json').read_text())
   m['units'].append({'episode':episode,'net_effect_status':report['net_effect_status'],'formal_ready':report['formal_ready'],'path':str(unit)})
   m['heartbeat_at']=now();write(out/'manifest.progress.json',m)
   print(json.dumps({'completed':len(m['units']),'planned':len(spec),'episode':episode,'checker':report}),flush=True)
   if result.returncode:raise RuntimeError('independent checker failed: '+str(report['errors']))
  m['status']='COMPLETED_FORMAL' if a.mode=='formal' else 'COMPLETED_TEST';m['formal_ready']=False
  m['execution_complete']=True;m['paper_final']=False
 except BaseException as e:
  m['status']='FAILED';m['error']={'type':type(e).__name__,'message':str(e)};raise
 finally:
  stop();m['finished_at']=now();write(out/'manifest.terminal.json',m);write(out/'manifest.progress.json',m)
if __name__=='__main__':main()
