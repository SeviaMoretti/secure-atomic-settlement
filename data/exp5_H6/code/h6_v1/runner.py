"""H6 runner. Pilot collects a complete round of failures before stopping."""
import argparse,datetime,json,subprocess,socket,time,signal,hashlib,os,random
from pathlib import Path
ROOT=Path('/home/gz785/code/exp2');HERE=ROOT/'scripts/h6_v1';CONFIG=ROOT/'configs/h6_v1/config.json'
NODE='/home/gz785/.local/node-v24.15.0-linux-x64/bin/node'
CONTROL=ROOT/'reports/r3/x1-e0-v1/g3-control.json'
def now():return datetime.datetime.now().astimezone().isoformat()
def load(p):return json.loads(Path(p).read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def write(p,j):
 p=Path(p);p.parent.mkdir(parents=True,exist_ok=True);t=p.with_suffix('.new');t.write_text(json.dumps(j,indent=2)+'\n');t.replace(p)
def port():
 with socket.socket() as s:s.settimeout(.2);return s.connect_ex(('127.0.0.1',9944))==0
def stop(proc):
 if proc and proc.poll() is None:
  proc.send_signal(signal.SIGINT)
  try:proc.wait(30)
  except subprocess.TimeoutExpired:proc.terminate();proc.wait(15)
def episode(out,logs,m,cfg,control,name,op,term,cond,r):
 proc=child=node_log=None;unit=out/'units'/name;unit.mkdir(parents=True)
 result={'episode':name,'operation':op,'terminal':term,'condition':cond,'repetition':r,'status':'FAIL','path':str(unit)}
 try:
  if port():raise RuntimeError('RPC port occupied before independent fresh episode')
  binary=control['artifacts']['node']['path'];assert sha(binary)==control['artifacts']['node']['sha256'],'node artifact drift'
  node_log=(logs/(name+'.node.log')).open('xb');cmd=[binary,'--dev','--tmp','--rpc-cors','all','--rpc-methods','unsafe']
  proc=subprocess.Popen(cmd,stdout=node_log,stderr=subprocess.STDOUT,start_new_session=True);deadline=time.monotonic()+90
  while not port():
   if proc.poll() is not None or time.monotonic()>deadline:raise RuntimeError('node startup failed')
   time.sleep(.25)
  artifact=str(ROOT/'build/h6-v1-contracts/fault') if cond=='F3' else str(Path(control['artifacts']['contract_wasm']['path']).parent)
  inp={'baseline':'G3','operation':op,'terminal':term,'condition':cond,'repetition':r,'run_id':m['run_id'],'episode_id':m['run_id']+'/'+name,'output_root':str(unit),'rpc':cfg['rpc'],'artifact_root':artifact,'artifact_mode':'fault' if cond=='F3' else 'normal','node_pid':proc.pid,'node_command':cmd,'normal_commit':control['implementation_commit']}
  write(unit/'input.json',inp);m.update(current_episode=name,node_pid=proc.pid,heartbeat_at=now());write(out/'manifest.progress.json',m)
  env=os.environ.copy();env['POLKADOT_DRIVER_ROOT']=str(ROOT/'scripts/phase4/node')
  with (logs/(name+'.executor.log')).open('xb') as log:
   child=subprocess.Popen([NODE,str(HERE/'unit.js'),str(unit/'input.json')],env=env,stdout=log,stderr=subprocess.STDOUT);deadline=time.monotonic()+cfg['episode_timeout_seconds']
   while child.poll() is None:
    if proc.poll() is not None:raise RuntimeError('node exited')
    if time.monotonic()>deadline:raise RuntimeError('episode timeout')
    m.update(heartbeat_at=now(),executor_pid=child.pid);write(out/'manifest.progress.json',m);time.sleep(2)
   if child.returncode:raise RuntimeError('executor failed; '+str(log.name))
  from checker import check
  report=check(unit);write(unit/'checker.json',report);result.update(status=report['status'],errors=report['errors'])
 except Exception as ex:
  result['errors']=[str(ex)];write(unit/'failure.json',{'error':str(ex),'recorded_at':now()})
 finally:
  stop(child);stop(proc)
  if node_log:node_log.close()
  inventory={p.name:sha(p) for p in unit.iterdir() if p.is_file()};write(unit/'inventory.json',inventory);result['inventory_sha256']=sha(unit/'inventory.json')
 return result
def run(mode,freeze=None,operations=None,terminals=None,conditions=None):
 import fcntl
 lock=(ROOT/'h6-v1.lock').open('a');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
 cfg=load(CONFIG);control=load(CONTROL)
 if mode!='diagnostic':assert freeze and not any([operations,terminals,conditions]),'pilot/formal must use complete frozen matrix'
 if freeze:
  from gate import identity
  identity(load(freeze))
 if mode=='formal':
  from gate import validate_formal
  validate_formal(freeze)
 if port():raise RuntimeError('RPC port occupied; will not attach to unidentified node')
 rid='tdsc-h6-v1-'+mode+'-'+datetime.datetime.now().strftime('%Y%m%dT%H%M%S%f');out=ROOT/'results/raw'/rid;out.mkdir();logs=ROOT/'logs'/rid;logs.mkdir()
 reps=cfg['formal_repetitions'] if mode=='formal' else cfg['pilot_repetitions'] if mode=='pilot' else 1
 rng=random.Random(cfg['seed']);spec=[]
 for r in range(1,reps+1):
  cells=[(o,t,c,r) for o in operations or cfg['operations'] for t in terminals or cfg['terminals'] for c in conditions or cfg['conditions']];rng.shuffle(cells);spec+=cells
 m={'run_id':rid,'created_at':now(),'pid':os.getpid(),'mode':mode,'status':'RUNNING','planned_episodes':len(spec),'planned_sync_transactions':len(spec)*2,'units':[],'validity_level':'paper-candidate' if mode=='formal' else 'diagnostic','freeze':str(freeze) if freeze else None,'config_sha256':sha(CONFIG),'script_sha256':{p.name:sha(p) for p in HERE.iterdir() if p.is_file()},'node_mode':cfg['node_mode'],'normal_commit':control['implementation_commit'],'failure_policy':'pilot covers remaining cells in current round; failed round blocks next round and formal; no automatic reruns'}
 write(out/'manifest.preflight.json',m);write(out/'manifest.progress.json',m);write(out/'config.json',cfg)
 if freeze:write(out/'freeze.json',load(freeze))
 try:
  for n,(op,term,cond,r) in enumerate(spec,1):
   if mode=='pilot' and any(u['repetition']<r and u['status']!='PASS' for u in m['units']):raise RuntimeError('complete preceding-round audit contains failed cells; see failures.json')
   name=f'{n:04d}-{op}-{term}-{cond}-r{r}'
   result=episode(out,logs,m,cfg,control,name,op,term,cond,r);m['units'].append(result)
   failures=[u for u in m['units'] if u['status']!='PASS'];write(out/'failures.json',failures)
   m.update(passed_episodes=len(m['units'])-len(failures),failed_episodes=len(failures),heartbeat_at=now());write(out/'manifest.progress.json',m)
   print(json.dumps({'completed':len(m['units']),'planned':len(spec),**result}),flush=True)
   if failures and mode=='formal':raise RuntimeError('formal checker failure; stopping')
  if any(u['status']!='PASS' for u in m['units']):raise RuntimeError('coverage audit completed with failed cells; see failures.json')
  m['status']='COMPLETED_FORMAL' if mode=='formal' else 'COMPLETED_TEST';m['execution_complete']=True
 except BaseException as ex:
  m['status']='FAILED';m['error']={'type':type(ex).__name__,'message':str(ex)};raise
 finally:
  m['finished_at']=now();write(out/'manifest.terminal.json',m);write(out/'manifest.progress.json',m);lock.close()
 return out
if __name__=='__main__':
 ap=argparse.ArgumentParser();ap.add_argument('--mode',choices=['diagnostic','pilot','formal'],default='diagnostic');ap.add_argument('--freeze');ap.add_argument('--operations',nargs='+');ap.add_argument('--terminals',nargs='+');ap.add_argument('--conditions',nargs='+');a=ap.parse_args();print(run(a.mode,a.freeze,a.operations,a.terminals,a.conditions))
