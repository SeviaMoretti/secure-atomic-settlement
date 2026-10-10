"""Validate E6 cost units and completed sealed E6 batches without mutating raw."""

from __future__ import annotations

import argparse
import hashlib
import json
import stat
from pathlib import Path
from typing import Any, Mapping


COMMON = {
    "latency_ms", "completion_blocks", "partial_fee", "weight_ref_time",
    "weight_proof_size", "storage_writes", "events_count", "rpc_response_ms",
    "phase_call_count",
}
ARCH = {
    "contract_gas_or_ref_time", "contract_storage_writes",
    "runtime_dispatch_count", "runtime_storage_writes", "typed_ce_call_count",
    "projection_storage_writes",
}
APPLICABLE = {
    "B1": {"contract_gas_or_ref_time", "contract_storage_writes"},
    "B2": {"runtime_dispatch_count", "runtime_storage_writes"},
    "G3": set(ARCH),
}
CLAIM_BOUNDARY = "formal-cost-candidate; fixed-medium-load-only; not-paper-final"


class E6ValidationError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise E6ValidationError(message)


def load(path: Path) -> dict[str, Any]:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise E6ValidationError(f"cannot load {path}: {error}") from error


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_candidate(preflight: Mapping[str, Any], path: Path, expected_hash: str) -> dict[str, Any]:
    require(path.is_file(), f"candidate manifest is missing: {path}")
    require(sha256(path) == expected_hash, "candidate manifest SHA-256 mismatch")
    mode = stat.S_IMODE(path.stat().st_mode)
    require(mode == 0o444, f"candidate manifest mode is {mode:o}, expected 444")
    candidate = load(path)
    require(candidate.get("status") == "frozen", "candidate is not frozen")
    require(candidate.get("validity_level") == "paper-candidate", "candidate level differs")
    authorization = candidate.get("authorization", {})
    require(authorization.get("e6_authorized") is True, "candidate does not authorize E6")
    require(authorization.get("performance_claims_authorized") is False,
            "S4.8 candidate unexpectedly authorizes performance claims")
    require(preflight["prerequisites"]["approved_s4_8_candidate_id"] == candidate["candidate_id"],
            "preflight/candidate ID mismatch")
    return candidate


def phase_extrinsic_index(phase: Any) -> int | None:
    if isinstance(phase, Mapping):
        value = phase.get("applyExtrinsic", phase.get("apply_extrinsic"))
        return int(value) if value is not None else None
    if isinstance(phase, str):
        try:
            return phase_extrinsic_index(json.loads(phase))
        except json.JSONDecodeError:
            return None
    return None


def event_payload(event: Mapping[str, Any], index: int, label: str) -> Mapping[str, Any]:
    data = event.get("data")
    require(isinstance(data, list) and len(data) > index, f"{label} data is missing")
    try:
        value = json.loads(data[index]) if isinstance(data[index], str) else data[index]
    except json.JSONDecodeError as error:
        raise E6ValidationError(f"{label} data is not JSON") from error
    require(isinstance(value, Mapping), f"{label} payload is not an object")
    return value


def actual_chain_metrics(block: Mapping[str, Any], extrinsic_index: int) -> dict[str, str]:
    rows = [
        row for row in block.get("events", [])
        if phase_extrinsic_index(row.get("phase")) == extrinsic_index
    ]
    terminal = [
        row for row in rows
        if row.get("section") == "system"
        and row.get("method") in {"ExtrinsicSuccess", "ExtrinsicFailed"}
    ]
    require(len(terminal) == 1, "block evidence must contain one terminal dispatch event for the extrinsic")
    terminal_row = terminal[0]
    payload_index = 0 if terminal_row["method"] == "ExtrinsicSuccess" else 1
    dispatch = event_payload(terminal_row, payload_index, terminal_row["method"])
    weight = dispatch.get("weight", dispatch.get("actualWeight", dispatch.get("actual_weight")))
    require(isinstance(weight, Mapping), "terminal dispatch event omitted post-dispatch weight")
    ref_time = weight.get("refTime", weight.get("ref_time"))
    proof_size = weight.get("proofSize", weight.get("proof_size"))
    require(ref_time is not None and proof_size is not None, "post-dispatch weight is incomplete")
    fees = [
        row for row in rows
        if row.get("section") == "transactionPayment" and row.get("method") == "TransactionFeePaid"
    ]
    require(len(fees) == 1, "block evidence must contain one actual fee event for the extrinsic")
    fee_data = fees[0].get("data")
    require(isinstance(fee_data, list) and len(fee_data) > 1, "actual fee event data is incomplete")
    return {
        "weight_ref_time": str(ref_time),
        "weight_proof_size": str(proof_size),
        "actual_fee": str(fee_data[1]),
    }


