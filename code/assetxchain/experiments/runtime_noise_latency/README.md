最新“主状态Trie与双child trie、N={1000,2000,5000,10000}、50笔元证probe、R=2”见 [main50/README.md](main50/README.md) 和 [main50/REPORT.md](main50/REPORT.md)。

最新按用户指定的“固定50笔元证probe + 权证噪音、重复2次、N={0,100,500,1000,2000,5000}”协议位于 [meta50/README.md](meta50/README.md)，结果见 [meta50/REPORT.md](meta50/REPORT.md)。本页旧协议的probe/噪音业务角色相反，不用于这次结论。

# 运行链上的状态组织与噪音时延对照

这是在线节点实验。主程序是 condition_v2.js + run_controlled.py；此前 condition.js/run_matrix.sh 的结果只作诊断，不用于抗噪音结论。subtree_prefix_latency 是另一个离线微基准，不能回答本实验问题。

## 比较对象

- dual_prefix：基于 cf3859eeadfc7286a1f0656a148a9fa245f82605 的原始 AssetXChain，元证和权证分别在 :asset_trie:、:certificate_trie:。
- single_tree：同一源版本，仅把 certificate_trie_info() 返回的 ChildInfo 改成 :asset_trie:；补丁是 single-tree.patch。元证键 assets/asset_id、权证键 asset_id||certificate_id 保持一致，隔离考察分树这一变量。
- 使用两者正常的 weight、相同 BABE 出块设置、0 tip。没有开启零权重、修改区块容量、加速共识或给基线插入额外计算。共享树会影响两个根的语义，这是该组织方式的直接结果。

## 测量协议

目标交易：Bob 将一张权证转给 Charlie（transferCertificate），会读取元证和权证并更新权证。噪音：6 个独立签名账户发送 registerAssetCore，写元证状态及全局计数。它是项目已有的免抵押登记路径；这里的噪音是无关业务写入，不是恶意失败请求。

矩阵是 2 种布局 × N={0,512,2048} × 5 次全新链 = 30 个节点会话、30 笔目标交易、25,600 笔背景交易（不含建链准备交易）。每个单元只有一个探针；每一轮交替运行布局的先后顺序。相同种子使用相同逻辑输入；资产 ID 因链上时间戳而不同，不声称状态哈希完全相等。

准备好目标与背景交易的签名后，等待一个新区块头：收到头后 200 ms 开始背景提交，4000 ms 提交目标。背景走独立 WebSocket 的 author_submitExtrinsic，无逐交易订阅；目标使用单个订阅。计时从已签名目标发送前开始，在首次 isInBlock 回调入口停止，之后的区块/事件查询均不计时。使用新端口 18944/31333、--tmp 状态，仅终止本进程启动的节点。

门禁要求所有配置噪音均已在目标提交前被 RPC 接收，并最终业务成功入块。保存每笔签名串、哈希、nonce、接收时刻、所在区块/索引、业务事件。保存节点日志、运行时 :code 哈希、二进制 SHA256、源码快照与配置。运行后读取 child storage 验证真实单树/双树布局，并核对权证最终持有人。

## 判断噪音影响

计算 Δ_single(N)=L_single(N)-L_single(0)、Δ_dual(N)=L_dual(N)-L_dual(0)，以及 D(N)=Δ_single(N)-Δ_dual(N)。D>0 才表示双树的噪音增量较小。按 5 对独立链会话计算 bootstrap 区间与符号置换检验；两档比较额外报告 Bonferroni 校正 p。正确性门禁没有预设性能方向。5 次不足以给出稳定 p95/p99，报告样本、均值、范围与配对差值。

## 运行

在 PowerShell 中执行（run-id 必须新建，已存在会拒绝覆盖）：

```powershell
wsl.exe -- bash -lc 'cd /home/gz785/code/assetxchain/experiments/runtime_noise_latency && python3 run_controlled.py --run-id my-online-noise-run --levels 0,512,2048 --repetitions 5'
wsl.exe -- bash -lc 'cd /home/gz785/code/assetxchain/experiments/runtime_noise_latency && python3 analyze_controlled.py results/my-online-noise-run'
```

当前完成批次见 results/controlled-r5-20260923；REPORT.md 给出最终审计后的结论。分析脚本只写派生 analysis.json/measurements.csv，不改原始单元 JSON/日志。源码快照位于每批结果的 source/；分析版本哈希写入 analysis.json。

依赖采用本机已有 /home/gz785/code/exp2/scripts/phase4/node/package.json 的 @polkadot/api，也可用 DRIVER_PACKAGE 指向另一个兼容安装。为本实验保存的两份节点副本位于 artifacts/，不依赖其他实验后来重建的二进制。

需要从固定源版本重建时：

```powershell
wsl.exe -- bash -lc 'cd /home/gz785/code/assetxchain/experiments/runtime_noise_latency && python3 build_variants.py --build'
```

重建工作树和构建产物都留在本实验目录下的 worktrees/、build/，不会重置主仓库。

## 结论范围

本实验测客户端观察到的首次入块时延，包含等待出块、区块制作/执行和本机 RPC 通知，不是纯 validate_transaction 执行时间或最终性。固定相位以接收到头的时间为准，仍有通知抖动。背景是有限突发，初始业务状态很小；不代表长期高 TPS 或 10^5 以上历史状态规模。

两种布局共享交易池、CPU、区块容量和共识。transferCertificate 还会读取元证，故只隔离其部分状态访问；分树不会自动消除全链拥塞。日志中的整块 proposal 毫秒数只作诊断，不能冒充单笔交易执行时延。

旧 runtime-noise-formal-20260923 目录名中的 formal 不表示论文正式证据：该 v1 脚本未控制提交相位，0..256 档结果应视为诊断；其他 smoke/失败日志也原样保留。
\n审计补充：查看 REPORT.md 的头到达间隔诊断。固定收到头后 4000 ms 不等于固定绝对 BABE slot 相位；当前小样本保留该波动，不能把反向均值差解释为树结构因果优势。\n