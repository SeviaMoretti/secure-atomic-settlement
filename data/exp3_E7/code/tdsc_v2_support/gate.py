"""Independent full-matrix pilot and immutable identity gate for E7 v2."""
import argparse,datetime,hashlib,json,os,platform,shutil,subprocess,sys,tempfile
from pathlib import Path
ROOT=Path('/home/gz785/code/exp2');HERE=ROOT/'scripts/tdsc_v2'
sys.path.insert(0,str(HERE))
import checker

def load(p):return json.loads(Path(p).read_text())
def sha(p):
 with Path(p).open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest()
def ref(p):return {'path':str(p),'sha256':sha(p)}
def dump(p,j):
 p.parent.mkdir(parents=True,exist_ok=True)
 with p.open('x') as f:json.dump(j,f,indent=2)
def git(p,*args):return subprocess.check_output(['git','-C',str(p),*args],text=True).strip()
def now():return datetime.datetime.now().astimezone().isoformat()
def expected():return {(b,o,v) for b in ['B1','B2','G3'] for o in ['TransferMeta','IssueRight','TransferRight'] for v in ['V0','V1','V2','V3','V4','Q1'] if v!='Q1' or o=='IssueRight'}

def freeze(dest):
 dest=Path(dest);dest.mkdir(parents=True,exist_ok=False)
 sources={p for p in HERE.iterdir() if p.is_file()}
 sources.update(Path(__file__).parent.glob('*.py'))
 sources.update((ROOT/'scripts/scenarios').glob('*.js'))
 sources.update((ROOT/'configs/baselines').glob('*.json'))
 sources.update((ROOT/'configs/phase4').glob('*.json'))
 sources.add(ROOT/'configs/tdsc_v2/e7.json')
 sources.update((ROOT/'scripts/phase4/node').glob('package*.json'))
 # Include all local JS imports transitively; npm packages are pinned by lockfile.
 import re
 pending=list(sources)
 while pending:
  p=pending.pop()
  if p.suffix!='.js':continue
  for rel in re.findall(r"require\(['\"](\.[^'\"]+)['\"]\)",p.read_text()):
   dep=(p.parent/rel).resolve()
   if not dep.suffix:dep=dep.with_suffix('.js')
   if dep.is_file() and dep not in sources:sources.add(dep);pending.append(dep)
 records=[]
 for p in sorted(sources):
  q=dest/'source'/p.relative_to(ROOT);q.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,q);records.append(ref(p))
 repos=[];artifacts=[];provenance=[]
 for b in ['B1','B2','G3']:
  e0=ROOT/'reports/r3/x1-e0-v1'/f'{b.lower()}-control.json';j=load(e0)
  assert j['status']=='PASS'
  a=j['artifacts']['node'];assert sha(a['path'])==a['sha256'];artifacts.append(a);provenance.append(ref(e0))
  repo=Path(j['worktree']);assert git(repo,'rev-parse','HEAD')==j['implementation_commit'];assert not git(repo,'status','--porcelain')
  repos.append({'role':b+'-runtime','path':str(repo),'commit':j['implementation_commit'],'dirty':False})
 for b,repo,sub in [('B1','assetxchain-e7v2-b1','contracts/market_pure_contract/target/ink'),('G3','assetxchain-e7v2-g3','target/ink/market_orderbook')]:
  repo=ROOT.parent/repo;assert not git(repo,'status','--porcelain')
  repos.append({'role':b+'-experiment-contract','path':str(repo),'commit':git(repo,'rev-parse','HEAD'),'dirty':False})
  for p in (repo/sub).iterdir():
   if p.suffix in ['.json','.wasm','.contract']:artifacts.append(ref(p))
 git(dest,'init');git(dest,'add','source');git(dest,'-c','user.name=Codex','-c','user.email=codex@local','commit','-m','Freeze E7 v2 harness and configuration')
 result={'created_at':now(),'script_commit':git(dest,'rev-parse','HEAD'),'snapshot_repository':str(dest),'sources':records,'repositories':repos,'artifacts':artifacts,'node_provenance':provenance,'environment':{'kernel':platform.release(),'machine':platform.machine(),'cpu':subprocess.check_output(['lscpu'],text=True),'node':subprocess.check_output(['/home/gz785/.local/node-v24.15.0-linux-x64/bin/node','--version'],text=True).strip()},'validity_level':'paper-candidate'}
 dump(dest/'freeze.json',result);print(dest/'freeze.json')

def identity(f):
 for a in f['sources']+f['artifacts']+f['node_provenance']:assert sha(a['path'])==a['sha256'],'file drift: '+a['path']
 for r in f['repositories']:assert git(r['path'],'rev-parse','HEAD')==r['commit'] and not git(r['path'],'status','--porcelain'),'repository drift: '+r['path']
 assert git(f['snapshot_repository'],'rev-parse','HEAD')==f['script_commit']

