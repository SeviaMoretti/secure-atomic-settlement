#!/usr/bin/env python3
"""Independent raw-block consistency gate and session-level noise interaction."""
import argparse,collections,csv,hashlib,itertools,json,math,random,statistics,re
from pathlib import Path
def require(ok,msg):
 if not ok:raise ValueError(msg)
def mean(x):return statistics.fmean(x)
def ci(values,seed=823):
 if len(values)<2:return None
 rng=random.Random(seed);n=len(values)
 vals=sorted(mean(rng.choices(values,k=n)) for _ in range(10000))
 return [vals[250],vals[9749]]
def sign_p(values):
 observed=mean(values)
 return sum(mean(v*s for v,s in zip(values,signs))>=observed-1e-9 for signs in itertools.product([-1,1],repeat=len(values)))/2**len(values)
def analyze(root):
 root=Path(root); manifest=json.loads((root/"manifest.json").read_text())
 reps=manifest["repetitions"];levels=manifest["levels"]
 jobs=manifest["jobs"]
 require(len(jobs)==reps*len(levels)*2,"incomplete matrix")
 cells={};runtime={};setup_methods={};raw_hashes={};records=[]
 for j in jobs:
  file=root/(j["stem"]+".json");raw_hashes[file.name]=hashlib.sha256(file.read_bytes()).hexdigest()
  d=json.loads(file.read_text());n=j["noise"];v=j["variant"];rep=j["repetition"]
  require(j["exit_code"]==0 and d["status"]=="complete",file.name+": run failed")
  require(d["noise_offered"]==n and d["variant"]==v and d["seed"]==manifest["seed_base"]+rep,"cell mismatch")
  require(d["key_mode"] is False and d["zero_weight"] is False,"wrong keys or weights")
  layout=d["layout"]
  require(layout["asset_in_asset"] and layout["certificate_in_asset"]==(v=="single_tree") and layout["certificate_in_right"]==(v=="dual_prefix"),"layout mismatch")
  runtime.setdefault(v,set()).add(d["runtime_code_blake2"])
  require(len(d["noise"])==n and all(x.get("accepted") and x["rpc_hash"]==x["hash"] for x in d["noise"]),"RPC noise coverage")
  require(len({x["hash"] for x in d["noise"]})==n,"duplicate noise hash")
  require(all(x["accepted_ms"]<d["target"]["sent_ms"] for x in d["noise"]),"noise not fully accepted before probe")
  require(abs(d["target"]["phase_ms"]-4000)<100,"target phase outside tolerance")
  anchor=d["anchor"]
  require(all(h["number"]<=anchor["number"] for h in d["heads"] if h["seen_ms"]<d["target"]["sent_ms"]),"phase crossed a block")
  require(d["target"]["observed_ms"]>d["target"]["sent_ms"],"timing order")
  byhash={}
  for b in d["blocks"]:
   for x in b["extrinsics"]:
    require(x["hash"] not in byhash,"duplicate included hash")
    events=[e for e in b["events"] if e["phase"].get("applyExtrinsic")==x["index"]]
    byhash[x["hash"]]=(b,x,events)
  for tx in d["noise"]+[d["target"]]:
   require(tx["hash"]=="0x"+hashlib.blake2b(bytes.fromhex(tx["signed_hex"][2:]),digest_size=32).hexdigest(),"signed extrinsic hash mismatch")
   require(tx["hash"] in byhash,"missing included tx")
   b,x,events=byhash[tx["hash"]]
   require(any(e["section"]=="system" and e["method"]=="ExtrinsicSuccess" for e in events),"extrinsic failure")
   require(not any(e["section"]=="system" and e["method"]=="ExtrinsicFailed" for e in events),"extrinsic failure")
   expected="CertificateTransferred" if tx is d["target"] else "AssetRegistered"
   require(any(e["section"]=="dataAssets" and e["method"]==expected for e in events),"missing business event")
   if tx is d["target"]:
    require(b["hash"]==tx["block_hash"] and b["number"]==tx["inclusion"]["number"],"inclusion mismatch")
    require(x["method"]=="transferCertificate","target method mismatch")
   else: require(x["method"]=="registerAssetCore","noise method mismatch")
  require(d["post_certificate"]["owner"]==d["fixture"]["target_buyer"],"owner mismatch")
  tx=d["target"]; key=(v,n,rep);require(key not in cells,"duplicate cell");cells[key]=tx["latency_ms"]
  setup_methods.setdefault((n,rep),set()).add(d["fixture"]["noise_hashes_digest"])
  checked_same=sum(byhash[t["hash"]][0]["hash"]==tx["block_hash"] for t in d["noise"])
  require(checked_same==d["checks"]["same_block_as_target"],"same-block noise mismatch")
  target_head=next(h for h in d["heads"] if h["hash"]==tx["block_hash"])
  observed_interval=target_head["seen_ms"]-anchor["seen_ms"]
  rec={"target_head_interval_ms":observed_interval,"variant":v,"noise":n,"repetition":rep,"latency_ms":tx["latency_ms"],
   "phase_ms":tx["phase_ms"],"completion_blocks":tx["inclusion"]["number"]-anchor["number"],
   "accepted":n,"included":n,"noise_same_block":checked_same,
   "target_index":tx["inclusion"]["index"],"target_block_extrinsics":tx["inclusion"]["block_extrinsic_count"],
   "raw":file.name}
  log=(root/(j["stem"]+".node.log")).read_text()
  log=re.sub(r"\x1b\[[0-9;]*m","",log)
  timings=dict((int(b),int(t)) for b,t in re.findall(r"Prepared block for proposing at (\d+) \((\d+) ms\)",log))
  rec["target_block_proposal_ms"]=timings.get(tx["inclusion"]["number"])
  records.append(rec)
 for v,codes in runtime.items():require(len(codes)==1,"runtime code changed within variant")
 require(runtime["dual_prefix"]!=runtime["single_tree"],"identical runtime code in variants")
 require(all(len(x)==1 for x in setup_methods.values()),"logical noise payloads differ between layouts")
 require(len(cells)==reps*len(levels)*2,"missing cells")
 groups=[]
 for n in levels:
  for v in ["single_tree","dual_prefix"]:
   vals=[cells[v,n,r] for r in range(reps)]
   groups.append({"noise":n,"variant":v,"mean_ms":mean(vals),"median_ms":statistics.median(vals),"min_ms":min(vals),"max_ms":max(vals),"samples":vals})
 effects=[]
 for n in levels:
  if n==0:continue
  single=[cells["single_tree",n,r]-cells["single_tree",0,r] for r in range(reps)]
  dual=[cells["dual_prefix",n,r]-cells["dual_prefix",0,r] for r in range(reps)]
  interaction=[s-d for s,d in zip(single,dual)]
  interval=ci(interaction)
  effects.append({"noise":n,"single_increment_ms":mean(single),"dual_increment_ms":mean(dual),
   "interaction_ms":mean(interaction),"interaction_samples":interaction,"bootstrap_95ci_ms":interval,
   "one_sided_sign_permutation_p":sign_p(interaction),
   "direction_supported_in_this_sample":interval is not None and interval[0]>0 and mean(single)>0,
   "bonferroni_one_sided_p":min(1,sign_p(interaction)*(len(levels)-1))})
 summary={"status":"PASS","schema":"runtime-noise-analysis-v2","repetitions":reps,"groups":groups,
  "interaction_definition":"(single noisy - single zero) - (dual noisy - dual zero); positive favors dual",
  "effects":effects,"records":records,"source_hashes":raw_hashes,
  "analyzer_sha256":hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
  "timing_diagnostic":{"target_head_interval_ms":[r["target_head_interval_ms"] for r in records],
   "short_one_block_intervals":sum(r["completion_blocks"]==1 and r["target_head_interval_ms"]<5500 for r in records),
   "interpretation":"Some received-head intervals can be shorter than the nominal 6000 ms slot. Keep these observations, do not attribute reversed noise effects to tree structure."},
  "runtime_code_hashes":{k:list(v)[0] for k,v in runtime.items()},
  "limits":["Each cell is one independent fresh dev chain and one probe. R=5 cannot estimate tail latency reliably.",
   "Fixed phase is relative to received head; includes local WebSocket observation delay.",
   "Noise is a finite presigned registration burst, not sustained TPS or large historical state.",
   "Probe transfers a certificate and reads both asset and certificate state; only part of its access is isolated.",
   "Common global state, execution CPU, transaction pool, block space, root computation and consensus are shared.",
   "No one-sided hypothesis outcome is required for the correctness gate."]}
 (root/"analysis.json").write_text(json.dumps(summary,indent=2)+"\n")
 with (root/"measurements.csv").open("w",newline="") as f:
  w=csv.DictWriter(f,fieldnames=records[0].keys());w.writeheader();w.writerows(records)
 return summary
if __name__=="__main__":
 p=argparse.ArgumentParser();p.add_argument("run_dir");a=p.parse_args()
 s=analyze(a.run_dir);print(json.dumps({k:s[k] for k in ["status","repetitions","groups","effects"]},indent=2))
