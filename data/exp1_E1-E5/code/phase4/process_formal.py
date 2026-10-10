"""Deterministically derive Phase 4 processed rows and reports from sealed raw."""

from __future__ import annotations

import argparse
import csv
import io
import json
import os
from collections import Counter
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

from phase4_contract import (
    TOOLING_VERSION,
    Phase4ValidationError,
    canonical_json_bytes,
    load_json,
    sha256_file,
)
from phase4_manifest import validate_artifacts


def _write_bytes(path: Path, payload: bytes) -> None:
    if path.exists():
        if path.read_bytes() != payload:
            raise Phase4ValidationError(f"existing deterministic output differs: {path}")
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o444)
    with os.fdopen(descriptor, "wb") as handle:
        handle.write(payload)
        handle.flush()
        os.fsync(handle.fileno())


def _result_rows(raw_root: Path) -> list[dict[str, Any]]:
    rows = []
    for path in raw_root.glob("attempts/*/r-*/attempt-*/result.json"):
        result = load_json(path)
        row = {
            "run_id": result["run"]["run_id"],
            "baseline_id": result["run"]["baseline_id"],
            "validity_level": result["run"]["validity_level"],
            "scenario_id": result["request"]["scenario_id"],
            "repetition": result["run"]["repetition"],
            "attempt_id": result["request"]["attempt_id"],
            "submission_sequence": result["outcome"]["submission_sequence"],
            "phase": result["request"]["phase"],
            "operation": result["request"]["operation"],
            "expected_outcome": result["request"]["expected_outcome"],
            "actual_outcome": result["outcome"]["protocol_outcome"],
            "common_error_code": result["outcome"]["common_error_code"],
            "included_block": result["outcome"]["included_block"],
            "extrinsic_index": result["outcome"]["extrinsic_index"],
            "event_index": result["outcome"]["event_index"],
            "settlement_record_count": result["settlement"]["settlement_record_count"],
            "success_event_count": result["events"]["success_event_count"],
            "expected_outcome_match": result["correctness"]["expected_outcome_match"],
            "invariant_violation_count": result["correctness"]["invariant_violation_count"],
            "timeout_count": result["correctness"]["timeout_count"],
            "cost": result["cost"],
            "architecture_cost": result["architecture_cost"],
            "raw_result_path": path.relative_to(raw_root).as_posix(),
            "raw_result_sha256": sha256_file(path),
        }
        rows.append(row)
    return sorted(rows, key=lambda row: (row["scenario_id"], row["repetition"], row["submission_sequence"], row["attempt_id"]))


def _test_gate_rows(raw_root: Path) -> list[dict[str, Any]]:
    rows = []
    for path in raw_root.glob("attempts/*/r-*/attempt-*/test-gate.json"):
        evidence = load_json(path)
        rows.append({
            "run_id": evidence["run_id"],
            "baseline_id": evidence["baseline_id"],
            "scenario_id": evidence["scenario_id"],
            "repetition": evidence["repetition"],
            "attempt_id": evidence["attempt_id"],
            "exit_code": evidence["exit_code"],
            "q_all_equal": evidence["q_all_equal"],
            "raw_test_gate_path": path.relative_to(raw_root).as_posix(),
            "raw_test_gate_sha256": sha256_file(path),
        })
    return sorted(rows, key=lambda row: (row["scenario_id"], row["repetition"], row["attempt_id"]))


def _csv_bytes(rows: Sequence[Mapping[str, Any]], fields: Sequence[str]) -> bytes:
    stream = io.StringIO(newline="")
    writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
    writer.writeheader()
    for row in rows:
        writer.writerow({field: row.get(field) for field in fields})
    return stream.getvalue().encode("utf-8")