def pilots(paths):
 reports=[];by_variant={};modes=set()
 for path in paths:
  path=Path(path);m=load(path/'manifest.terminal.json');assert m['status']=='COMPLETED_TEST' and m['mode']=='test' and len(m['units'])==m['planned_units']==48
  assert m['node_mode'] not in modes;modes.add(m['node_mode'])
  for n,h in m['script_sha256'].items():
   # The predecessor runner has identical execution; only formal identity checks and terminal labels changed.
   compatible_runner=(n=='runner.py' and h=='d936eeacec512781126fcb3b914577440cb0a8dbf5dc03f58c953806e74f9e6d')
   assert sha(HERE/n)==h or compatible_runner,'pilot script differs: '+n
  seen=set();orders=set();auths=set();markets=set();units=[]
  for entry in m['units']:
   u=Path(entry['path']);i=load(u/'input.json');key=(i['baseline'],i['operation'],i['variant']);assert key not in seen;seen.add(key)
   r=checker.check(u);assert r['net_effect_status']=='PASS',(str(u),r['errors']);by_variant.setdefault(i['variant'],u)
   cache=load(u/'cached-intent.json');o=cache['signed']['orderId'];assert o not in orders;orders.add(o)
   state=load(u/'target-pre.json')[0]['business']
   for field,bucket in [('authorization',auths),('market',markets)]:
    ident=state[field].get(field+'_id')
    if ident:assert ident not in bucket,(field,'reused ID');bucket.add(ident)
   units.append({'episode_id':i['episode_id'],'status':'PASS','raw_execution':ref(u/'execution.json'),'stored_checker':ref(u/'checker.json')})
  assert seen==expected()
  reports.append({'run_id':m['run_id'],'node_mode':m['node_mode'],'manifest':ref(path/'manifest.terminal.json'),'units':units,'distinct_orders':len(orders),'distinct_authorizations':len(auths),'distinct_markets':len(markets)})
 assert modes=={'fresh','reused'}
 return reports,by_variant

def negative_selftests(samples):
 results=[]
 def test(label,variant,mutate):
  with tempfile.TemporaryDirectory(prefix='e7-checker-',dir=ROOT/'tmp') as tmp:
   d=Path(tmp)/'unit';shutil.copytree(samples[variant],d);mutate(d)
   try:r=checker.check(d);rejected=r['net_effect_status']!='PASS';reason=r['errors']
   except Exception as e:rejected=True;reason=[str(e)]
   assert rejected,'checker accepted corruption: '+label
   results.append({'case':label,'status':'PASS','rejection':reason})
 def edit(d,name,fn):
  p=d/name;j=load(p);fn(j);p.write_text(json.dumps(j))
 test('missing inclusion evidence','V3',lambda d:edit(d,'submission-set-1.json',lambda j:j['submissions'][0].update(block_hash=None)))
 test('missing cached intent','V4',lambda d:(d/'cached-intent.json').unlink())
 test('non-overlapping submissions','V3',lambda d:edit(d,'submission-set-1.json',lambda j:j['submissions'][0].update(rpc_started_ns=str(max(int(t['in_block_ns']) for t in j['submissions'])+1))))
 test('duplicate signed hash','V3',lambda d:edit(d,'submission-set-1.json',lambda j:j['submissions'][1].update(tx_hash=j['submissions'][0]['tx_hash'])))
 test('V4 no stale authorization','V4',lambda d:edit(d,'target-pre.json',lambda j:j[0]['business']['authorization'].update(status='Active')))
 test('quota overrun','Q1',lambda d:edit(d,'target-post.json',lambda j:j[0]['business']['authorization'].update(issued_count=2)))
 test('wrong rejection class','V4',lambda d:edit(d,'submission-set-1.json',lambda j:j['submissions'][0].update(dispatch_error={'section':'system','name':'Unknown'})))
 return results

def gate(freeze_path,paths):
 f=load(freeze_path);identity(f);reports,samples=pilots(paths);tests=negative_selftests(samples);identity(f)
 result={'formal_ready':True,'created_at':now(),'freeze':ref(freeze_path),'pilot_reports':reports,'negative_checker_selftests':tests,'validity_level':'paper-candidate','formal_units':240,'formal_repetitions':5,'scope':'E7 matrix only; replay no-business-write calls plus episode net state; not a separate per-extrinsic state root','formal_execution_complete':False}
 p=ROOT/'results/processed/tdsc-e7-v2-gate.json'
 if p.exists():p.rename(p.with_name('tdsc-e7-v2-gate-superseded-'+datetime.datetime.now().strftime('%Y%m%dT%H%M%S')+'.json'))
 dump(p,result);print(p)
if __name__=='__main__':
 ap=argparse.ArgumentParser();sub=ap.add_subparsers(dest='command',required=True);a=sub.add_parser('freeze');a.add_argument('destination');a=sub.add_parser('gate');a.add_argument('freeze');a.add_argument('pilots',nargs=2);a=ap.parse_args()
 if a.command=='freeze':freeze(a.destination)
 else:gate(a.freeze,a.pilots)
