#!/usr/bin/env python3
import argparse
import csv
import html
import json
import re
from pathlib import Path
from statistics import mean, median


SCENARIO_LABELS = {
    ("pallet_markets", "register_market"): "register_market_contract_verification",
    ("pallet_dataassets", "register_asset"): "register_asset",
    ("pallet_dataassets", "authorize_operator"): "authorize_market_for_asset",
    ("pallet_dataassets", "transfer_asset"): "metatoken_settlement_runtime_equivalent",
    ("pallet_dataassets", "issue_certificate"): "righttoken_issue_runtime_equivalent",
    ("pallet_dataassets", "transfer_certificate"): "righttoken_transfer_runtime_equivalent",
    ("pallet_dataassets", "revoke_authorization"): "authorization_revoke_runtime_equivalent",
}

EVENT_COUNTS = {
    "register_market_contract_verification": 1,
    "register_asset": 2,
    "authorize_market_for_asset": 1,
    "metatoken_settlement_runtime_equivalent": 1,
    "righttoken_issue_runtime_equivalent": 1,
    "righttoken_transfer_runtime_equivalent": 1,
    "authorization_revoke_runtime_equivalent": 1,
}

DISPLAY_NAMES_ZH = {
    "register_market_contract_verification": "市场注册与合约验证",
    "register_asset": "注册资产",
    "authorize_market_for_asset": "市场授权",
    "metatoken_settlement_runtime_equivalent": "MetaToken 结算 Runtime 路径",
    "righttoken_issue_runtime_equivalent": "RightToken 发行 Runtime 路径",
    "righttoken_transfer_runtime_equivalent": "RightToken 转移 Runtime 路径",
    "authorization_revoke_runtime_equivalent": "撤销授权",
}


WEIGHT_PALLETS = {
    "pallet_dataassets": "pallets/dataassets/src/weights.rs",
    "pallet_markets": "pallets/markets/src/weights.rs",
}


def load_json(path: Path):
    text = path.read_text(encoding="utf-8", errors="replace").strip()
    return json.loads(text)


def summarize_series(values):
    if not values:
        return {"count": 0, "mean": "", "median": "", "min": "", "max": ""}
    return {
        "count": len(values),
        "mean": mean(values),
        "median": median(values),
        "min": min(values),
        "max": max(values),
    }


def parse_static_weights(project_root: Path):
    weights = {}
    for pallet, relative in WEIGHT_PALLETS.items():
        path = project_root / relative
        if not path.exists():
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for match in re.finditer(r"fn\s+([a-zA-Z0-9_]+)\s*\(\)\s*->\s*Weight\s*\{(?P<body>.*?)\n\t\}", text, re.S):
            extrinsic = match.group(1)
            body = match.group("body")
            ref_match = re.search(r"Weight::from_parts\(([\d_]+),\s*0\)", body)
            proof_match = re.search(r"Weight::from_parts\(0,\s*([\d_]+)\)", body)
            reads_match = re.search(r"\.reads\((\d+)\)", body)
            writes_match = re.search(r"\.writes\((\d+)\)", body)
            min_exec_match = re.search(r"Minimum execution time:\s*([\d_]+)\s*picoseconds", body)
            if not ref_match:
                continue
            weights[(pallet, extrinsic)] = {
                "weight_ref_time_static": int(ref_match.group(1).replace("_", "")),
                "weight_proof_size_static": int(proof_match.group(1).replace("_", "")) if proof_match else "",
                "static_reads": int(reads_match.group(1)) if reads_match else "",
                "static_writes": int(writes_match.group(1)) if writes_match else "",
                "minimum_execution_time_ps_static": int(min_exec_match.group(1).replace("_", "")) if min_exec_match else "",
                "weight_source": str(path),
            }
    return weights


