"""Frozen H4 identity, full pilot coverage, negative timing checks and calibration gates."""
import json,subprocess,sys,shutil,tempfile,hashlib,re,datetime,platform
from pathlib import Path
from runner import ROOT,HERE,CONFIG,load,sha,now
from checker import check
GATE=ROOT/'results/processed/h4-v1-gate.json'
def ref(p):return {'path':str(p),'sha256':sha(p)}
def dump(p,j):
 p=Path(p);p.parent.mkdir(parents=True,exist_ok=True)
 with p.open('x') as f:json.dump(j,f,indent=2)
def git(p,*args):return subprocess.check_output(['git','-C',str(p),*args],text=True).strip()
def freeze(dest):
 dest=Path(dest);dest.mkdir(parents=True,exist_ok=False)
 files={p for p in HERE.iterdir() if p.is_file()};files.update((ROOT/'scripts/scenarios').glob('*.js'));files.update((ROOT/'configs/baselines').glob('*.json'));files.update((ROOT/'configs/phase4').glob('*.json'));files.add(CONFIG);files.add(ROOT/'scripts/tdsc_v2/transport.js');files.update((ROOT/'scripts/phase4/node').glob('package*.json'))
 queue=list(files)
 while queue:
  p=queue.pop()
  if p.suffix!='.js':continue
  for rel in re.findall(r"require\(['\"](\.[^'\"]+)['\"]\)",p.read_text()):
   dep=(p.parent/rel).resolve()
   if not dep.suffix:dep=dep.with_suffix('.js')
   if dep.is_file() and dep not in files:files.add(dep);queue.append(dep)
 for p in files:
  q=dest/'source'/p.relative_to(ROOT);q.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,q)
 repos=[];arts=[];provenance=[]
 for b in ['B1','B2','G3']:
  normal=ROOT/'reports/r3/x1-e0-v1'/f'{b.lower()}-control.json';n=load(normal)
  online=ROOT/'build/h4-v1-online'/b/'provenance.json';o=load(online)
  for r,commit in [(n['worktree'],n['implementation_commit']),(o['worktree'],o['commit'])]:
   assert not git(r,'status','--porcelain');assert git(r,'rev-parse','HEAD')==commit;repos.append({'path':r,'commit':commit,'dirty':False})
  for a in list(n['artifacts'].values())+[o['node']]:assert sha(a['path'])==a['sha256'];arts.append(a)
  provenance.extend([ref(normal),ref(online)])
 git(dest,'init');git(dest,'add','source');git(dest,'-c','user.name=Codex','-c','user.email=codex@local','commit','-m','Freeze H4 timing, online execution and paired calibration')
 result={'created_at':now(),'repositories':repos,'artifacts':arts,'sources':[ref(p) for p in sorted(files)],'provenance':provenance,'snapshot_repository':str(dest),'script_commit':git(dest,'rev-parse','HEAD'),'hardware':{'kernel':platform.release(),'cpu':subprocess.check_output(['lscpu'],text=True),'memory':Path('/proc/meminfo').read_text()},'timing_scope':'client callbacks and host Runtime API execution; separate clocks; no pure-network claim'}
 dump(dest/'freeze.json',result);return dest/'freeze.json'
def identity(f):
 for a in f['sources']+f['artifacts']+f['provenance']:assert sha(a['path'])==a['sha256'],'identity drift '+a['path']
 for r in f['repositories']:assert git(r['path'],'rev-parse','HEAD')==r['commit'] and not git(r['path'],'status','--porcelain'),'source drift '+r['path']
 assert git(f['snapshot_repository'],'rev-parse','HEAD')==f['script_commit']
def recheck_run(path,mode):
 path=Path(path);m=load(path/'manifest.terminal.json');assert m['status']==('COMPLETED_FORMAL' if mode in ['formal','calibration'] else 'COMPLETED_TEST');assert m['mode']==mode
 for n,h in m['script_sha256'].items():assert sha(HERE/n)==h,'pilot source changed '+n
 assert sha(CONFIG)==m['config_sha256'];assert len(m['units'])==m['planned_samples']+m['planned_warmups']
 rows=[];ids=set()
 for u in m['units']:
  p=Path(u['path']);i=load(p/'input.json');c=check(p);assert c['status']=='PASS',(str(p),c['errors']);order=load(p/'canonical-order.json')['orderId']
  # Calibration intentionally reuses fixture identities across separate on/off chains.
  key=(i['instrumentation'],order) if mode=='calibration' else order
  assert key not in ids,'duplicate order';ids.add(key);rows.append((p,i,c))
 return m,rows

