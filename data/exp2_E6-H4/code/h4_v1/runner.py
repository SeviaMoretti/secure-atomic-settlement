"""H4 owned-node sessions; heavy checking only after timed session ends."""
import argparse,datetime,json,subprocess,socket,time,signal,os,hashlib,random
from pathlib import Path
ROOT=Path('/home/gz785/code/exp2');HERE=ROOT/'scripts/h4_v1';NODE='/home/gz785/.local/node-v24.15.0-linux-x64/bin/node'
CONFIG=ROOT/'configs/h4_v1/config.json'
def now():return datetime.datetime.now().astimezone().isoformat()
def load(p):return json.loads(Path(p).read_text())
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def write(p,j):
 p=Path(p);t=p.with_suffix('.new');t.write_text(json.dumps(j,indent=2)+'\n');t.replace(p)
def port():
 with socket.socket() as s:s.settimeout(.2);return s.connect_ex(('127.0.0.1',9944))==0
def plan(mode,node_mode,baselines,operations):
 cfg=load(CONFIG);rng=random.Random(cfg['seed']);sessions=[]
 rounds=10 if mode=='formal' else 5 if mode=='calibration' else 1 if mode=='probe' else 2
 for rep in range(1,rounds+1):
  bs=list(baselines);rng.shuffle(bs)
  for b in bs:
   if node_mode=='fresh':
    ops=list(operations);rng.shuffle(ops)
    for op in ops:sessions.append({'baseline':b,'round':rep,'units':[{'operation':op,'kind':'sample','within_cell':1}]})
   else:
    units=[{'operation':op,'kind':'warmup','within_cell':0} for op in operations]
    for k in range(1,4 if mode=='formal' else 2):
     ops=list(operations);rng.shuffle(ops);units.extend({'operation':op,'kind':'sample','within_cell':k} for op in ops)
    if mode=='calibration':
     toggles=['off','on'];rng.shuffle(toggles)
     for toggle in toggles:sessions.append({'baseline':b,'round':rep,'units':units,'instrumentation':toggle,'pair_id':f'{b}-r{rep}'})
    else:sessions.append({'baseline':b,'round':rep,'units':units})
 return sessions