def weight_value(value: Any, camel: str, snake: str) -> str:
    require(isinstance(value, Mapping), "weight value is not an object")
    item = value.get(camel, value.get(snake))
    require(item is not None, f"weight field {camel} is missing")
    return str(item)


def validate_metrics(
    result: Mapping[str, Any], measurement: Mapping[str, Any], block: Mapping[str, Any], baseline: str,
) -> None:
    cost = result.get("cost", {})
    architecture = result.get("architecture_cost", {})
    require(set(cost) == COMMON, "common metric key set differs")
    require(set(architecture) == ARCH, "architecture metric key set differs")
    for name, metric in cost.items():
        require(metric.get("availability") == "measured", f"common metric {name} is not measured")
        require(metric.get("value") is not None, f"common metric {name} has null value")
    for name, metric in architecture.items():
        if name in APPLICABLE[baseline]:
            require(metric.get("availability") == "measured", f"applicable metric {name} is not measured")
            require(metric.get("value") is not None, f"applicable metric {name} has null value")
        else:
            require(metric == {"availability": "N/A", "value": None, "unit": metric.get("unit")},
                    f"inapplicable metric {name} must be N/A/null")
    raw = measurement["raw_measurement"]
    require(measurement.get("schema_version") == 2, "E6 measurement schema must be v2")
    require(raw.get("actual_fee") is not None, "actual fee is missing from raw measurement")
    require(raw.get("post_dispatch_weight") is not None, "post-dispatch weight is missing from raw measurement")
    chain = actual_chain_metrics(block, int(raw["extrinsic_index"]))
    require(raw["actual_fee"] == chain["actual_fee"], "raw actual fee differs from TransactionFeePaid event")
    require(raw["weight_ref_time"] == chain["weight_ref_time"],
            "raw ref_time differs from terminal dispatch event")
    require(raw["weight_proof_size"] == chain["weight_proof_size"],
            "raw proof_size differs from terminal dispatch event")
    require(weight_value(raw["post_dispatch_weight"], "refTime", "ref_time") == chain["weight_ref_time"],
            "raw post-dispatch ref_time differs from terminal dispatch event")
    require(weight_value(raw["post_dispatch_weight"], "proofSize", "proof_size") == chain["weight_proof_size"],
            "raw post-dispatch proof_size differs from terminal dispatch event")
    require(result["outcome"]["dispatch_fee_amount"] == chain["actual_fee"],
            "result dispatch fee is not the actual fee event")
    require(cost["partial_fee"]["value"] == chain["actual_fee"],
            "result partial_fee is not the actual fee event")
    require(cost["weight_ref_time"]["value"] == chain["weight_ref_time"],
            "result ref_time is not the terminal dispatch weight")
    require(cost["weight_proof_size"]["value"] == chain["weight_proof_size"],
            "result proof_size is not the terminal dispatch weight")
    if baseline == "G3":
        declared = raw.get("declared_contract_gas_limit")
        require(declared is not None, "G3 measured extrinsic omitted declared contract gas limit")
        require(weight_value(declared, "refTime", "ref_time") == "1000000000000",
                "G3 declared ref_time gas limit differs from frozen WeightV2 bound")
        require(weight_value(declared, "proofSize", "proof_size") == "104857600",
                "G3 declared proof_size gas limit differs from frozen WeightV2 bound")
        require((chain["weight_ref_time"], chain["weight_proof_size"]) !=
                ("1000000000000", "104857600"),
                "G3 actual weight was replaced by the fixed declared gas limit")
    require(cost["storage_writes"]["value"] == str(raw["storage"]["write_call_count"]),
            "storage_writes differs from trace evidence")
    require(cost["rpc_response_ms"]["value"] == str(raw["rpc_response_ms"]),
            "rpc_response_ms differs from probe evidence")
    if baseline in {"B1", "G3"}:
        require(architecture["contract_storage_writes"]["value"] ==
                str(raw["storage"]["contract_write_call_count"]),
                "contract storage writes differ from trace evidence")
    if baseline in {"B2", "G3"}:
        require(architecture["runtime_storage_writes"]["value"] ==
                str(raw["storage"]["runtime_write_call_count"]),
                "runtime storage writes differ from trace evidence")
    if baseline == "G3":
        require(architecture["projection_storage_writes"]["value"] ==
                str(raw["storage"]["projection_write_call_count"]),
                "projection storage writes differ from trace evidence")