def negatives(sample):
 results=[]
 def edit(p,name,fn):j=load(p/name);fn(j);(p/name).write_text(json.dumps(j))
 cases=[('missing finalization',lambda p:edit(p,'transactions.json',lambda j:j[0].update(c_fin=None))),('backward clock',lambda p:edit(p,'transactions.json',lambda j:j[0].update(c_in='0'))),('wrong finalized block',lambda p:edit(p,'transactions.json',lambda j:j[0].update(finalized_block_hash='0x'+'00'*32))),('missing inclusion callback',lambda p:edit(p,'transactions.json',lambda j:j[0].update(statuses=[s for s in j[0]['statuses'] if s['status']!='InBlock']))),('wrong transaction identity',lambda p:edit(p,'transactions.json',lambda j:j[0].update(tx_hash='0x'+'00'*32))),('wrong settlement record',lambda p:edit(p,'post.json',lambda j:j['record'].update(gross_price='1')))]
 for name,fn in cases:
  with tempfile.TemporaryDirectory(dir=ROOT/'tmp',prefix='h4-check-') as tmp:
   p=Path(tmp)/'unit';shutil.copytree(sample,p);fn(p)
   try:c=check(p);ok=c['status']!='PASS'
   except Exception:ok=True
   assert ok,'checker accepts corruption '+name;results.append({'case':name,'status':'PASS'})
 # Corrupt only a derived copy of the node log, leaving all original evidence intact.
 with tempfile.TemporaryDirectory(dir=ROOT/'tmp',prefix='h4-online-check-') as tmp:
  p=Path(tmp)/'unit';shutil.copytree(sample,p);i=load(p/'execution.json');original=Path(i['node_log']);log=Path(tmp)/'empty-node.log';log.write_text('');i['node_log']=str(log);i['instrumentation']='on';(p/'execution.json').write_text(json.dumps(i));assert check(p)['status']!='PASS';results.append({'case':'missing live execution span','status':'PASS'})
 return results

def pilot_gate(frozen,paths):
 f=load(frozen);identity(f);reports=[];modes=set();sample=None
 for p in paths:
  m,rows=recheck_run(p,'pilot');assert m['node_mode'] not in modes;modes.add(m['node_mode']);assert m['planned_samples']==18
  samples=[(u,c) for _,u,c in rows if u['kind']=='sample'];seen={(u['baseline'],u['operation'],u['repetition']) for u,c in samples};expected={(b,o,n) for b in ['B1','B2','G3'] for o in ['TransferMeta','IssueRight','TransferRight'] for n in [1,2]};assert seen==expected and len(samples)==18
  assert m['planned_warmups']==(18 if m['node_mode']=='reused' else 0)
  assert all(u['instrumentation']==('on' if u['repetition']==1 else 'off') for u,c in samples)
  sample=next(p for p,i,c in rows if i['instrumentation']=='on')
  reports.append({'run_id':m['run_id'],'node_mode':m['node_mode'],'manifest':ref(Path(p)/'manifest.terminal.json'),'samples':18,'warmups':m['planned_warmups'],'status':'PASS','online_matched_transactions':sum(len(c['online_execution']['matches']) for _,i,c in rows if i['instrumentation']=='on')})
 assert modes=={'fresh','reused'}
 tests=negatives(sample);identity(f)
 # E7 is correctness evidence, not latency data.
 e7=ROOT/'reports/e7-v2-formal-independent-recheck-20260921.json';e=load(e7);assert e['passed']==240 and e['failed']==0 and e['identity']=='PASS'
 j={'pilot_ready':True,'calibration_ready':False,'created_at':now(),'freeze':ref(frozen),'pilot_reports':reports,'negative_selftests':tests,'e7_correctness':ref(e7),'formal_main_samples':270,'formal_main_warmups':90,'calibration_samples':90,'calibration_warmups':90,'scope':f['timing_scope']}
 dump(GATE,j);return j

def calibration_gate(path):
 g=load(GATE);identity(load(g['freeze']['path']));m,rows=recheck_run(path,'calibration');assert m['planned_samples']==90 and m['planned_warmups']==90 and len(m['sessions'])==30
 pairs={}
 for p,i,c in rows:
  if i['kind']=='sample':
   key=(i['baseline'],i['operation'],i['repetition']);assert i['instrumentation'] not in pairs.setdefault(key,{});pairs[key][i['instrumentation']]=(p,i,c)
 assert len(pairs)==45
 for key,variants in pairs.items():
  assert set(variants)=={'on','off'};off,on=variants['off'],variants['on'];assert off[1]['fixture_id']==on[1]['fixture_id']
  # Pair business inputs; dynamic expiry/block fields stay explicitly observed, not fabricated.
  a=load(off[0]/'canonical-order.json')['order'];b=load(on[0]/'canonical-order.json')['order']
  for field in ['operation','price','currency','sellerStateVersion','orderNonce','objectType']:assert a[field]==b[field],('paired fixture mismatch',key,field)
  pa=load(off[0]/'pre.json')['business'];pb=load(on[0]/'pre.json')['business'];assert pa['market']['active_orders']==pb['market']['active_orders']==0
 from process import calibration_summary
 summary=calibration_summary(rows);dest=ROOT/'results/processed'/(m['run_id']+'-calibration.json');dump(dest,summary)
 g['calibration_ready']=True;g['calibration_manifest']=ref(Path(path)/'manifest.terminal.json');g['calibration_summary']=ref(dest);g['calibration_pairs']=45
 GATE.rename(GATE.with_name('h4-v1-gate-before-calibration-'+datetime.datetime.now().strftime('%Y%m%dT%H%M%S')+'.json'));dump(GATE,g);return g

def validate_formal(mode):
 g=load(GATE);assert g['pilot_ready'];assert sha(g['freeze']['path'])==g['freeze']['sha256'];identity(load(g['freeze']['path']))
 if mode=='formal':assert g['calibration_ready'],'calibration not complete';assert sha(g['calibration_summary']['path'])==g['calibration_summary']['sha256']
 return g
if __name__=='__main__':
 if sys.argv[1]=='freeze':print(freeze(sys.argv[2]))
 elif sys.argv[1]=='pilot':print(json.dumps(pilot_gate(sys.argv[2],sys.argv[3:])))
 elif sys.argv[1]=='calibration':print(json.dumps(calibration_gate(sys.argv[2])))
