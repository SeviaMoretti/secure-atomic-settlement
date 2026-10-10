"""Build/test isolated H6 contract; preserve normal production artifact identity."""
import json, os, subprocess, hashlib, shutil, datetime, sys
from pathlib import Path
ROOT=Path('/home/gz785/code/exp2')
REPO=Path('/home/gz785/code/assetxchain-h6-g3')
OUT=ROOT/'build/h6-v1-contracts'
def sha(p): return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def main():
    OUT.mkdir(parents=True,exist_ok=True)
    env=os.environ.copy();env['PATH']='/home/gz785/.cargo/bin:'+env['PATH'];env['RUSTUP_TOOLCHAIN']='1.85.0'
    env['CARGO_TARGET_DIR']='/home/gz785/code/assetxchain-g3-v2/target-s212'
    for name,args in ([] if '--record-only' in sys.argv else [('normal-tests',['test','-p','market_orderbook','--lib']),('fault-tests',['test','-p','market_orderbook','--lib','--features','h6-fault-injection'])]):
        with (OUT/(name+'.log')).open('w') as log:
            subprocess.run(['cargo',*args],cwd=REPO,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
    for mode,features in ([] if '--record-only' in sys.argv else [('normal',[]),('fault',['--features','h6-fault-injection'])]):
        env['CARGO_TARGET_DIR']=str(OUT/(mode+'-target'))
        args=['cargo','contract','build','--release','--manifest-path',str(REPO/'contracts/market_orderbook/Cargo.toml'),*features]
        with (OUT/(mode+'-build.log')).open('w') as log:
            subprocess.run(args,cwd=REPO,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
        files=list((OUT/(mode+'-target')).rglob('market_orderbook.contract'))
        if not files: files=list((REPO/'target').rglob('market_orderbook.contract'))
        assert len(files)==1,files
        dest=OUT/mode;dest.mkdir(exist_ok=True)
        for ext in ['wasm','json','contract']:shutil.copy2(files[0].with_suffix('.'+ext),dest/('market_orderbook.'+ext))
    labels=lambda mode:{x['label'] for x in json.loads((OUT/mode/'market_orderbook.json').read_text())['spec']['messages']}
    assert labels('fault')-labels('normal')=={'h6_corrupt_projection'}
    assert not labels('normal')-labels('fault')
    normal=json.loads((OUT/'normal/market_orderbook.json').read_text());fault=json.loads((OUT/'fault/market_orderbook.json').read_text())
    original=json.loads((ROOT/'reports/r3/x1-e0-v1/g3-control.json').read_text())
    for name,n in [('normal-tests',11),('fault-tests',12)]:assert f'test result: ok. {n} passed; 0 failed' in (OUT/(name+'.log')).read_text()
    data={'created_at':datetime.datetime.now().astimezone().isoformat(),'worktree':str(REPO),'commit':subprocess.check_output(['git','-C',str(REPO),'rev-parse','HEAD'],text=True).strip(),'dirty':bool(subprocess.check_output(['git','-C',str(REPO),'status','--porcelain'],text=True).strip()),'abi_added':['h6_corrupt_projection'],'normal_wasm_matches_control':sha(OUT/'normal/market_orderbook.wasm')==original['artifacts']['contract_wasm']['sha256'],'normal_artifact_policy':'F0/F1/F2 use original frozen control; rebuilt normal retained for ABI isolation and diagnostics; same source does not imply byte-identical build','frozen_control':original,'artifacts':{mode:{ext:{'path':str(OUT/mode/('market_orderbook.'+ext)),'sha256':sha(OUT/mode/('market_orderbook.'+ext))} for ext in ['wasm','json','contract']} for mode in ['normal','fault']},'test_logs':{name:sha(OUT/(name+'.log')) for name in ['normal-tests','fault-tests']}}
    (OUT/'provenance.json').write_text(json.dumps(data,indent=2)+'\n')
    print(json.dumps(data),flush=True)
if __name__=='__main__':main()