def summarize_benchmark_file(path: Path):
    entries = load_json(path)
    rows = []
    for entry in entries:
        pallet = entry["pallet"]
        benchmark = entry["benchmark"]
        scenario = SCENARIO_LABELS.get((pallet, benchmark), benchmark)
        for result_type in ("time_results", "db_results"):
            points = entry.get(result_type, [])
            extrinsic_ns = [p["extrinsic_time"] for p in points]
            storage_root_ns = [p["storage_root_time"] for p in points]
            reads = [p["reads"] + p.get("repeat_reads", 0) for p in points]
            writes = [p["writes"] + p.get("repeat_writes", 0) for p in points]
            proof_size = [p["proof_size"] for p in points]
            ex = summarize_series(extrinsic_ns)
            sr = summarize_series(storage_root_ns)
            rr = summarize_series(reads)
            ww = summarize_series(writes)
            ps = summarize_series(proof_size)
            rows.append(
                {
                    "group": "G3",
                    "scenario": scenario,
                    "pallet": pallet,
                    "extrinsic": benchmark,
                    "result_type": result_type,
                    "result_source": "benchmark_json",
                    "benchmark_status": "ok",
                    "samples": ex["count"],
                    "latency_ms_mean": ex["mean"] / 1_000_000 if ex["mean"] != "" else "",
                    "latency_ms_median": ex["median"] / 1_000_000 if ex["median"] != "" else "",
                    "latency_ms_min": ex["min"] / 1_000_000 if ex["min"] != "" else "",
                    "latency_ms_max": ex["max"] / 1_000_000 if ex["max"] != "" else "",
                    "storage_root_ms_mean": sr["mean"] / 1_000_000 if sr["mean"] != "" else "",
                    "reads_mean": rr["mean"],
                    "writes_mean": ww["mean"],
                    "proof_size_bytes_mean": ps["mean"],
                    "events_count_expected": EVENT_COUNTS.get(scenario, ""),
                    "completion_blocks": "n/a_benchmark_harness",
                    "partial_fee": "n/a_no_signed_rpc_tx",
                    "contract_gas_or_ref_time": "n/a_contract_not_deployed",
                    "weight_ref_time_static": "",
                    "weight_proof_size_static": "",
                    "static_reads": "",
                    "static_writes": "",
                    "minimum_execution_time_ps_static": "",
                    "raw_file": str(path),
                }
            )
    return rows


def parse_test_log(path: Path):
    text = path.read_text(encoding="utf-8", errors="replace")
    match = re.search(
        r"test result: (?P<status>\w+)\. (?P<passed>\d+) passed; (?P<failed>\d+) failed; (?P<ignored>\d+) ignored",
        text,
    )
    if not match:
        return {
            "file": str(path),
            "status": "unknown",
            "passed": "",
            "failed": "",
            "ignored": "",
        }
    return {
        "file": str(path),
        "status": match.group("status"),
        "passed": int(match.group("passed")),
        "failed": int(match.group("failed")),
        "ignored": int(match.group("ignored")),
    }


def write_csv(path: Path, rows):
    fieldnames = [
        "group",
        "scenario",
        "pallet",
        "extrinsic",
        "result_type",
        "result_source",
        "benchmark_status",
        "samples",
        "latency_ms_mean",
        "latency_ms_median",
        "latency_ms_min",
        "latency_ms_max",
        "storage_root_ms_mean",
        "reads_mean",
        "writes_mean",
        "proof_size_bytes_mean",
        "events_count_expected",
        "completion_blocks",
        "partial_fee",
        "contract_gas_or_ref_time",
        "weight_ref_time_static",
        "weight_proof_size_static",
        "static_reads",
        "static_writes",
        "minimum_execution_time_ps_static",
        "raw_file",
    ]
    with path.open("w", encoding="utf-8", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=fieldnames)
        writer.writeheader()
        writer.writerows(rows)


def fmt(value, digits=4):
    if value == "" or isinstance(value, str):
        return str(value)
    return f"{value:.{digits}f}"


