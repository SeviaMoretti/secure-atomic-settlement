"""Execute H4 pilots, paired calibration, then launch formal main without monitoring it."""
import datetime,json,os,subprocess,sys,time
from pathlib import Path
from gate import ROOT,HERE,load,identity,pilot_gate,calibration_gate,validate_formal,ref
from runner import write,now

def main():
 frozen=Path(sys.argv[1]);report=Path(sys.argv[2]);report.mkdir(parents=True,exist_ok=False)
 state={'created_at':now(),'pid':os.getpid(),'freeze':str(frozen),'stage':'STARTING','completed_runs':[],'monitor_main':False};status=report/'status.json';write(status,state)
 def batch(mode,node_mode):
  state['stage']=mode.upper()+'_'+node_mode.upper();state['current_run']=None;write(status,state);before=set((ROOT/'results/raw').glob('tdsc-h4-v1-*'))
  with (report/(mode+'-'+node_mode+'.log')).open('xb') as log:
   child=subprocess.Popen(['python3',str(HERE/'runner.py'),'--mode',mode,'--node-mode',node_mode],stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
   state['runner_pid']=child.pid;write(status,state)
   while child.poll() is None:
    added=set((ROOT/'results/raw').glob('tdsc-h4-v1-*'))-before
    if len(added)==1:state['current_run']=str(next(iter(added)))
    state['heartbeat_at']=now();write(status,state);time.sleep(5)
   if child.returncode:raise RuntimeError(mode+' runner failed; see '+str(log.name))
  added=set((ROOT/'results/raw').glob('tdsc-h4-v1-*'))-before
  if len(added)!=1:raise RuntimeError('ambiguous run identity')
  run=added.pop();state['completed_runs'].append(str(run));write(status,state);return run
 try:
  identity(load(frozen))
  reused=batch('pilot','reused');fresh=batch('pilot','fresh')
  state['stage']='PILOT_GATE';write(status,state);pilot_gate(frozen,[reused,fresh])
  calibration=batch('calibration','reused')
  state['stage']='CALIBRATION_GATE';write(status,state);calibration_gate(calibration);validate_formal('formal')
  state['stage']='LAUNCH_MAIN';write(status,state);before=set((ROOT/'results/raw').glob('tdsc-h4-v1-formal-*'))
  with (report/'formal-main.log').open('xb') as log:
   child=subprocess.Popen(['python3',str(HERE/'runner.py'),'--mode','formal','--node-mode','reused','--instrumentation','off'],stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  until=time.monotonic()+30
  while time.monotonic()<until:
   if child.poll() is not None:raise RuntimeError('formal main exited during startup')
   added=set((ROOT/'results/raw').glob('tdsc-h4-v1-formal-*'))-before
   if len(added)==1 and (next(iter(added))/'manifest.preflight.json').exists():break
   time.sleep(.2)
  else:raise RuntimeError('main startup acknowledgement timed out')
  state.update(stage='MAIN_LAUNCHED_NO_MONITOR',main_pid=child.pid,main_run=str(added.pop()),launched_at=now());write(status,state)
 except BaseException as e:
  state.update(stage='FAILED',error=type(e).__name__+': '+str(e),finished_at=now());write(status,state);raise
if __name__=='__main__':main()
