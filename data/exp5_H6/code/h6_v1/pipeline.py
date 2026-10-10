"""Preexperiment -> full offline gate -> detached formal run, no formal polling."""
import sys,subprocess,time,os,traceback
from pathlib import Path
from runner import ROOT,HERE,now,write,load
from gate import identity,pilot_gate,validate_formal
def main():
 freeze=Path(sys.argv[1]);out=Path(sys.argv[2]);out.mkdir(parents=True,exist_ok=True)
 status={'created_at':now(),'pid':os.getpid(),'freeze':str(freeze),'stage':'PREFLIGHT','monitor_formal':False};write(out/'status.json',status)
 try:
  identity(load(freeze));before=set((ROOT/'results/raw').glob('tdsc-h6-v1-pilot-*'))
  with (out/'pilot.log').open('xb') as log:
   proc=subprocess.Popen(['python3',str(HERE/'runner.py'),'--mode','pilot','--freeze',str(freeze)],stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
   status.update(stage='PILOT_RUNNING',runner_pid=proc.pid);write(out/'status.json',status)
   while proc.poll() is None:
    runs=set((ROOT/'results/raw').glob('tdsc-h6-v1-pilot-*'))-before
    if len(runs)==1:status['current_run']=str(next(iter(runs)))
    status['heartbeat_at']=now();write(out/'status.json',status);time.sleep(5)
   if proc.returncode:raise RuntimeError('H6 pilot failed; see pilot log and terminal manifest')
  runs=set((ROOT/'results/raw').glob('tdsc-h6-v1-pilot-*'))-before;assert len(runs)==1;pilot=next(iter(runs))
  status.update(stage='PILOT_GATE',current_run=str(pilot));write(out/'status.json',status)
  gate=pilot_gate(freeze,pilot);write(out/'pilot-gate.json',gate);validate_formal(freeze)
  before=set((ROOT/'results/raw').glob('tdsc-h6-v1-formal-*'))
  with (out/'formal.log').open('xb') as log:
   proc=subprocess.Popen(['python3',str(HERE/'runner.py'),'--mode','formal','--freeze',str(freeze)],stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
  deadline=time.monotonic()+60
  while time.monotonic()<deadline:
   if proc.poll() is not None:raise RuntimeError('formal runner exited before startup confirmation')
   runs=set((ROOT/'results/raw').glob('tdsc-h6-v1-formal-*'))-before
   if len(runs)==1 and (next(iter(runs))/'manifest.progress.json').exists():
    status.update(stage='FORMAL_LAUNCHED_NO_MONITOR',formal_pid=proc.pid,formal_started_at=now(),current_run=str(next(iter(runs))));write(out/'status.json',status);return
   time.sleep(1)
  raise RuntimeError('formal startup not confirmed within 60 seconds')
 except Exception as ex:
  status.update(stage='FAILED',error=str(ex),failed_at=now());write(out/'status.json',status);raise
if __name__=='__main__':main()
