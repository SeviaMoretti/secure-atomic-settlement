#!/usr/bin/env python3
"""Generate a Chinese report from the verified live-chain dataset."""
import argparse,json,statistics
from pathlib import Path
def main():
 p=argparse.ArgumentParser();p.add_argument("run_dir");args=p.parse_args()
 root=Path(args.run_dir);s=json.loads((root/"analysis.json").read_text());m=json.loads((root/"manifest.json").read_text())
 assert s["status"]=="PASS"
 groups={(x["variant"],x["noise"]):x for x in s["groups"]}
 lines=["# 在线噪音与状态组织实验","",
 "本报告来自 AssetXChain 实际节点首次入块时延；离线 Trie proof/lookup 数字不参与此结论。",
 "",f"批次：{root.name}；源版本：{m['commit']}；独立临时链会话：{len(s['records'])}；每组 {s['repetitions']} 次。",
 "",
 "| 背景交易数 N | 单树均值 ms | 双树均值 ms | 单树噪音增量 ms | 双树噪音增量 ms |",
 "|---:|---:|---:|---:|---:|"]
 for n in m["levels"]:
  a=groups["single_tree",n]["mean_ms"];b=groups["dual_prefix",n]["mean_ms"]
  lines.append(f"| {n} | {a:.3f} | {b:.3f} | {a-groups['single_tree',0]['mean_ms']:.3f} | {b-groups['dual_prefix',0]['mean_ms']:.3f} |")
 lines+=["","D=(单树有噪音−单树零噪音)−(双树有噪音−双树零噪音)。D>0 才表示双树噪音增量较小。区间按独立会话配对重采样，不把背景交易当作独立探针。","",
 "| N | D ms | 配对 bootstrap 95% CI ms | 单侧精确置换 p | 两档 Bonferroni p |",
 "|---:|---:|---|---:|---:|"]
 for e in s["effects"]:
  lo,hi=e["bootstrap_95ci_ms"]
  lines.append(f"| {e['noise']} | {e['interaction_ms']:.3f} | [{lo:.3f}, {hi:.3f}] | {e['one_sided_sign_permutation_p']:.4f} | {e['bonferroni_one_sided_p']:.4f} |")
 supported=[e for e in s["effects"] if e["direction_supported_in_this_sample"] and e["bonferroni_one_sided_p"]<.05]
 lines+=["","## 结论","",
  ("部分负载支持双树降低噪音增量，具体见上表；不能推广为任意噪音或交易类型。" if supported else
   "本次受控在线实验没有确证“双树比单树降低噪音对交易时延的影响”。全部样本、均值、增量和区间均如实保留；个别绝对均值较小或离线微基准加速不能替代抗噪音证据。"),
  "","## 负载与门禁","",
  f"目标交易成功 {len(s['records'])}/{len(s['records'])}；背景交易 attempted / RPC accepted / included / business success 均为 {sum(r['accepted'] for r in s['records']):,}。",
  "门禁逐笔核对签名交易 Blake2 哈希、块内索引、ExtrinsicSuccess、登记/转移业务事件及最终权证持有人。childstate_getStorage 确认真实单树/双树布局；两种运行时 :code 不同且各组固定；前缀模式和正常权重均开启。",
  "所有噪音在目标提交前已被 RPC 接收；目标发送相位为收到锚定头后 4000±100 ms。计时在首个 isInBlock 回调入口截止，后续补充查询不计时。",
  "","## 范围","",
  "每个条件是全新 --tmp 链、一个 transferCertificate 探针；背景是 6 个账户发送 registerAssetCore（项目已有免抵押路径）。0、512、2048 表示突发数量，不是 TPS。相同种子匹配逻辑输入，但资产 ID 含链上时间戳，不声称原始状态根一致。",
  "单树仅把 certificate_trie_info 指向资产 ChildInfo；键规则、权重、tip、业务验证、BABE 参数不变。目标同时读元证和权证，共识、CPU、交易池、区块容量与全局状态仍共享。分树并不自动提供全链负载隔离。",
  "指标包含等待出块、整块制作/执行与 RPC 通知，不是纯交易验证函数时延或最终性。每组只有 5 个独立探针，区间是探索性估计，不能报告稳定 p95/p99。初始业务状态小，不覆盖十万历史状态或持续高 TPS。",
  "",
  "客户端头到达诊断：本批次有 "+str(s["timing_diagnostic"]["short_one_block_intervals"])+" 个相邻目标块的头到达间隔短于 5500 ms（名义 slot 为 6000 ms）。固定 4000 ms 相位是相对于收到锚定头，不等于固定绝对时隙相位。几次带噪音时延反而更短，提示时隙/通知抖动尚未完全消除；此来源没有进一步分解，不据此宣称单树具有反向优势。",
  "","## 块内证据","",
  "| 布局 | N | 平均同块背景数 | 平均目标索引 | 平均整块 proposal ms |",
  "|---|---:|---:|---:|---:|"]
 for n in m["levels"]:
  for v in ["single_tree","dual_prefix"]:
   rr=[r for r in s["records"] if r["noise"]==n and r["variant"]==v]
   pp=[r["target_block_proposal_ms"] for r in rr if r["target_block_proposal_ms"] is not None]
   lines.append(f"| {v} | {n} | {statistics.fmean(r['noise_same_block'] for r in rr):.1f} | {statistics.fmean(r['target_index'] for r in rr):.1f} | {statistics.fmean(pp) if pp else float('nan'):.1f} |")
 lines+=["","目标索引按节点真实排序记录；背景先提交不等于先执行。整块 proposal 不能解释为目标独立执行时间。",
  "","## 复核","",
  "原始单元 JSON 与 node.log/driver.log、manifest.json、source/ 源快照均保留。analysis.json 包含全部样本、源数据与分析器哈希，measurements.csv 为逐会话汇总。正确性门禁不预设性能方向。",
  "",
  "在本实验目录执行：",
  "",
  f"    python3 analyze_controlled.py results/{root.name}",
  "    python3 run_controlled.py --run-id another-online-run --levels 0,512,2048 --repetitions 5",
  "",
  "新批次拒绝覆盖。使用 18944/31333 独立端口，仅终止本程序启动的节点。主仓库跟踪源码无改动；单树补丁保存在 single-tree.patch 和隔离工作树。",
  "旧 v1 目录名中的 formal 不是论文有效性等级；该批次未控制相位，只作诊断。"
 ]
 report="\n".join(lines)+"\n"
 (root/"REPORT.md").write_text(report,encoding="utf-8")
 (root.parent.parent/"REPORT.md").write_text(report,encoding="utf-8")
 print(root/"REPORT.md")
if __name__=="__main__":main()
