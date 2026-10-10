import subprocess,json,hashlib,datetime
from pathlib import Path
ROOT=Path('/home/gz785/code/exp2')
for b,original in [('B2','assetxchain-b2'),('B1','assetxchain-b1'),('G3','assetxchain-g3-v2')]:
 repo=ROOT.parent/('assetxchain-h4-'+b.lower());target=ROOT.parent/original/'target';log=ROOT/'logs'/('h4-build-'+b.lower()+'.log')
 import os
 env=os.environ.copy();env['CARGO_TARGET_DIR']=str(target)
 with log.open('ab') as f:subprocess.run(['/home/gz785/.cargo/bin/cargo','+1.85.0','build','--release','-p','assetxchain-node'],cwd=repo,env=env,stdout=f,stderr=subprocess.STDOUT,check=True)
 if subprocess.check_output(['git','-C',str(repo),'status','--porcelain'],text=True).strip():
  subprocess.run(['git','-C',str(repo),'add','Cargo.lock'],check=True);subprocess.run(['git','-C',str(repo),'commit','-m','build(h4): freeze node instrumentation dependency graph'],check=True)
 assert not subprocess.check_output(['git','-C',str(repo),'status','--porcelain'],text=True).strip()
 dest=ROOT/'build/h4-v1-online'/b;dest.mkdir(parents=True,exist_ok=True)
 import shutil
 binary=dest/'assetxchain-node';shutil.copy2(target/'release/assetxchain-node',binary)
 with binary.open('rb') as f:h=hashlib.file_digest(f,'sha256').hexdigest()
 info={'baseline':b,'worktree':str(repo),'commit':subprocess.check_output(['git','-C',str(repo),'rev-parse','HEAD'],text=True).strip(),'dirty':False,'node':{'path':str(binary),'sha256':h},'build_command':'cargo +1.85.0 build --release -p assetxchain-node','build_log':str(log),'built_at':datetime.datetime.now().astimezone().isoformat(),'toggle':'H4_EXECUTION_TIMING=0/1','boundary':'WasmExecutor::call for BlockBuilder_apply_extrinsic; includes host/runtime boundary overhead'}
 (dest/'provenance.json').write_text(json.dumps(info,indent=2));print(json.dumps(info),flush=True)
