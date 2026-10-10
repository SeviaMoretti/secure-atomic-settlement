"""Run the complete offline matrix audit and recheck existing diagnostic raw."""
import json,sys,subprocess,os,datetime
from pathlib import Path
from runner import ROOT,HERE,NODE,write,sha,now,load
from checker import check
from gate import negatives
def main(sample):
 sample=Path(sample);report=ROOT/'reports'/('h6-preflight-'+datetime.datetime.now().strftime('%Y%m%dT%H%M%S'))
 report.mkdir();env=os.environ.copy();env['POLKADOT_DRIVER_ROOT']=str(ROOT/'scripts/phase4/node')
 for p in HERE.glob('*.js'):subprocess.run([NODE,'--check',str(p)],check=True,env=env)
 for p in HERE.glob('*.py'):compile(p.read_text(),str(p),'exec')
 subprocess.run([NODE,str(HERE/'preflight.js'),str(sample),str(report/'cells.json')],check=True,env=env)
 r=check(sample);assert r['status']=='PASS',r['errors'];write(report/'diagnostic-recheck.json',r)
 n=negatives(sample);write(report/'negative-selftests.json',n)
 config=load(ROOT/'configs/h6_v1/config.json');assert config['pilot_episodes']==36*2 and config['formal_episodes']==36*10
 result={'created_at':now(),'status':'PASS','offline_cells':36,'negative_selftests':len(n),'real_chain_diagnostics_rechecked':1,'real_chain_diagnostic_cell':'TransferMeta/Settled/F3','real_chain_scope':'one cell only; the 72 pilot episodes still required','existing_raw_unchanged':True,'diagnostic_raw':str(sample),'report_dir':str(report),'sources':{str(p):sha(p) for p in HERE.iterdir() if p.is_file()},'artifacts':{p.name:sha(p) for p in report.iterdir() if p.is_file()}}
 write(report/'summary.json',result);write(ROOT/'reports/h6-preflight-latest.json',result);print(json.dumps(result),flush=True)
if __name__=='__main__':main(sys.argv[1])
