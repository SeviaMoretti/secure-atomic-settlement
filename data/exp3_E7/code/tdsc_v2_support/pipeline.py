"""Run remaining pilots, verify gate, launch formal detached, then exit."""
import datetime,json,os,subprocess,sys,time
from pathlib import Path
from gate import ROOT,HERE,load,identity,gate

def main():
 reused=Path(sys.argv[1]);frozen=Path(sys.argv[2]);report=Path(sys.argv[3]);report.mkdir(parents=True,exist_ok=False)
 state={'created_at':datetime.datetime.now().astimezone().isoformat(),'pid':os.getpid(),'stage':'WAIT_REUSED_TEST','reused_test':str(reused),'freeze':str(frozen),'listen_after_formal_launch':False}
 def save():
  p=report/'status.json';t=p.with_suffix('.new');t.write_text(json.dumps(state,indent=2));t.replace(p)
 save()
 try:
  m=load(reused/'manifest.progress.json');pid=m['pid']
  while not (reused/'manifest.terminal.json').exists():
   os.kill(pid,0)
   if time.time()-(reused/'manifest.progress.json').stat().st_mtime>120:raise RuntimeError('test heartbeat stale')
   time.sleep(5)
  m=load(reused/'manifest.terminal.json')
  if m['status']!='COMPLETED_TEST':raise RuntimeError('reused tests failed: '+str(m.get('error')))
  identity(load(frozen));state['stage']='FRESH_TEST';save()
  before=set((ROOT/'results/raw').glob('tdsc-e7-v2-test-*'))
  with (report/'fresh-test.log').open('xb') as log:
   subprocess.run(['python3',str(HERE/'runner.py'),'--node-mode','fresh'],stdout=log,stderr=subprocess.STDOUT,check=True)
  added=set((ROOT/'results/raw').glob('tdsc-e7-v2-test-*'))-before
  if len(added)!=1:raise RuntimeError('fresh run identity ambiguous')
  fresh=added.pop();state['fresh_test']=str(fresh);state['stage']='CHECK_GATE';save()
  gate(frozen,[reused,fresh]);identity(load(frozen))
  state['stage']='LAUNCHING_FORMAL';save()
  before=set((ROOT/'results/raw').glob('tdsc-e7-v2-formal-*'))
  with (report/'formal-launch.log').open('xb') as log:
   child=subprocess.Popen(['python3',str(HERE/'runner.py'),'--mode','formal','--node-mode','reused'],stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  # Startup acknowledgement only; never follow formal progress afterward.
  until=time.monotonic()+20
  while time.monotonic()<until:
   if child.poll() is not None:raise RuntimeError('formal runner exited during startup')
   added=set((ROOT/'results/raw').glob('tdsc-e7-v2-formal-*'))-before
   if len(added)==1 and (next(iter(added))/'manifest.preflight.json').exists():break
   time.sleep(.2)
  else:raise RuntimeError('formal startup acknowledgement timed out')
  formal=next(iter(added));state.update(stage='FORMAL_LAUNCHED_NO_MONITOR',formal_pid=child.pid,formal_run=str(formal),formal_launch_at=datetime.datetime.now().astimezone().isoformat());save()
 except BaseException as e:
  state.update(stage='FAILED',error=type(e).__name__+': '+str(e));save();raise
if __name__=='__main__':main()