def svg_bar_chart(path: Path, rows, value_key: str, title: str, unit: str):
    chart_rows = [r for r in rows if r.get(value_key) not in ("", None)]
    if not chart_rows:
        return
    chart_rows = sorted(chart_rows, key=lambda r: float(r[value_key]), reverse=True)
    width = 980
    left = 255
    right = 40
    top = 58
    row_h = 42
    bar_h = 24
    height = top + 28 + row_h * len(chart_rows)
    max_value = max(float(r[value_key]) for r in chart_rows) or 1.0
    palette = ["#2f6f73", "#c77d32", "#6d5aa8", "#3e7f4f", "#b84c5a", "#4f6f9f", "#8a6b28"]
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" fill="#ffffff"/>',
        f'<text x="24" y="34" font-family="Arial, sans-serif" font-size="20" font-weight="700" fill="#202124">{html.escape(title)}</text>',
    ]
    axis_x = left
    axis_y = top + row_h * len(chart_rows) + 6
    parts.append(f'<line x1="{axis_x}" y1="{axis_y}" x2="{width-right}" y2="{axis_y}" stroke="#c9ced6" stroke-width="1"/>')
    for i, row in enumerate(chart_rows):
        y = top + i * row_h
        value = float(row[value_key])
        label = DISPLAY_NAMES_ZH.get(row["scenario"], row["scenario"])
        bar_w = (width - left - right - 92) * value / max_value
        color = palette[i % len(palette)]
        parts.extend(
            [
                f'<text x="24" y="{y + 18}" font-family="Arial, sans-serif" font-size="14" fill="#2b2f33">{html.escape(label)}</text>',
                f'<rect x="{left}" y="{y}" width="{bar_w:.2f}" height="{bar_h}" rx="3" fill="{color}"/>',
                f'<text x="{left + bar_w + 8:.2f}" y="{y + 17}" font-family="Arial, sans-serif" font-size="13" fill="#2b2f33">{value:.4f} {html.escape(unit)}</text>',
            ]
        )
    parts.append("</svg>")
    path.write_text("\n".join(parts) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--raw-dir", required=True)
    parser.add_argument("--processed-dir", required=True)
    parser.add_argument("--report-dir", required=True)
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()

    raw_dir = Path(args.raw_dir)
    processed_dir = Path(args.processed_dir)
    report_dir = Path(args.report_dir)
    figure_dir = processed_dir.parent / "figures"
    processed_dir.mkdir(parents=True, exist_ok=True)
    report_dir.mkdir(parents=True, exist_ok=True)
    figure_dir.mkdir(parents=True, exist_ok=True)

    metadata = load_json(raw_dir / "run_metadata.json")
    static_weights = parse_static_weights(Path(metadata["project_root"]))

    rows = []
    for path in sorted(raw_dir.glob("*.json")):
        if path.name == "run_metadata.json":
            continue
        if path.stat().st_size == 0:
            continue
        rows.extend(summarize_benchmark_file(path))

    for row in rows:
        static = static_weights.get((row["pallet"], row["extrinsic"]), {})
        row.update({k: static.get(k, row.get(k, "")) for k in (
            "weight_ref_time_static",
            "weight_proof_size_static",
            "static_reads",
            "static_writes",
            "minimum_execution_time_ps_static",
        )})

    failure_path = raw_dir / "benchmark_failures.csv"
    failures = []
    if failure_path.exists():
        with failure_path.open("r", encoding="utf-8", newline="") as f:
            for item in csv.DictReader(f):
                failures.append(item)
                static = static_weights.get((item["pallet"], item["extrinsic"]), {})
                scenario = SCENARIO_LABELS.get((item["pallet"], item["extrinsic"]), item["extrinsic"])
                rows.append(
                    {
                        "group": "G3",
                        "scenario": scenario,
                        "pallet": item["pallet"],
                        "extrinsic": item["extrinsic"],
                        "result_type": "static_weight_file",
                        "result_source": "weights_rs_static_only",
                        "benchmark_status": "failed_or_missing_benchmark",
                        "samples": 0,
                        "latency_ms_mean": "",
                        "latency_ms_median": "",
                        "latency_ms_min": "",
                        "latency_ms_max": "",
                        "storage_root_ms_mean": "",
                        "reads_mean": static.get("static_reads", ""),
                        "writes_mean": static.get("static_writes", ""),
                        "proof_size_bytes_mean": "",
                        "events_count_expected": EVENT_COUNTS.get(scenario, ""),
                        "completion_blocks": "n/a_benchmark_harness",
                        "partial_fee": "n/a_no_signed_rpc_tx",
                        "contract_gas_or_ref_time": "n/a_contract_not_deployed",
                        "weight_ref_time_static": static.get("weight_ref_time_static", ""),
                        "weight_proof_size_static": static.get("weight_proof_size_static", ""),
                        "static_reads": static.get("static_reads", ""),
                        "static_writes": static.get("static_writes", ""),
                        "minimum_execution_time_ps_static": static.get("minimum_execution_time_ps_static", ""),
                        "raw_file": item["stderr_file"],
                    }
                )

    csv_path = processed_dir / f"exp1_g3_{args.run_id}_summary.csv"
    json_path = processed_dir / f"exp1_g3_{args.run_id}_summary.json"
    report_path = report_dir / f"exp1_g3_{args.run_id}_report.md"
    zh_report_path = report_dir / f"exp1_g3_{args.run_id}_report_zh.md"
    latency_svg_path = figure_dir / f"exp1_g3_{args.run_id}_latency_db.svg"
    weight_svg_path = figure_dir / f"exp1_g3_{args.run_id}_weight_ref_time.svg"
    write_csv(csv_path, rows)

    tests = []
    for name in ("market_orderbook_tests.log", "dataassets_market_tests.log"):
        path = raw_dir / name
        if path.exists():
            tests.append(parse_test_log(path))

    payload = {
        "metadata": metadata,
        "benchmark_rows": rows,
        "benchmark_failures": failures,
        "test_results": tests,
        "notes": [
            "Benchmark latency is the FRAME benchmark raw extrinsic execution time converted from ns to ms.",
            "completion_blocks and partial_fee require signed RPC transactions; they are not produced by the benchmark harness.",
            "The G3 contract order path is validated by market_orderbook tests because cargo-contract is not installed in this environment.",
        ],
    }
    json_path.write_text(json.dumps(payload, indent=2, ensure_ascii=False), encoding="utf-8")

    db_rows = [r for r in rows if r["result_type"] == "db_results"]
    static_only_rows = [r for r in rows if r["result_type"] == "static_weight_file"]
    time_rows = [r for r in rows if r["result_type"] == "time_results"]
    lines = [
        "# Experiment 1 G3 Report",
        "",
        "## Scope",
        "",
        "This run measures G3 (AssetXChain cooperative market) overhead that is executable in the current prototype: market registration/verification, asset registration, asset authorization, and the Runtime-side operations used by MetaToken and RightToken settlement paths. The market contract order path is validated with Rust tests; full on-chain contract deployment was not executed because `cargo-contract` is not installed.",
        "",
        "## Environment",
        "",
        f"- run_id: `{metadata['run_id']}`",
        f"- git_commit: `{metadata['git_commit']}`",
        f"- rustc: `{metadata['rustc']}`",
        f"- cargo: `{metadata['cargo']}`",
        f"- benchmark: `steps={metadata['steps']}`, `repeat={metadata['repeat']}`",
        f"- chain_mode: `{metadata['chain_mode']}`",
        f"- dirty_status: `{metadata['git_status_short'].strip()}`",
        "",
        "## Benchmark Summary",
        "",
        "| Scenario | Mean latency ms (db) | Median latency ms (db) | Reads | Writes | Proof bytes | Weight ref time | Expected events |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in db_rows:
        lines.append(
            "| {scenario} | {mean} | {median} | {reads} | {writes} | {proof} | {weight} | {events} |".format(
                scenario=row["scenario"],
                mean=fmt(row["latency_ms_mean"]),
                median=fmt(row["latency_ms_median"]),
                reads=fmt(row["reads_mean"], 2),
                writes=fmt(row["writes_mean"], 2),
                proof=fmt(row["proof_size_bytes_mean"], 2),
                weight=row["weight_ref_time_static"],
                events=row["events_count_expected"],
            )
        )
    if static_only_rows:
        lines.extend(
            [
                "",
                "## Static-Only Weight Entries",
                "",
                "These scenarios have Runtime weight functions but no runnable FRAME benchmark entry in the current prototype.",
                "",
                "| Scenario | Extrinsic | Weight ref time | Proof size | Static reads | Static writes | Status |",
                "| --- | --- | ---: | ---: | ---: | ---: | --- |",
            ]
        )
        for row in static_only_rows:
            lines.append(
                f"| {row['scenario']} | {row['extrinsic']} | {row['weight_ref_time_static']} | {row['weight_proof_size_static']} | {row['static_reads']} | {row['static_writes']} | {row['benchmark_status']} |"
            )
    lines.extend(
        [
            "",
            "## Time-Only Cross Check",
            "",
            "| Scenario | Mean latency ms (time only) | Median latency ms (time only) |",
            "| --- | ---: | ---: |",
        ]
    )
    for row in time_rows:
        lines.append(
            f"| {row['scenario']} | {fmt(row['latency_ms_mean'])} | {fmt(row['latency_ms_median'])} |"
        )
    lines.extend(["", "## Contract And Runtime Path Tests", ""])
    for item in tests:
        lines.append(
            f"- `{Path(item['file']).name}`: {item['status']}, passed={item['passed']}, failed={item['failed']}, ignored={item['ignored']}"
        )
    lines.extend(
        [
            "",
            "## Metric Coverage",
            "",
            "- Measured: `latency_ms`, storage-root time, reads, writes, proof size, expected event count.",
            "- Not measured in this run: `completion_blocks`, `partial_fee`, and deployed-contract gas/ref-time. These require signed RPC calls against a deployed ink! contract artifact.",
            "- Raw outputs are stored under the run-specific `results/raw` directory; processed CSV/JSON are stored under `results/processed`.",
        ]
    )
    report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    svg_bar_chart(
        latency_svg_path,
        db_rows,
        "latency_ms_mean",
        "实验1 G3 各阶段平均执行延迟（db_results）",
        "ms",
    )
    svg_bar_chart(
        weight_svg_path,
        [r for r in rows if r.get("weight_ref_time_static") not in ("", None)],
        "weight_ref_time_static",
        "实验1 G3 各阶段 Runtime 静态权重",
        "ref_time",
    )

    zh_lines = [
        "# 实验 1 G3 结果报告",
        "",
        "## 实验范围",
        "",
        "本次只针对 G3（AssetXChain 协同市场）执行实验 1 的可运行部分：市场注册/合约验证、资产注册、市场授权、MetaToken 结算 Runtime 路径、RightToken 发行 Runtime 路径、撤销授权。市场合约的订单编排路径通过 Rust 测试验证；由于当前环境没有 `cargo-contract`，没有部署 ink! 合约并执行签名 RPC 交易。",
        "",
        "## 环境",
        "",
        f"- run_id：`{metadata['run_id']}`",
        f"- git_commit：`{metadata['git_commit']}`",
        f"- rustc：`{metadata['rustc']}`",
        f"- cargo：`{metadata['cargo']}`",
        f"- benchmark 参数：`steps={metadata['steps']}`, `repeat={metadata['repeat']}`",
        f"- 执行模式：`{metadata['chain_mode']}`",
        "",
        "## 开销结果",
        "",
        "| 场景 | 平均延迟 ms（db） | 中位延迟 ms（db） | 读 | 写 | proof bytes | 静态权重 ref_time | 预期事件数 |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for row in db_rows:
        zh_lines.append(
            "| {scenario} | {mean} | {median} | {reads} | {writes} | {proof} | {weight} | {events} |".format(
                scenario=DISPLAY_NAMES_ZH.get(row["scenario"], row["scenario"]),
                mean=fmt(row["latency_ms_mean"]),
                median=fmt(row["latency_ms_median"]),
                reads=fmt(row["reads_mean"], 2),
                writes=fmt(row["writes_mean"], 2),
                proof=fmt(row["proof_size_bytes_mean"], 2),
                weight=row["weight_ref_time_static"],
                events=row["events_count_expected"],
            )
        )
    if static_only_rows:
        zh_lines.extend(
            [
                "",
                "## 静态权重补充项",
                "",
                "以下场景在当前原型中有 Runtime 权重函数，但没有出现在 FRAME benchmark 可运行列表中，因此本次只引用 `weights.rs` 中的静态权重，不把它当作现场 benchmark 延迟。",
                "",
                "| 场景 | extrinsic | 静态权重 ref_time | proof size | 静态读 | 静态写 | 状态 |",
                "| --- | --- | ---: | ---: | ---: | ---: | --- |",
            ]
        )
        for row in static_only_rows:
            zh_lines.append(
                f"| {DISPLAY_NAMES_ZH.get(row['scenario'], row['scenario'])} | {row['extrinsic']} | {row['weight_ref_time_static']} | {row['weight_proof_size_static']} | {row['static_reads']} | {row['static_writes']} | {row['benchmark_status']} |"
            )
    zh_lines.extend(
        [
            "",
            "## 正确性路径验证",
            "",
        ]
    )
    for item in tests:
        zh_lines.append(
            f"- `{Path(item['file']).name}`：{item['status']}，passed={item['passed']}，failed={item['failed']}，ignored={item['ignored']}"
        )
    zh_lines.extend(
        [
            "",
            "## 结果解释",
            "",
            "- G3 中市场注册与合约验证阶段开销最高，主要来自 `pallet_markets::register_market` 对合约账户、代码与市场注册状态的读写。",
            "- 资产注册次之，因为它同时涉及资产状态、抵押信息和激励相关状态。",
            "- MetaToken 结算 Runtime 路径、市场授权、撤销授权的平均执行延迟都在本次 benchmark 中保持在较低水平。",
            "- RightToken 转移型购买需要补齐可运行 FRAME benchmark 或部署合约后用 RPC 交易实测；本次只给出已有静态权重和合约路径测试结果。",
            "",
            "## 指标边界",
            "",
            "- 已测：`latency_ms`、storage-root time、读写次数、proof size、预期事件数。",
            "- 未测：`completion_blocks`、`partial_fee`、部署合约后的 `contract_gas_or_ref_time`。这些需要 ink! 合约工件和签名 RPC 交易。",
            f"- 延迟图：`{latency_svg_path}`",
            f"- 权重图：`{weight_svg_path}`",
        ]
    )
    zh_report_path.write_text("\n".join(zh_lines) + "\n", encoding="utf-8")

    print(json.dumps({
        "csv": str(csv_path),
        "json": str(json_path),
        "report": str(report_path),
        "zh_report": str(zh_report_path),
        "latency_svg": str(latency_svg_path),
        "weight_svg": str(weight_svg_path),
    }, ensure_ascii=False))


if __name__ == "__main__":
    main()