def validate_unit(
    unit_root: Path,
    preflight_path: Path,
    candidate_path: Path,
    candidate_sha256: str,
) -> dict[str, Any]:
    preflight = load(preflight_path)
    require(preflight.get("experiment_family") == "E6", "preflight is not E6")
    require(preflight["inputs"]["seed"] == 20260726, "E6 seed differs")
    require(preflight["inputs"]["repetitions"] == 10, "E6 repetitions differ")
    require(preflight["inputs"]["workload_id"] == "formal-cost-medium-v2",
            "E6 workload must use actual-metric v2")
    candidate = validate_candidate(preflight, candidate_path, candidate_sha256)
    unit = load(unit_root / "e6-unit-cost.json")
    require(unit.get("schema_version") == 2, "E6 unit cost schema must be v2")
    require(unit["run_id"] == preflight["run_id"], "unit/preflight run ID mismatch")
    baseline = preflight["baseline"]["baseline_id"]
    require(unit["baseline_id"] == baseline, "unit baseline differs")
    require(unit["candidate"]["candidate_id"] == candidate["candidate_id"], "unit candidate differs")
    require(unit["candidate"]["manifest_sha256"] == candidate_sha256, "unit candidate hash differs")
    require(unit["retained"] is True and unit["anomalies"] == [], "unit retention/anomaly state differs")
    require(unit["cost_boundary"]["amortized_setup_denominator_orders"] == 1,
            "unit setup amortization denominator differs")
    require(unit["per_order"]["phase_call_counts"] == {
        "create_order": 1, "fund_order": 1, "settle": 1,
    }, "per-order phase calls differ")
    require(unit["per_order"]["transaction_count"] == 3, "per-order transaction count differs")
    attempts = sorted(path for path in unit_root.glob("attempt-*") if path.is_dir())
    require(len(attempts) == 3, "E6 unit must contain three measured attempts")
    phases: list[str] = []
    trace_hashes: list[str] = []
    for attempt in attempts:
        result = load(attempt / "result.json")
        measurement = load(attempt / "e6-measurement.json")
        block = load(attempt / "block-events.json")
        require(result["claim_boundary"] == CLAIM_BOUNDARY, "E6 result claim boundary differs")
        require(measurement["retained"] is True and measurement["warm_up"] is False,
                "E6 attempt retention/warm-up state differs")
        require(measurement["anomaly_flags"] == [], "E6 attempt anomaly flags differ")
        require(measurement["transaction_hash"] == block["transaction_hash"],
                "measurement/block transaction hash differs")
        require(measurement["included_block_hash"] == block["included_block_hash"],
                "measurement/block included hash differs")
        require(measurement["extrinsic_index"] == block["extrinsic_index"],
                "measurement/block extrinsic index differs")
        raw = measurement["raw_measurement"]
        require(raw.get("estimated_partial_fee") is not None,
                "declared paymentInfo fee estimate is missing")
        require(raw.get("declared_payment_weight") is not None,
                "declared paymentInfo weight is missing")
        require(raw.get("metric_source") == {
            "fee": {"section": "transactionPayment", "method": "TransactionFeePaid"},
            "weight": {"section": "system", "method": "ExtrinsicSuccess"},
        }, "actual metric source metadata differs")
        validate_metrics(result, measurement, block, baseline)
        phases.append(result["request"]["phase"])
        trace_hashes.append(measurement["raw_measurement"]["storage_trace_response_sha256"])
    require(phases == ["create_order", "fund_order", "settle"], "E6 phase sequence differs")
    return {
        "schema_id": "assetxchain-exp2-phase4-e6-unit-validation",
        "schema_version": 2,
        "status": "PASS",
        "run_id": preflight["run_id"],
        "baseline_id": baseline,
        "scenario_id": unit["scenario_id"],
        "repetition": unit["repetition"],
        "attempts": 3,
        "setup_transactions": unit["setup"]["transaction_count"],
        "trace_response_sha256": trace_hashes,
        "candidate_id": candidate["candidate_id"],
        "candidate_manifest_sha256": candidate_sha256,
        "claim_boundary": CLAIM_BOUNDARY,
    }


