"""Identity and coverage gates for the H6 extension; never inherits paper-final."""
import sys,json,subprocess,shutil,hashlib,re,platform,tempfile,collections
from pathlib import Path
from runner import ROOT,HERE,CONFIG,CONTROL,load,sha,now,write
from checker import check
GATE=ROOT/'results/processed/h6-v1-gate.json'
def git(p,*args):return subprocess.check_output(['git','-C',str(p),*args],text=True).strip()
def ref(p):return {'path':str(p),'sha256':sha(p)}
def freeze(dest):
 dest=Path(dest);dest.mkdir(parents=True,exist_ok=False)
 files=set(p for p in HERE.iterdir() if p.is_file());files.add(CONFIG)
 for folder,pattern in [('scripts/scenarios','*.js'),('configs/baselines','*.json'),('configs/phase4','*.json'),('scripts/phase4/node','package*.json')]:files.update((ROOT/folder).glob(pattern))
 pending=list(files)
 while pending:
  p=pending.pop()
  if p.suffix!='.js':continue
  for rel in re.findall(r"require\(['\"](\.[^'\"]+)['\"]\)",p.read_text()):
   q=(p.parent/rel).resolve()
   if not q.suffix:q=q.with_suffix('.js')
   if q.is_file() and q not in files:files.add(q);pending.append(q)
 for p in files:
  q=dest/'source'/p.relative_to(ROOT);q.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,q)
 c=load(CONTROL);pro=ROOT/'build/h6-v1-contracts/provenance.json';b=load(pro)
 assert not b['dirty'];assert b['abi_added']==['h6_corrupt_projection']
 repositories=[]
 for r,h in [(c['worktree'],c['implementation_commit']),(b['worktree'],b['commit'])]:
  assert git(r,'rev-parse','HEAD')==h and not git(r,'status','--porcelain');repositories.append({'path':r,'commit':h,'dirty':False})
 arts=list(c['artifacts'].values())+[v for mode in b['artifacts'].values() for v in mode.values()]
 for a in arts:assert sha(a['path'])==a['sha256']
 e7=ROOT/'reports/e7-v2-formal-independent-recheck-20260921.json';e=load(e7);assert e['identity']=='PASS' and len(e['units'])==240 and all(u['status']=='PASS' for u in e['units'])
 offline=ROOT/'reports/h6-preflight-latest.json';audit=load(offline);assert audit['status']=='PASS' and audit['offline_cells']==36
 for p,h in audit['sources'].items():assert sha(p)==h,'sources changed after preflight '+p
 h4=ROOT/'results/raw/tdsc-h4-v1-formal-reused-20260921T133757/manifest.terminal.json';assert load(h4)['status']=='COMPLETED_FORMAL'
 git(dest,'init');git(dest,'add','source');git(dest,'-c','user.name=Codex','-c','user.email=codex@local','commit','-m','Freeze H6 projection recovery protocol and independent checks')
 obj={'created_at':now(),'repositories':repositories,'artifacts':arts,'sources':[ref(p) for p in sorted(files)],'provenance':[ref(CONTROL),ref(pro),ref(e7),ref(h4)],'test_logs':[ref(ROOT/'build/h6-v1-contracts'/name) for name in ['normal-tests.log','fault-tests.log']],'snapshot_repository':str(dest),'script_commit':git(dest,'rev-parse','HEAD'),'hardware':{'kernel':platform.release(),'cpu':subprocess.check_output(['lscpu'],text=True),'memory':Path('/proc/meminfo').read_text()},'validity_level':'paper-candidate','f0_f1_f2':'original frozen normal artifact','f3':'separate gated fault artifact; only projection mutation','excluded':'I11; automatic order discovery; node crash recovery'}
 obj['provenance'].append(ref(offline))
 obj['provenance'].extend(ref(Path(audit['report_dir'])/name) for name in audit['artifacts'])
 write(dest/'freeze.json',obj);return dest/'freeze.json'