def main():
 ap=argparse.ArgumentParser();ap.add_argument('--mode',choices=['probe','pilot','calibration','formal'],default='pilot');ap.add_argument('--node-mode',choices=['fresh','reused'],default='reused');ap.add_argument('--instrumentation',choices=['off','on','paired'],default='paired');ap.add_argument('--baselines',nargs='+',default=['B1','B2','G3']);ap.add_argument('--operations',nargs='+',default=['TransferMeta','IssueRight','TransferRight']);a=ap.parse_args()
 if a.mode in ['formal','calibration']:
  if a.node_mode!='reused' or set(a.baselines)!={'B1','B2','G3'} or set(a.operations)!={'TransferMeta','IssueRight','TransferRight'}:raise RuntimeError('formal matrix must match frozen plan')
  from gate import validate_formal
  gate=validate_formal(a.mode)
 if port():raise RuntimeError('RPC occupied')
 specs=plan(a.mode,a.node_mode,a.baselines,a.operations);run='tdsc-h4-v1-'+a.mode+'-'+a.node_mode+'-'+datetime.datetime.now().strftime('%Y%m%dT%H%M%S')
 out=ROOT/'results/raw'/run;logs=ROOT/'logs'/run;out.mkdir();logs.mkdir()
 m={'run_id':run,'mode':a.mode,'node_mode':a.node_mode,'status':'RUNNING','created_at':now(),'pid':os.getpid(),'planned_samples':sum(u['kind']=='sample' for s in specs for u in s['units']),'planned_warmups':sum(u['kind']=='warmup' for s in specs for u in s['units']),'planned_sessions':len(specs),'units':[],'sessions':[],'script_sha256':{p.name:sha(p) for p in HERE.iterdir() if p.is_file()},'config_sha256':sha(CONFIG),'seed':load(CONFIG)['seed'],'data_level':'paper-candidate' if a.mode in ['formal','calibration'] else 'pilot','node_execution_ms':None,'online_instrumentation_calibration':'PLANNED_PAIRED_ON_OFF'}
 if a.mode in ['formal','calibration']:write(out/'gate.json',gate);write(out/'freeze.json',load(gate['freeze']['path']))
 write(out/'manifest.preflight.json',m);write(out/'manifest.progress.json',m);write(out/'session-plan.json',specs)
 proc=None;child=None;node_log=None
 def stop():
  nonlocal proc,node_log
  if child is not None and child.poll() is None:
   child.terminate()
   try:child.wait(10)
   except subprocess.TimeoutExpired:child.kill();child.wait()
  if proc is not None and proc.poll() is None:
   proc.send_signal(signal.SIGINT)
   try:proc.wait(20)
   except subprocess.TimeoutExpired:proc.terminate();proc.wait(10)
  if node_log:node_log.close()
  proc=None;node_log=None
 try:
  for n,s in enumerate(specs,1):
   instrumentation=s.get('instrumentation',('on' if s['round']%2==1 else 'off') if a.instrumentation=='paired' and a.mode=='pilot' else 'off' if a.instrumentation=='paired' else a.instrumentation)
   b=s['baseline'];sid=f'{n:03d}-{b}-r{s["round"]}';sd=out/'sessions'/sid;sd.mkdir(parents=True)
   provenance=load(ROOT/'reports/r3/x1-e0-v1'/f'{b.lower()}-control.json')
   for art in provenance['artifacts'].values():
    if sha(art['path'])!=art['sha256']:raise RuntimeError('baseline artifact changed')
   online=load(ROOT/'build/h4-v1-online'/b/'provenance.json')
   if sha(online['node']['path'])!=online['node']['sha256']:raise RuntimeError('online node drift')
   binary=online['node']['path'];artifact=str(Path(provenance['artifacts']['contract_metadata']['path']).parent) if b!='B2' else None
   node_log=(logs/(sid+'.node.log')).open('xb');cmd=[binary,'--dev','--tmp','--rpc-cors','all','--rpc-methods','unsafe','--disable-log-color','--detailed-log-output']
   node_env=os.environ.copy();node_env['H4_EXECUTION_TIMING']='1' if instrumentation=='on' else '0'
   proc=subprocess.Popen(cmd,stdout=node_log,stderr=subprocess.STDOUT,start_new_session=True,env=node_env)
   until=time.monotonic()+90
   while not port():
    if proc.poll() is not None or time.monotonic()>until:raise RuntimeError('node readiness failed')
    time.sleep(.3)
   units=[]
   for ix,u in enumerate(s['units'],1):
    name=f'{ix:02d}-{u["operation"]}-{u["kind"]}-{u["within_cell"]}';p=sd/'units'/name
    units.append({**u,'baseline':b,'repetition':s['round'],'run_id':run,'episode_id':run+'/'+sid+'/'+name,'session_id':sid,'session_index':n,'session_order_index':ix,'prior_complete_orders':ix-1,'output_root':str(p),'rpc':'ws://127.0.0.1:9944','artifact_root':artifact,'node_mode':a.node_mode,'instrumentation':instrumentation,'node_log':str(logs/(sid+'.node.log')),'calibration_pair_id':s.get('pair_id'),'fixture_id':f'h4-calibration-{b}-r{s["round"]}-{name}' if a.mode=='calibration' else run+'/'+sid+'/'+name})
   session={'baseline':b,'session_id':sid,'rpc':'ws://127.0.0.1:9944','output_root':str(sd),'artifact_root':artifact,'node_pid':proc.pid,'units':units,'node_command':cmd,'provenance':provenance,'online_provenance':online,'instrumentation':instrumentation};write(sd/'session.json',session)
   m['current_session']=sid;m['session_started_at']=now();m['node_pid']=proc.pid;write(out/'manifest.progress.json',m)
   env=os.environ.copy();env['POLKADOT_DRIVER_ROOT']=str(ROOT/'scripts/phase4/node')
   with (logs/(sid+'.executor.log')).open('xb') as log:
    child=subprocess.Popen([NODE,str(HERE/'session.js'),str(sd/'session.json')],stdout=log,stderr=subprocess.STDOUT,env=env)
    until=time.monotonic()+load(CONFIG)['session_timeout_seconds']
    while child.poll() is None:
     if proc.poll() is not None:raise RuntimeError('owned node exited')
     if time.monotonic()>until:raise RuntimeError('session timeout')
     m['heartbeat_at']=now();m['session_executed']=len(list((sd/'units').glob('*/complete.json')));m['executor_pid']=child.pid;write(out/'manifest.progress.json',m);time.sleep(1)
    if child.returncode:raise RuntimeError('session executor failed: '+sid)
   stop()
   for u in units:
    p=Path(u['output_root']);checked=subprocess.run(['python3',str(HERE/'checker.py'),str(p)],capture_output=True,text=True,timeout=60)
    c=load(p/'checker.json');m['units'].append({'path':str(p),'episode_id':u['episode_id'],'baseline':b,'operation':u['operation'],'repetition':u['repetition'],'kind':u['kind'],'session_id':sid,'status':c['status']});write(out/'manifest.progress.json',m)
    if checked.returncode:raise RuntimeError('independent checker: '+str(c['errors']))
   m['sessions'].append({'session_id':sid,'status':'PASS','samples':sum(u['kind']=='sample' for u in units),'warmups':sum(u['kind']=='warmup' for u in units)});write(out/'manifest.progress.json',m)
   print(json.dumps({'session':sid,'checked_samples':sum(u['kind']=='sample' for u in m['units']),'planned_samples':m['planned_samples'],'checked_warmups':sum(u['kind']=='warmup' for u in m['units'])}),flush=True)
  m['status']='COMPLETED_FORMAL' if a.mode in ['formal','calibration'] else 'COMPLETED_TEST';m['paper_final']=False
 except BaseException as e:m['status']='FAILED';m['error']=type(e).__name__+': '+str(e);raise
 finally:stop();m['finished_at']=now();write(out/'manifest.terminal.json',m);write(out/'manifest.progress.json',m)
if __name__=='__main__':main()