def validate_batch(raw_root: Path, candidate_path: Path, candidate_sha256: str) -> dict[str, Any]:
    preflight_path = raw_root / "manifest.preflight.json"
    preflight = load(preflight_path)
    candidate = validate_candidate(preflight, candidate_path, candidate_sha256)
    seal = load(raw_root / "manifest.seal.json")
    require(seal.get("status") == "completed", "E6 batch seal is not completed")
    require(seal["checker"]["status"] == "PASS", "generic correctness checker did not pass")
    require(seal["attempt_counts"] == {
        "planned_units": 30, "completed_units": 30,
        "planned_attempts": 90, "completed_attempts": 90,
    }, "E6 batch unit/attempt counts differ")
    units = []
    for item in preflight["planned_execution"]:
        unit_root = raw_root / "attempts" / item["scenario_id"] / f"r-{item['repetition']:03d}"
        stored_report = load(unit_root / "e6-unit-validation.json")
        report = validate_unit(
            unit_root, preflight_path, candidate_path, candidate_sha256,
        )
        require(stored_report == report, f"stored/recomputed unit validation differs: {unit_root}")
        require(report["scenario_id"] == item["scenario_id"] and report["repetition"] == item["repetition"],
                "unit validation/planned execution mismatch")
        units.append(report)
    require(len(units) == 30, "E6 batch does not contain 30 validated units")
    return {
        "schema_id": "assetxchain-exp2-phase4-e6-batch-validation",
        "schema_version": 2,
        "status": "PASS",
        "run_id": preflight["run_id"],
        "baseline_id": preflight["baseline"]["baseline_id"],
        "candidate_id": candidate["candidate_id"],
        "candidate_manifest_sha256": candidate_sha256,
        "units": 30,
        "attempts": 90,
        "repetitions": 10,
        "scenarios": ["LEG-META-01", "LEG-ISSUE-01", "LEG-RESALE-01"],
        "warm_up_policy": "none; every R=10 sample is a retained fresh-chain cold-start observation",
        "anomaly_count": 0,
        "claim_boundary": CLAIM_BOUNDARY,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("unit", "batch"))
    parser.add_argument("root", type=Path)
    parser.add_argument("--preflight", type=Path)
    parser.add_argument("--candidate-manifest", type=Path, required=True)
    parser.add_argument("--candidate-sha256", required=True)
    args = parser.parse_args()
    if args.mode == "unit":
        require(args.preflight is not None, "unit mode requires --preflight")
        report = validate_unit(
            args.root, args.preflight, args.candidate_manifest, args.candidate_sha256,
        )
    else:
        report = validate_batch(args.root, args.candidate_manifest, args.candidate_sha256)
    print(json.dumps(report, sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except E6ValidationError as error:
        print(f"ERROR: {error}", file=__import__("sys").stderr)
        raise SystemExit(2)
