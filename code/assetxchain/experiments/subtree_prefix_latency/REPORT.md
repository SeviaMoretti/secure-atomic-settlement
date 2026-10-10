# AssetXChain 双子树+前缀与单子树实验报告

实验目录：/home/gz785/code/assetxchain/experiments/subtree_prefix_latency  
代码版本：cf3859eeadfc7286a1f0656a148a9fa245f82605  
配置：N=4096 个元证记录，K=4 个权证/资产，seed=20260923，80 个 batch，每 batch 256 次查询；每 4 个 batch 为一个配对周期，共 20 个周期。

实验使用项目实际 Substrate sp-trie LayoutV1<Blake2Hasher>。single_prefix 将元证与权证放入同一 Trie；dual_prefix 使用独立的元证 Trie 和权证 Trie。两者采用项目当前键规则：元证为 assets/<asset_id>，权证为 <asset_id><certificate_id>。每个 workload 的记录、值、查询序列相同。所有合法证明、Trie lookup 和四类篡改输入均通过正确性门禁。

## 主要 workload：both

每笔查询验证一条元证和一条权证。

| 指标 | single_prefix | dual_prefix | 双子树相对结果 |
|---|---:|---:|---:|
| proof 平均 CPU 时延 | 11.348 µs/笔 | 9.556 µs/笔 | 1.187×，降低 15.79% |
| proof 中位数 | 11.200 µs/笔 | 9.481 µs/笔 | — |
| proof P95 | 13.378 µs/笔 | 11.523 µs/笔 | — |
| lookup 平均 CPU 时延 | 1.895 µs/笔 | 1.475 µs/笔 | 1.285×，降低 22.15% |
| proof 平均节点数 | 13.47 | 10.56 | — |

配对 proof 差值（单子树减双子树）为 1.792 µs/笔，bootstrap 95% CI 为 [1.265, 2.229] µs/笔；lookup 差值为 0.420 µs/笔，bootstrap 95% CI 为 [0.363, 0.475] µs/笔。两项的单侧配对符号置换 p 值均为 0.00005（20 个配对周期，随机重复 20,000 次）。

## 分对象结果

元证 proof：single_prefix 7.795 µs/笔，dual_prefix 4.563 µs/笔，速度比 1.708×，差值 CI [3.102, 3.359] µs/笔。

权证 proof：single_prefix 5.522 µs/笔，dual_prefix 5.447 µs/笔，速度比 1.014×，差值 CI [-0.150, 0.308] µs/笔。该区间跨过 0，说明在当前 K=4 的权证单对象查询中没有足够证据断言双子树更快。

因此，整体 both 结果的优势主要来自元证树规模被独立出来后证明路径变短；不能把优势归因于“前缀字符串本身”。hash-scattered 对照用于显示键布局敏感性，正式主比较是 single_prefix 与 dual_prefix。

## 证据边界

结果支持以下局部结论：在当前 AssetXChain 键规则、N=4096、K=4 和本地 Substrate Patricia Trie 实现下，同时验证元证和权证时，双子树布局的本地证明验证与 Trie lookup CPU 时延低于单子树布局。

结果不测 RPC 提交、Runtime dispatch、数据库 I/O、交易入块、确认或最终性，因此不应直接表述为端到端交易时延已经降低。原始逐 batch 数据在 results/native_full.csv，统计结果在 results/native_summary.json；两者均由 WSL 命令生成。
