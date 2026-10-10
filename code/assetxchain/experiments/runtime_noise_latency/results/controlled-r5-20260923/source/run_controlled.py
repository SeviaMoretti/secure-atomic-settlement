#!/usr/bin/env python3
"""Run isolated fresh dev chains sequentially. Never terminate unrelated processes."""
import argparse,datetime,hashlib,json,os,signal,socket,subprocess,time,shutil
from pathlib import Path
ROOT=Path("/home/gz785/code/assetxchain")
EXP=ROOT/"experiments/runtime_noise_latency"
BINS={v:EXP/"artifacts"/("node-"+v) for v in ["dual_prefix","single_tree"]}
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def run(*args):return subprocess.check_output(args,text=True).strip()
def main():
 p=argparse.ArgumentParser()
 p.add_argument("--run-id",default="controlled-"+datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ"))
 p.add_argument("--levels",default="0,512,2048");p.add_argument("--repetitions",type=int,default=5)
 p.add_argument("--rpc-port",type=int,default=18944)
 a=p.parse_args(); levels=list(map(int,a.levels.split(",")))
 assert a.repetitions>0 and 0 in levels and len(set(levels))==len(levels)
 out=EXP/"results"/a.run_id;out.mkdir(parents=True,exist_ok=False)
 (out/"source").mkdir()
 for name in ["condition_v2.js","run_controlled.py","analyze_controlled.py","single-tree.patch"]:
  shutil.copy2(EXP/name,out/"source"/name)
 config={"schema":"runtime-noise-controlled-v2","levels":levels,"repetitions":a.repetitions,
  "probe_count_per_cell":1,"seed_base":20260923,"noise_phase_ms":200,"target_phase_ms":4000,
  "classification":"exploratory live-chain; finite paired fresh-chain repetitions",
  "host":run("uname","-a"),"cpu":run("lscpu"),"rustc":run("rustc","--version"),
  "nodejs":run("node","--version"),"commit":run("git","-C",str(ROOT),"rev-parse","HEAD"),
  "git_status":run("git","-C",str(ROOT),"status","--porcelain=v1"),
  "source_sha256":{str(x.relative_to(EXP)):sha(x) for x in [EXP/"condition_v2.js",EXP/"run_controlled.py"]},
  "binary_sha256":{k:sha(v) for k,v in BINS.items()},
  "single_patch":run("git","-C","/home/gz785/code/assetxchain-noise-single","diff","--","pallets/dataassets/src/lib.rs"),
  "jobs":[]}
 (out/"manifest.json").write_text(json.dumps(config,indent=2)+"\n")
 for rep in range(a.repetitions):
  for ni,noise in enumerate(levels):
   variants=list(BINS)
   if (rep+ni)%2:variants.reverse()
   for variant in variants:
    # Explicitly fail if any chosen endpoint is occupied.
    for port in [a.rpc_port,31333]:
     s=socket.socket();s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1);s.bind(("127.0.0.1",port));s.close()
    stem=f"r{rep:02d}_{variant}_n{noise}"
    command=[str(BINS[variant]),"--dev","--tmp","--rpc-methods","unsafe",
     "--rpc-port",str(a.rpc_port),"--port","31333","--no-prometheus","--no-telemetry",
     "--no-mdns"]
    print("START",stem,flush=True)
    with (out/(stem+".node.log")).open("w") as log:
     proc=subprocess.Popen(command,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
     try:
      for _ in range(120):
       if proc.poll() is not None:raise RuntimeError("node failed: "+stem)
       if "Running JSON-RPC server" in (out/(stem+".node.log")).read_text(errors="replace"):break
       time.sleep(.5)
      else:raise RuntimeError("RPC not ready")
      cmd=["node",str(out/"source"/"condition_v2.js"),f"ws://127.0.0.1:{a.rpc_port}",variant,str(noise),str(20260923+rep),str(out/(stem+".json"))]
      with (out/(stem+".driver.log")).open("w") as driver:
       completed=subprocess.run(cmd,stdout=driver,stderr=subprocess.STDOUT,timeout=300)
      job={"stem":stem,"repetition":rep,"variant":variant,"noise":noise,"command":command,"driver":cmd,"exit_code":completed.returncode}
      config["jobs"].append(job);(out/"manifest.json").write_text(json.dumps(config,indent=2)+"\n")
      if completed.returncode:raise RuntimeError("driver failed: "+stem)
      print("DONE",stem,flush=True)
     finally:
      if proc.poll() is None:
       os.killpg(proc.pid,signal.SIGINT)
       try:proc.wait(timeout=20)
       except subprocess.TimeoutExpired:
        os.killpg(proc.pid,signal.SIGTERM);proc.wait(timeout=10)
 print(out,flush=True)
if __name__=="__main__":main()
