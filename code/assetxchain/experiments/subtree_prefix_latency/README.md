# AssetXChain 子树/前缀验证时延实验

本实验位于 /home/gz785/code/assetxchain/experiments/subtree_prefix_latency，不加入主 Cargo workspace，也不修改 Runtime、pallet 或已有实验目录。

它使用项目当前依赖的 Substrate sp-trie（LayoutV1<Blake2Hasher>）构造真实 Patricia Trie，并比较：

- single_prefix：元证和权证放在一棵 Trie；元证键为 assets/<asset_id>，权证键为项目当前的 <asset_id><certificate_id>；
- dual_prefix：元证和权证分别放在两棵 Trie，使用与项目相同的键规则；
- single_hash/dual_hash：将权证键替换为项目已有的 hash-scattered 选项，作为敏感性对照。

每个 workload 使用相同的资产、权证、值、随机种子和查询。both 每笔查询同时验证一条元证和一条权证；proof 测量 sp_trie::verify_trie_proof，lookup 测量 Trie 数据读取。建树和证明生成在计时区间外。每 4 个 batch 构成一个配对周期，旋转并反转执行顺序，以平衡位置效应。所有合法证明都必须通过；篡改 value、root、proof 或 key 的检查必须拒绝。

通过 PowerShell 调用 WSL 运行正式批次：

wsl.exe -- bash -lc 'cd /home/gz785/code/assetxchain/experiments/subtree_prefix_latency/native && cargo build --release && cd .. && native/target/release/assetxchain-trie-latency 4096 4 20260923 80 256 > results/native_full.csv && python3 process_native.py'

参数依次是：元证记录数、每个资产的权证数、固定 seed、batch 数、每个 batch 的查询数。当前结果使用 N=4096、K=4、80×256；每种布局每种 workload/metric 有 20 个配对周期。

结果文件：

- results/native_full.csv：不可修改的逐 batch 原始输出；
- results/native_summary.json：由原始 CSV 重新生成的统计；
- results/run_metadata.json：commit、WSL、Rust、Cargo 和源文件哈希；
- results/native_smoke.csv：较小的 smoke 输出；
- native/src/main.rs：实验源代码；
- process_native.py：统计处理脚本。

本实验的结论范围是 Trie 证明验证和本地 Trie lookup 的 CPU 时延。它不测 RPC 提交、Runtime dispatch、数据库 I/O、交易入块或最终性；因此不能直接写成“链上交易端到端时延一定更低”。