def identity(f):
 for a in f['sources']+f['artifacts']+f['provenance']+f['test_logs']:assert sha(a['path'])==a['sha256'],'drift '+a['path']
 for r in f['repositories']:assert git(r['path'],'rev-parse','HEAD')==r['commit'] and not git(r['path'],'status','--porcelain'),'repository drift'
 assert git(f['snapshot_repository'],'rev-parse','HEAD')==f['script_commit']
def recheck(run):
 p=Path(run);m=load(p/'manifest.terminal.json');assert m['status'] in ['COMPLETED_TEST','COMPLETED_FORMAL']
 assert len(m['units'])==m['planned_episodes'];ids=set();counts=collections.Counter()
 assert m['config_sha256']==sha(CONFIG)
 for n,h in m['script_sha256'].items():assert sha(HERE/n)==h,'run scripts changed '+n
 for u in m['units']:
  root=Path(u['path']);assert sha(root/'inventory.json')==u['inventory_sha256']
  for n,h in load(root/'inventory.json').items():assert sha(root/n)==h,'raw evidence drift '+str(root/n)
  r=check(root);assert r['status']=='PASS',(str(root),r['errors'])
  i=load(root/'input.json');key=(i['operation'],i['terminal'],i['condition'],i['repetition']);assert key not in ids;ids.add(key);counts[key[:3]]+=1
 cfg=load(CONFIG);reps=cfg['pilot_repetitions'] if m['mode']=='pilot' else cfg['formal_repetitions']
 assert counts==collections.Counter({(o,t,c):reps for o in cfg['operations'] for t in cfg['terminals'] for c in cfg['conditions']}),'coverage incomplete'
 return m
def negatives(sample):
 cases=[('missing-finality','transactions.json',lambda j:j[-1].update(c_fin=None)),('wrong-inclusion','transactions.json',lambda j:j[-1].update(block_hash='0x'+'00'*32)),('backward-clock','transactions.json',lambda j:j[-1].update(c_in='0')),('missing-projection','snapshot-after1.json',lambda j:j.update(projection_return_scale='0x0000')),('wrong-terminal','snapshot-after2.json',lambda j:j['order'].update(state='Open')),('authority-mutated','snapshot-after1.json',lambda j:j['authority'].update(asset='0x00')),('unexplained-payment','snapshot-after2.json',lambda j:next(iter(j['balances'].values())).update(free='1')),('wrong-fee','transactions.json',lambda j:j[-1].update(fee='1')),('missing-sync-event','transactions.json',lambda j:j[-1].update(contract_events=[]))]
 result=[]
 for name,file,fn in cases:
  with tempfile.TemporaryDirectory(prefix='h6-check-') as tmp:
   dest=Path(tmp)/'unit';shutil.copytree(sample,dest);j=load(dest/file);fn(j);write(dest/file,j);assert check(dest,payload=False)['status']=='FAIL','negative checker accepted '+name
   result.append({'case':name,'status':'PASS'})
 return result
def pilot_gate(frozen,run):
 f=load(frozen);identity(f);m=recheck(run);assert m['mode']=='pilot' and len(m['units'])==72
 result={'created_at':now(),'formal_ready':True,'freeze':ref(frozen),'pilot':ref(Path(run)/'manifest.terminal.json'),'pilot_episodes':72,'sync_transactions':144,'coverage':'36 cells x 2','negative_selftests':negatives(m['units'][0]['path']),'formal_episodes':360,'formal_sync_transactions':720,'validity_level':'paper-candidate'}
 write(GATE,result);return result
def validate_formal(frozen):
 g=load(GATE);assert g['formal_ready'] and sha(frozen)==g['freeze']['sha256'];identity(load(frozen));assert sha(g['pilot']['path'])==g['pilot']['sha256'];return g
if __name__=='__main__':
 if sys.argv[1]=='freeze':print(freeze(sys.argv[2]))
 elif sys.argv[1]=='pilot':print(json.dumps(pilot_gate(sys.argv[2],sys.argv[3])))
