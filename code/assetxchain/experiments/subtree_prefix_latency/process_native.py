import csv,json,math,random,statistics
from pathlib import Path

def quantiles(values):
    values=sorted(values)
    return {"mean_ns_per_tx":statistics.fmean(values),"median_ns_per_tx":statistics.median(values),
      "p95_ns_per_tx":values[min(len(values)-1,math.ceil(.95*len(values))-1)],
      "stdev_ns_per_tx":statistics.stdev(values) if len(values)>1 else 0.0,
      "min_ns_per_tx":min(values),"max_ns_per_tx":max(values)}

def bootstrap(values,seed,rounds=10000):
    rng=random.Random(seed); n=len(values)
    means=sorted(statistics.fmean(values[rng.randrange(n)] for _ in range(n)) for _ in range(rounds))
    return [means[int(.025*rounds)],means[int(.975*rounds)]]

def permutation(values,seed,rounds=20000):
    rng=random.Random(seed); observed=statistics.fmean(values); ge=0
    for _ in range(rounds):
        if statistics.fmean(x if rng.getrandbits(1) else -x for x in values)>=observed: ge+=1
    return (ge+1)/(rounds+1)

def main():
    here=Path(__file__).resolve().parent
    rows=list(csv.DictReader((here/"results/native_full.csv").open()))
    for row in rows:
        row["batch"]=int(row["batch"]); row["position"]=int(row["position"])
        row["ns_per_tx"]=int(row["elapsed_ns"])/int(row["transactions"])
        row["proof_bytes_mean"]=float(row["proof_bytes_mean"]); row["proof_nodes_mean"]=float(row["proof_nodes_mean"])
    summary={"schema_version":"subtree-prefix-native-v1","source":"results/native_full.csv",
      "config":{"records":int(rows[0]["n"]),"rights_per_asset":4,"seed":int(rows[0]["seed"]),
        "batches":max(r["batch"] for r in rows)+1,"batch_size":256,"modes":["single_prefix","dual_prefix","single_hash","dual_hash"],
        "workloads":["meta","right","both"],"metrics":["proof","lookup"]},"groups":{}}
    for work in ["meta","right","both"]:
        for metric in ["proof","lookup"]:
            sub=[r for r in rows if r["work"]==work and r["metric"]==metric]
            g={}
            for mode in ["single_prefix","dual_prefix","single_hash","dual_hash"]:
                vals=[r["ns_per_tx"] for r in sub if r["mode"]==mode]
                mr=[r for r in sub if r["mode"]==mode]
                g[mode]={**quantiles(vals),"mean_proof_nodes":statistics.fmean(r["proof_nodes_mean"] for r in mr),
                         "mean_proof_bytes":statistics.fmean(r["proof_bytes_mean"] for r in mr)}
            for suffix in ["prefix","hash"]:
                sm="single_"+suffix; dm="dual_"+suffix
                s=[r for r in sub if r["mode"]==sm]; d=[r for r in sub if r["mode"]==dm]
                sc={r["batch"]:r["ns_per_tx"] for r in s}; dc={r["batch"]:r["ns_per_tx"] for r in d}
                cycle_diffs=[statistics.fmean(dc[b] for b in sorted(dc) if b//4==cycle)-statistics.fmean(sc[b] for b in sorted(sc) if b//4==cycle) for cycle in range(max(r["batch"] for r in sub)//4+1)]
                # Positive value below means single-tree time minus dual-tree time.
                cycle_diffs=[-x for x in cycle_diffs]
                mean_diff=statistics.fmean(cycle_diffs)
                g["comparison_"+suffix]={"single_mean_ns_per_tx":g[sm]["mean_ns_per_tx"],
                   "dual_mean_ns_per_tx":g[dm]["mean_ns_per_tx"],
                   "speedup_single_over_dual":g[sm]["mean_ns_per_tx"]/g[dm]["mean_ns_per_tx"],
                   "mean_difference_single_minus_dual_ns":mean_diff,
                   "cycle_differences_single_minus_dual_ns":cycle_diffs,
                   "bootstrap_95ci_ns":bootstrap(cycle_diffs,20260923),
                   "one_sided_permutation_p_dual_faster":permutation(cycle_diffs,20260923)}
            summary["groups"][work+"/"+metric]=g
    (here/"results/native_summary.json").write_text(json.dumps(summary,indent=2)+"\n")
    print(json.dumps(summary,indent=2))
if __name__=="__main__": main()