def _report(preflight: Mapping[str, Any], seal: Mapping[str, Any], summary: Mapping[str, Any]) -> bytes:
    lines = [
        f"# Formal Run {preflight['run_id']}",
        "",
        f"- Baseline: `{preflight['baseline']['baseline_id']} {preflight['baseline']['baseline_name']}`",
        f"- Family: `{preflight['experiment_family']}`",
        f"- Validity: `{preflight['validity_level']}` (not paper-candidate or paper-final)",
        f"- Commit: `{preflight['baseline']['head']}`; dirty: `{str(preflight['baseline']['dirty']).lower()}`",
        f"- Config: `{preflight['inputs']['workload']['path']}`; seed: `{preflight['inputs']['seed']}`; repetitions: `{preflight['inputs']['repetitions']}`",
        f"- Seal/checker: `{seal['status']}` / `{seal['checker']['status']}`",
        "",
        "| Metric | Count |",
        "| --- | ---: |",
        f"| Results | {summary['result_count']} |",
        f"| Test gates | {summary['test_gate_count']} |",
        f"| Accepted | {summary['outcome_counts'].get('accepted', 0)} |",
        f"| Rejected | {summary['outcome_counts'].get('rejected', 0)} |",
        f"| Timeouts | {summary['outcome_counts'].get('timeout', 0)} |",
        f"| Checker failures | {summary['checker_failure_count']} |",
        "",
        "This report is a deterministic derivative of one sealed raw run. It does not promote the run to paper-candidate or paper-final.",
        "",
    ]
    return "\n".join(lines).encode("utf-8")


def process_run(raw_root: Path) -> dict[str, Any]:
    artifact_status = validate_artifacts(raw_root)
    preflight = load_json(raw_root / "manifest.preflight.json")
    seal = load_json(raw_root / "manifest.seal.json")
    checker = load_json(raw_root / "checker-report.json")
    if seal["status"] != "completed" or seal["checker"]["status"] != "PASS" or checker["summary"]["status"] != "PASS":
        raise Phase4ValidationError("only a completed checker-PASS sealed run may be processed")
    if preflight["validity_level"] != "provisional-candidate":
        raise Phase4ValidationError("processor accepts only the Phase 4 provisional raw level")
    rows = _result_rows(raw_root)
    test_gates = _test_gate_rows(raw_root)
    outcomes = Counter(row["actual_outcome"] for row in rows)
    errors = Counter(row["common_error_code"] for row in rows if row["common_error_code"] is not None)
    summary = {
        "schema_id": "assetxchain-exp2-phase4-processed-summary",
        "schema_version": 1,
        "tooling_version": TOOLING_VERSION,
        "source_run_id": preflight["run_id"],
        "source_preflight_sha256": sha256_file(raw_root / "manifest.preflight.json"),
        "source_seal_sha256": sha256_file(raw_root / "manifest.seal.json"),
        "source_checker_sha256": sha256_file(raw_root / "checker-report.json"),
        "baseline_id": preflight["baseline"]["baseline_id"],
        "experiment_family": preflight["experiment_family"],
        "validity_level": preflight["validity_level"],
        "result_count": len(rows),
        "test_gate_count": len(test_gates),
        "outcome_counts": dict(sorted(outcomes.items())),
        "error_counts": dict(sorted(errors.items())),
        "checker_failure_count": len(checker["failures"]),
        "artifact_count": artifact_status["artifacts"],
    }
    processed_root = Path(preflight["paths"]["processed"])
    attempts_payload = b"".join(canonical_json_bytes(row) for row in rows)
    test_gate_payload = b"".join(canonical_json_bytes(row) for row in test_gates)
    csv_fields = (
        "run_id", "baseline_id", "validity_level", "scenario_id", "repetition",
        "attempt_id", "submission_sequence", "phase", "operation",
        "expected_outcome", "actual_outcome", "common_error_code",
        "included_block", "extrinsic_index", "event_index",
        "settlement_record_count", "success_event_count",
        "expected_outcome_match", "invariant_violation_count", "timeout_count",
        "raw_result_path", "raw_result_sha256",
    )
    outputs = {
        processed_root / "summary.json": canonical_json_bytes(summary),
        processed_root / "attempts.jsonl": attempts_payload,
        processed_root / "test-gates.jsonl": test_gate_payload,
        processed_root / "correctness.csv": _csv_bytes(rows, csv_fields),
        Path(preflight["paths"]["report"]): _report(preflight, seal, summary),
    }
    for path, payload in outputs.items():
        _write_bytes(path, payload)
    output_hashes = {str(path): sha256_file(path) for path in sorted(outputs, key=str)}
    derivation = {
        "schema_id": "assetxchain-exp2-phase4-derivation",
        "schema_version": 1,
        "tooling_version": TOOLING_VERSION,
        "source_run_id": preflight["run_id"],
        "source_hashes": {
            "preflight": summary["source_preflight_sha256"],
            "seal": summary["source_seal_sha256"],
            "checker": summary["source_checker_sha256"],
        },
        "output_hashes": output_hashes,
    }
    derivation_path = processed_root / "derivation.json"
    _write_bytes(derivation_path, canonical_json_bytes(derivation))
    return {"status": "PASS", "run_id": preflight["run_id"], "processed": str(processed_root), "report": preflight["paths"]["report"], "output_hashes": {**output_hashes, str(derivation_path): sha256_file(derivation_path)}}


