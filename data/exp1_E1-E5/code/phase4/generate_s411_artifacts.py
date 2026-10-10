#!/usr/bin/env python3
"""Generate S4.11 correctness/cost tables and SVG figures from frozen inputs."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import os
import statistics
from pathlib import Path

CANDIDATE_ID = "p4-paper-candidate-20260806T004403+0800"
CANDIDATE_SHA = "40c349d225241d08509f5ff9bf688c65da257a853b01eb02aab374499309cca2"
RUNS = {
    "B1": "p4-e6-b1-20260807T032215+0800-8514bf07-9a8b56c2",
    "B2": "p4-e6-b2-20260807T032218+0800-dc10ea68-9a8b56c2",
    "G3": "p4-e6-g3-20260807T032220+0800-47ebd794-9a8b56c2",
}
INPUT_SHA = {
    "B1": "fba9c202ece8afd94e630d24e3a1da90719275ca4e2621b16d0af97f3987b5de",
    "B2": "f489278a042627950f4dd5ae4063bbc46dc473b8c340b637b6b6812b8ec5a475",
    "G3": "dde02af420b9b96e3106277f3d64d2e58a59b6e9e551fb80539075103ce32449",
}
WORKLOAD_RELATIVE = Path("configs/workloads/formal_cost_v2.json")
WORKLOAD_SHA = "9a8b56c26c449b97560857c29fb7adde208f4ebd4878eabc29d7de2973283530"
NAMES = {"B1": "Contract-only", "B2": "Runtime-only", "G3": "AssetXChain-CRS"}
PHASES = ("create_order", "fund_order", "settle")
OPERATIONS = {
    "LEG-META-01": "Meta sale",
    "LEG-ISSUE-01": "Right issue",
    "LEG-RESALE-01": "Right resale",
}
METRICS = [
    ("cost", "latency_ms", "latency", "ms"),
    ("cost", "completion_blocks", "completion_blocks", "blocks"),
    ("cost", "weight_ref_time", "weight_ref_time", "ref_time"),
    ("cost", "weight_proof_size", "weight_proof_size", "bytes"),
    ("cost", "partial_fee", "partial_fee", "plancks"),
    ("cost", "events_count", "events_count", "count"),
    ("cost", "rpc_response_ms", "rpc_response", "ms"),
    ("cost", "storage_writes", "storage_writes", "count"),
    ("architecture_cost", "contract_gas_or_ref_time", "contract_gas_or_ref_time", "ref_time"),
    ("architecture_cost", "contract_storage_writes", "contract_storage_writes", "count"),
    ("architecture_cost", "projection_storage_writes", "projection_storage_writes", "count"),
    ("architecture_cost", "runtime_dispatch_count", "runtime_dispatch_count", "count"),
    ("architecture_cost", "runtime_storage_writes", "runtime_storage_writes", "count"),
    ("architecture_cost", "typed_ce_call_count", "typed_ce_call_count", "count"),
]
METRIC_SOURCES = {
    "weight_ref_time": "terminal system dispatch_info.weight.refTime",
    "weight_proof_size": "terminal system dispatch_info.weight.proofSize",
    "partial_fee": "transactionPayment.TransactionFeePaid.actual_fee",
}


def file_sha(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def percentile(values: list[float], quantile: float) -> float:
    ordered = sorted(values)
    if len(ordered) == 1:
        return ordered[0]
    position = (len(ordered) - 1) * quantile
    lower, upper = math.floor(position), math.ceil(position)
    if lower == upper:
        return ordered[lower]
    return ordered[lower] + (ordered[upper] - ordered[lower]) * (position - lower)


def summarize(values: list[float]) -> dict:
    if not values:
        return {"n": 0, "median": None, "mean": None, "stdev": None, "p10": None, "p90": None}
    return {
        "n": len(values),
        "median": statistics.median(values),
        "mean": statistics.fmean(values),
        "stdev": statistics.stdev(values) if len(values) > 1 else 0.0,
        "p10": percentile(values, 0.10),
        "p90": percentile(values, 0.90),
    }


def measured_values(rows: list[dict], group: str, key: str) -> tuple[str, list[float]]:
    availability = {row[group][key]["availability"] for row in rows}
    if availability == {"N/A"}:
        if any(row[group][key]["value"] is not None for row in rows):
            raise SystemExit(f"{group}.{key} encodes N/A with a non-null value")
        return "N/A", []
    if availability != {"measured"}:
        raise SystemExit(f"{group}.{key} has mixed or unsupported availability: {availability}")
    values = [float(row[group][key]["value"]) for row in rows]
    if len(values) != len(rows):
        raise SystemExit(f"{group}.{key} has incomplete measured values")
    return "measured", values


def aggregate_orders(rows: list[dict], baseline: str, scenario_id: str) -> list[dict]:
    samples = []
    repetitions = sorted({row["repetition"] for row in rows})
    if repetitions != list(range(1, 11)):
        raise SystemExit(f"{baseline}/{scenario_id} repetitions are not exactly 1..10")
    for repetition in repetitions:
        attempts = [row for row in rows if row["repetition"] == repetition]
        if len(attempts) != len(PHASES) or {row["phase"] for row in attempts} != set(PHASES):
            raise SystemExit(
                f"{baseline}/{scenario_id}/r-{repetition:03d} does not contain the three required phases"
            )
        sample = {"repetition": repetition, "metrics": {}}
        for group, key, metric, _unit in METRICS:
            availability, values = measured_values(attempts, group, key)
            sample["metrics"][metric] = {
                "availability": availability,
                "value": sum(values) if availability == "measured" else None,
            }
        availability, values = measured_values(attempts, "cost", "phase_call_count")
        if availability != "measured":
            raise SystemExit(f"{baseline}/{scenario_id} phase call count is not measured")
        sample["phase_call_count_total"] = sum(values)
        samples.append(sample)
    return samples


def display(value) -> str:
    if value is None:
        return "N/A"
    number = float(value)
    if abs(number) >= 1_000_000:
        return f"{number:.3e}"
    if abs(number - round(number)) < 1e-9:
        return str(int(round(number)))
    return f"{number:.3f}"


def write_csv(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", newline="", encoding="utf-8") as output:
        writer = csv.DictWriter(output, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)
    os.chmod(path, 0o444)


def write_json(path: Path, value) -> None:
    path.write_text(json.dumps(value, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    os.chmod(path, 0o444)


def make_svg(path: Path, title: str, unit: str, rows: list[dict]) -> None:
    width, height = 1180, 620
    left, right, top, bottom = 95, 30, 70, 120
    plot_width, plot_height = width - left - right, height - top - bottom
    upper = max(float(row["p90_raw"]) for row in rows) * 1.18
    colors = {"B1": "#2563eb", "B2": "#d97706", "G3": "#15803d"}
    bar_width = plot_width / len(rows) * 0.62
    svg = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        '<rect width="100%" height="100%" fill="white"/>',
        f'<text x="{left}" y="32" font-family="Arial" font-size="22" font-weight="600">{title}</text>',
        f'<text x="18" y="{top + plot_height / 2}" transform="rotate(-90 18 {top + plot_height / 2})" text-anchor="middle" font-family="Arial" font-size="14">{unit}</text>',
    ]
    for index in range(6):
        y = top + plot_height - plot_height * index / 5
        value = upper * index / 5
        svg.append(f'<line x1="{left}" y1="{y:.1f}" x2="{width-right}" y2="{y:.1f}" stroke="#e5e7eb"/>')
        svg.append(f'<text x="{left-10}" y="{y+5:.1f}" text-anchor="end" font-family="Arial" font-size="12" fill="#4b5563">{display(value)}</text>')
    svg.append(f'<line x1="{left}" y1="{top}" x2="{left}" y2="{top+plot_height}" stroke="#374151"/>')
    svg.append(f'<line x1="{left}" y1="{top+plot_height}" x2="{width-right}" y2="{top+plot_height}" stroke="#374151"/>')
    for index, row in enumerate(rows):
        x = left + plot_width * (index + 0.5) / len(rows)
        median, p10, p90 = (float(row[key]) for key in ("median_raw", "p10_raw", "p90_raw"))
        base_y = top + plot_height
        median_y = base_y - plot_height * median / upper
        p10_y = base_y - plot_height * p10 / upper
        p90_y = base_y - plot_height * p90 / upper
        color = colors[row["baseline_id"]]
        svg.append(f'<rect x="{x-bar_width/2:.1f}" y="{median_y:.1f}" width="{bar_width:.1f}" height="{base_y-median_y:.1f}" fill="{color}"/>')
        svg.append(f'<line x1="{x:.1f}" y1="{p10_y:.1f}" x2="{x:.1f}" y2="{p90_y:.1f}" stroke="#111827" stroke-width="2"/>')
        svg.append(f'<line x1="{x-7:.1f}" y1="{p10_y:.1f}" x2="{x+7:.1f}" y2="{p10_y:.1f}" stroke="#111827"/>')
        svg.append(f'<line x1="{x-7:.1f}" y1="{p90_y:.1f}" x2="{x+7:.1f}" y2="{p90_y:.1f}" stroke="#111827"/>')
        svg.append(f'<text x="{x:.1f}" y="{base_y+22}" text-anchor="middle" font-family="Arial" font-size="12">{row["baseline_id"]}</text>')
        svg.append(f'<text x="{x:.1f}" y="{base_y+39}" text-anchor="middle" font-family="Arial" font-size="11" fill="#374151">{row["operation"]}</text>')
    legend_x = left
    for baseline, name in NAMES.items():
        svg.append(f'<rect x="{legend_x}" y="{height-35}" width="14" height="14" fill="{colors[baseline]}"/>')
        svg.append(f'<text x="{legend_x+20}" y="{height-23}" font-family="Arial" font-size="12">{baseline} {name}</text>')
        legend_x += 175
    svg.append(f'<text x="{width-right}" y="{height-23}" text-anchor="end" font-family="Arial" font-size="11" fill="#4b5563">complete order; bars=median; whiskers=p10-p90; n=10</text>')
    svg.append("</svg>")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(svg) + "\n", encoding="utf-8")
    os.chmod(path, 0o444)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("/home/gz785/code/exp2"))
    parser.add_argument("--candidate-manifest", type=Path, required=True)
    parser.add_argument("--out-root", type=Path, required=True)
    parser.add_argument("--figure-root", type=Path, required=True)
    args = parser.parse_args()

    candidate = json.loads(args.candidate_manifest.read_text(encoding="utf-8"))
    if candidate.get("candidate_id") != CANDIDATE_ID or file_sha(args.candidate_manifest) != CANDIDATE_SHA:
        raise SystemExit("candidate identity/hash mismatch")
    if candidate.get("candidate_gate", {}).get("status") != "PASS":
        raise SystemExit("candidate gate is not PASS")

    workload_path = args.root / WORKLOAD_RELATIVE
    workload = json.loads(workload_path.read_text(encoding="utf-8"))
    if file_sha(workload_path) != WORKLOAD_SHA:
        raise SystemExit("corrected E6 workload hash mismatch")
    metadata = workload.get("metadata", {})
    if metadata.get("repetitions") != 10 or metadata.get("baseline_ids") != ["B1", "B2", "G3"]:
        raise SystemExit("corrected E6 workload identity mismatch")

    input_rows, input_hashes = {}, {}
    for baseline, run_id in RUNS.items():
        path = args.root / "results" / "processed" / run_id / "attempts.jsonl"
        if file_sha(path) != INPUT_SHA[baseline]:
            raise SystemExit(f"{baseline} corrected processed input hash mismatch")
        rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
        if len(rows) != 90:
            raise SystemExit(f"{baseline} processed row count is not 90")
        if {row["baseline_id"] for row in rows} != {baseline} or {row["run_id"] for row in rows} != {run_id}:
            raise SystemExit(f"{baseline} processed identity mismatch")
        if any(row["actual_outcome"] != "accepted" or not row["expected_outcome_match"] for row in rows):
            raise SystemExit(f"{baseline} contains an unexpected E6 outcome")
        input_rows[baseline] = rows
        input_hashes[baseline] = {"path": str(path), "sha256": file_sha(path)}

    args.out_root.mkdir(parents=True, exist_ok=True)
    args.figure_root.mkdir(parents=True, exist_ok=True)

    correctness_rows = []
    for family in candidate["candidate_gate"]["families"]:
        for baseline in ("B1", "B2", "G3"):
            item = candidate["correctness_runs"][family][baseline]
            correctness_rows.append({
                "baseline_id": baseline, "baseline_name": NAMES[baseline], "family": family,
                "planned_units": item["planned_units"], "completed_units": item["completed_units"],
                "planned_attempts": item["planned_attempts"], "completed_attempts": item["completed_attempts"],
                "result_count": item["result_count"], "independent_recheck_status": item["independent_recheck_status"],
                "artifact_validation": item["artifact_validation"], "seal_status": item["seal_status"],
                "run_id": item["run_id"], "source_seal_sha256": item["seal_sha256"],
            })
    write_csv(args.out_root / "correctness_runs.csv", correctness_rows)
    gate = candidate["candidate_gate"]["checker_totals"]
    audit = candidate["candidate_gate"]["e5_audit_closure"]
    write_csv(args.out_root / "correctness_gate.csv", [{
        "scope": "all E1-E5 candidate runs", **gate,
        "successful_settlement_records": audit["successful_settlement_records"],
        "settlement_record_field_count": audit["settlement_record_field_count"],
        "complete_proof": audit["complete_proof"], "i11": audit["i11"],
    }])

    cost_rows, phase_cost_rows, raw_stats, order_samples = [], [], {}, {}
    for baseline, rows in input_rows.items():
        for scenario_id, operation in OPERATIONS.items():
            subset = [row for row in rows if row["scenario_id"] == scenario_id]
            samples = aggregate_orders(subset, baseline, scenario_id)
            order_samples[(baseline, scenario_id)] = samples
            for group, key, metric, unit in METRICS:
                states = {sample["metrics"][metric]["availability"] for sample in samples}
                values = [
                    sample["metrics"][metric]["value"]
                    for sample in samples
                    if sample["metrics"][metric]["availability"] == "measured"
                ]
                if states not in ({"measured"}, {"N/A"}):
                    raise SystemExit(f"{baseline}/{scenario_id}/{metric} has mixed per-order availability")
                summary = summarize(values)
                state = "measured" if states == {"measured"} else "N/A"
                raw_stats[(baseline, operation, metric)] = summary
                cost_rows.append({
                    "baseline_id": baseline, "baseline_name": NAMES[baseline], "operation": operation,
                    "metric": metric, "availability": state, "unit": unit, "n": summary["n"],
                    "median": display(summary["median"]), "mean": display(summary["mean"]),
                    "stdev": display(summary["stdev"]), "p10": display(summary["p10"]), "p90": display(summary["p90"]),
                    "scope": "complete order; sum of create_order, fund_order, settle per repetition; R=10",
                    "metric_source": METRIC_SOURCES.get(metric, "corrected processed E6 field"),
                    "source_run_id": RUNS[baseline],
                })
                for phase in PHASES:
                    phase_subset = [row for row in subset if row["phase"] == phase]
                    phase_state, phase_values = measured_values(phase_subset, group, key)
                    phase_summary = summarize(phase_values)
                    phase_cost_rows.append({
                        "baseline_id": baseline, "baseline_name": NAMES[baseline], "operation": operation,
                        "phase": phase, "metric": metric, "availability": phase_state, "unit": unit,
                        "n": phase_summary["n"], "median": display(phase_summary["median"]),
                        "mean": display(phase_summary["mean"]), "stdev": display(phase_summary["stdev"]),
                        "p10": display(phase_summary["p10"]), "p90": display(phase_summary["p90"]),
                        "scope": "single protocol phase; R=10",
                        "metric_source": METRIC_SOURCES.get(metric, "corrected processed E6 field"),
                        "source_run_id": RUNS[baseline],
                    })
    write_csv(args.out_root / "e6_cost_table.csv", cost_rows)
    write_csv(args.out_root / "e6_phase_cost_table.csv", phase_cost_rows)

    call_rows = []
    for baseline in input_rows:
        for scenario_id, operation in OPERATIONS.items():
            totals = [sample["phase_call_count_total"] for sample in order_samples[(baseline, scenario_id)]]
            if set(totals) != {3.0}:
                raise SystemExit(f"{baseline}/{scenario_id} does not have exactly three calls per order")
            summary = summarize(totals)
            call_rows.append({
                "baseline_id": baseline, "baseline_name": NAMES[baseline], "operation": operation,
                "scope": "per-order", "metric": "phase_call_count_total", "unit": "count", "n": summary["n"],
                "median": display(summary["median"]), "mean": display(summary["mean"]), "stdev": display(summary["stdev"]),
                "p10": display(summary["p10"]), "p90": display(summary["p90"]),
                "setup_calls": "not-collected (outside processed E6 schema; not architecture N/A)",
                "source_run_id": RUNS[baseline],
            })
    write_csv(args.out_root / "e6_order_call_counts.csv", call_rows)

    for metric, title, unit, filename in [
        ("latency", "E6 complete-order latency at fixed medium load", "sum of three phase latencies (ms)", "s4_11_order_latency_median_ms.svg"),
        ("partial_fee", "E6 complete-order actual transaction fee at fixed medium load", "sum of actual fees (plancks)", "s4_11_order_actual_fee_median_plancks.svg"),
        ("weight_ref_time", "E6 complete-order post-dispatch weight at fixed medium load", "sum of post-dispatch ref_time", "s4_11_order_weight_ref_time_median.svg"),
    ]:
        chart_rows = []
        for baseline in ("B1", "B2", "G3"):
            for operation in OPERATIONS.values():
                summary = raw_stats[(baseline, operation, metric)]
                chart_rows.append({"baseline_id": baseline, "operation": operation, "median_raw": summary["median"], "p10_raw": summary["p10"], "p90_raw": summary["p90"]})
        make_svg(args.figure_root / filename, title, unit, chart_rows)

    lookup = {(row["baseline_id"], row["operation"], row["metric"]): row for row in cost_rows}
    lines = [
        "# S4.11 Tables and Figures", "",
        f"Candidate: {CANDIDATE_ID}; manifest SHA-256: {CANDIDATE_SHA}.", "",
        "Validity: paper-final correctness only plus corrected formal-cost-candidate; fixed-medium-load-only; corrected cost is not paper-final. Main experiment contains B1/B2/G3 only; L1/L2 are excluded.", "",
        "## Correctness gate", "",
        "Correctness rows are copied from the immutable S4.8 candidate manifest. S4.11 does not re-read or modify E1-E5 raw files.", "",
        "| Completed units | Completed attempts | Expected mismatches | False accepts | False rejects | Invariant violations | Duplicates | Dirty state | Event mismatches | Timeouts | SettlementRecords | Fields/record |",
        "|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|",
        f"| {gate['completed_units']} | {gate['completed_attempts']} | {gate['expected_outcome_mismatches']} | {gate['false_accepts']} | {gate['false_rejects']} | {gate['invariant_violations']} | {gate['duplicates']} | {gate['dirty_state']} | {gate['event_mismatches']} | {gate['timeouts']} | {audit['successful_settlement_records']} | {audit['settlement_record_field_count']} |",
        "", "## E6 cost summary", "",
        "The primary cost table first sums create_order, fund_order, and settle within each repetition, then reports n, median, mean, sample standard deviation, and p10-p90 across the ten complete orders. The phase table preserves the three components separately. N/A denotes architecture inapplicability, not zero. Setup calls are outside the processed E6 schema and are labeled not-collected rather than N/A; every measured order contains three required phase calls.", "",
        "The fee column is the sum of TransactionFeePaid.actual_fee for the three extrinsics. The weight column is the sum of their terminal post-dispatch ref_time values; neither column uses paymentInfo estimates or the G3 declared contract gas limit.", "",
        "| Baseline | Operation | Complete-order latency median (ms) | Actual fee median (plancks) | Post-dispatch ref_time median | Storage writes median | Events median | n |",
        "|---|---|---:|---:|---:|---:|---:|---:|",
    ]
    for baseline in ("B1", "B2", "G3"):
        for operation in OPERATIONS.values():
            get = lambda metric: lookup[(baseline, operation, metric)]["median"]
            sample_count = lookup[(baseline, operation, "latency")]["n"]
            lines.append(f"| {baseline} {NAMES[baseline]} | {operation} | {get('latency')} | {get('partial_fee')} | {get('weight_ref_time')} | {get('storage_writes')} | {get('events_count')} | {sample_count} |")
    lines.extend(["", "No inferential significance test or scalability curve is produced. These fixed-medium-load observations support descriptive comparison only; corrected cost evidence remains pending S4.12 independent review.", "", "## Figures", "", "- s4_11_order_latency_median_ms.svg: complete-order latency median with p10-p90 whiskers.", "- s4_11_order_actual_fee_median_plancks.svg: complete-order actual transaction fee median with p10-p90 whiskers.", "- s4_11_order_weight_ref_time_median.svg: complete-order post-dispatch ref_time median with p10-p90 whiskers.", ""])
    report = args.out_root / "tables_and_figures.md"
    report.write_text("\n".join(lines), encoding="utf-8")
    os.chmod(report, 0o444)

    derivation = {
        "schema_id": "assetxchain-exp2-s411-artifacts", "schema_version": 2, "step": "S4.11",
        "candidate_id": CANDIDATE_ID, "candidate_manifest": str(args.candidate_manifest),
        "candidate_manifest_sha256": CANDIDATE_SHA,
        "correctness_source": "immutable S4.8 candidate manifest correctness_runs and candidate_gate",
        "cost_sources": input_hashes, "baselines": ["B1", "B2", "G3"], "excluded_baselines": ["L1", "L2"],
        "workload": {"path": str(workload_path), "sha256": WORKLOAD_SHA},
        "repetitions": 10, "statistics": ["n", "median", "mean", "sample_stdev", "p10", "p90"],
        "primary_cost_aggregation": "sum create_order, fund_order, and settle within each repetition before descriptive statistics",
        "phase_detail": "reported separately in e6_phase_cost_table.csv",
        "corrected_metric_semantics": METRIC_SOURCES,
        "anomaly_policy": "retain all samples; canonical E6 validators reported anomaly_count=0",
        "setup_calls": "not-collected: outside processed E6 schema; not architecture N/A",
        "claim_boundary": "paper-final correctness only; descriptive fixed-medium-load corrected formal-cost-candidate; corrected cost not-paper-final",
        "outputs": {"table_root": str(args.out_root), "figure_root": str(args.figure_root)},
    }
    write_json(args.out_root / "derivation.json", derivation)
    print(json.dumps({"status": "PASS", "correctness_rows": len(correctness_rows), "cost_rows": len(cost_rows), "phase_cost_rows": len(phase_cost_rows), "table_root": str(args.out_root), "figure_root": str(args.figure_root)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