def aggregate(processed_roots: Sequence[Path], output_root: Path) -> dict[str, Any]:
    if len(processed_roots) != 3:
        raise Phase4ValidationError("main aggregate requires exactly B1/B2/G3 processed roots")
    summaries = [load_json(root / "summary.json") for root in processed_roots]
    if {item["baseline_id"] for item in summaries} != {"B1", "B2", "G3"}:
        raise Phase4ValidationError("main aggregate may contain only one B1, B2, and G3 run")
    shared = {(item["experiment_family"], item["validity_level"]) for item in summaries}
    if len(shared) != 1 or next(iter(shared))[1] != "provisional-candidate":
        raise Phase4ValidationError("aggregate family/validity scope differs or is not provisional-candidate")
    summaries.sort(key=lambda item: ("B1", "B2", "G3").index(item["baseline_id"]))
    rows = [{
        "baseline_id": item["baseline_id"],
        "source_run_id": item["source_run_id"],
        "result_count": item["result_count"],
        "test_gate_count": item["test_gate_count"],
        "accepted": item["outcome_counts"].get("accepted", 0),
        "rejected": item["outcome_counts"].get("rejected", 0),
        "timeouts": item["outcome_counts"].get("timeout", 0),
        "checker_failure_count": item["checker_failure_count"],
    } for item in summaries]
    comparison = {
        "schema_id": "assetxchain-exp2-phase4-main-comparison",
        "schema_version": 1,
        "tooling_version": TOOLING_VERSION,
        "scope": "main-experiment-B1-B2-G3-only",
        "experiment_family": summaries[0]["experiment_family"],
        "validity_level": "provisional-candidate",
        "rows": rows,
    }
    _write_bytes(output_root / "comparison.json", canonical_json_bytes(comparison))
    fields = ("baseline_id", "source_run_id", "result_count", "test_gate_count", "accepted", "rejected", "timeouts", "checker_failure_count")
    _write_bytes(output_root / "comparison.csv", _csv_bytes(rows, fields))
    return {"status": "PASS", "output": str(output_root), "baselines": [row["baseline_id"] for row in rows]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    process_parser = subparsers.add_parser("process")
    process_parser.add_argument("raw_root", type=Path)
    aggregate_parser = subparsers.add_parser("aggregate")
    aggregate_parser.add_argument("output_root", type=Path)
    aggregate_parser.add_argument("processed_roots", nargs=3, type=Path)
    args = parser.parse_args()
    result = process_run(args.raw_root) if args.command == "process" else aggregate(args.processed_roots, args.output_root)
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Phase4ValidationError as error:
        print(f"ERROR: {error}", file=os.sys.stderr)
        raise SystemExit(2)
