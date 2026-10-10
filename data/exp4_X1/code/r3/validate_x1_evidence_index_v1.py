#!/usr/bin/env python3
"""Independently validate the completed R3/X1 batch, seals, and evidence index."""

from __future__ import annotations

import argparse
import hashlib
import json
from collections import Counter
from datetime import datetime
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo


EXPECTED_PAPER_FINAL_ID = "p4-paper-final-20260807T124151+0800"
EXPECTED_PAPER_FINAL_SHA256 = (
    "48098917d631a3c49ee5eb4a4fd58acb885ca63be99b300ff838ea00ad227ae0"
)
EXPECTED_CLAIM_BOUNDARY = "evolvability-only; no performance or architecture ranking"


class ValidationError(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValidationError(message)


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        value = json.load(handle)
    require(isinstance(value, dict), f"JSON root is not an object: {path}")
    return value


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def file_ref(path: Path) -> dict[str, Any]:
    resolved = path.resolve()
    return {
        "path": str(resolved),
        "bytes": resolved.stat().st_size,
        "sha256": sha256_file(resolved),
    }


class Validator:
    def __init__(self) -> None:
        self.validated: dict[str, str] = {}
        self.validated_bytes = 0

    def validate_ref(self, ref: dict[str, Any], label: str) -> Path:
        require(isinstance(ref, dict), f"invalid file reference for {label}")
        path = Path(str(ref.get("path", ""))).resolve()
        require(path.is_file(), f"missing referenced file for {label}: {path}")
        expected_hash = str(ref.get("sha256", ""))
        require(len(expected_hash) == 64, f"invalid SHA-256 for {label}: {path}")
        if "bytes" in ref:
            require(path.stat().st_size == int(ref["bytes"]),
                    f"byte-count mismatch for {label}: {path}")
        key = str(path)
        if key in self.validated:
            require(self.validated[key] == expected_hash,
                    f"conflicting expected hashes for {path}")
            return path
        actual_hash = sha256_file(path)
        require(actual_hash == expected_hash, f"SHA-256 mismatch for {label}: {path}")
        self.validated[key] = expected_hash
        self.validated_bytes += path.stat().st_size
        return path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--batch", type=Path, required=True)
    parser.add_argument("--index", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    validator = Validator()
    batch_path = args.batch.resolve()
    index_path = args.index.resolve()
    output_path = args.out.resolve()
    require(not output_path.exists(), f"refusing to overwrite validation report: {output_path}")

    batch = load_json(batch_path)
    require(batch.get("schema_id") == "assetxchain-exp2-r3-x1-formal-batch",
            "unexpected batch schema")
    require(batch.get("status") == "completed", "batch is not completed")
    require(batch.get("cell_count") == 36, "batch cell_count is not 36")
    require(batch.get("repetitions_per_cell") == 10,
            "batch repetitions_per_cell is not 10")
    require(batch.get("evidence_level") == "extended-experiment-candidate",
            "unexpected batch evidence level")
    require(batch.get("claim_boundary") == EXPECTED_CLAIM_BOUNDARY,
            "unexpected batch claim boundary")
    batch_cells = batch.get("cells")
    require(isinstance(batch_cells, list) and len(batch_cells) == 36,
            "batch does not contain exactly 36 cells")
    validator.validate_ref(batch["harness_manifest"], "batch harness manifest")

    index = load_json(index_path)
    require(index.get("schema_id") == "assetxchain-exp2-r2-x1-evidence-index",
            "unexpected evidence-index schema")
    require(index.get("status") == "indexed", "evidence index is not indexed")
    require(index.get("cell_count") == 36, "index cell_count is not 36")
    require(index.get("validity_level") == "paper-candidate",
            "unexpected index validity level")
    require(index.get("claim_boundary") == EXPECTED_CLAIM_BOUNDARY,
            "unexpected index claim boundary")
    protected = index.get("protected_paper_final", {})
    require(protected.get("final_id") == EXPECTED_PAPER_FINAL_ID,
            "protected paper-final ID changed")
    require(protected.get("manifest_sha256") == EXPECTED_PAPER_FINAL_SHA256,
            "protected paper-final hash changed")
    require(protected.get("mutable") is False, "protected paper-final is marked mutable")
    index_runs = index.get("runs")
    require(isinstance(index_runs, list) and len(index_runs) == 36,
            "index does not contain exactly 36 runs")

    batch_ids: set[str] = set()
    batch_cell_by_id: dict[str, dict[str, Any]] = {}
    for cell in batch_cells:
        require(isinstance(cell, dict), "batch cell is not an object")
        run_id = str(cell.get("run_id", ""))
        require(run_id and run_id not in batch_ids, f"duplicate batch run ID: {run_id}")
        batch_ids.add(run_id)
        batch_cell_by_id[run_id] = cell
        for key in ("plan", "preflight", "result", "checker", "seal"):
            validator.validate_ref(cell[key], f"batch {run_id} {key}")
        require(Path(str(cell.get("live_root", ""))).resolve().is_dir(),
                f"missing live root for {run_id}")

    index_ids: set[str] = set()
    baseline_counts: Counter[str] = Counter()
    operation_counts: Counter[str] = Counter()
    variant_counts: Counter[str] = Counter()
    checker_pass_count = 0
    completed_seal_count = 0
    ports_closed_count = 0
    invariant_violation_count = 0
    total_repetitions = 0

    for row in index_runs:
        require(isinstance(row, dict), "index run is not an object")
        run_id = str(row.get("run_id", ""))
        require(run_id and run_id not in index_ids, f"duplicate index run ID: {run_id}")
        index_ids.add(run_id)
        require(run_id in batch_cell_by_id, f"index run absent from batch: {run_id}")
        seal_path = validator.validate_ref(row["seal"], f"index seal {run_id}")
        seal = load_json(seal_path)
        require(seal.get("schema_id") == "assetxchain-exp2-r2-x1-run-seal",
                f"unexpected seal schema: {run_id}")
        require(seal.get("status") == "completed", f"seal is not completed: {run_id}")
        require(seal.get("run_id") == run_id, f"seal run ID mismatch: {run_id}")
        require(seal.get("evidence_level") == "extended-experiment-candidate",
                f"unexpected evidence level: {run_id}")
        require(seal.get("claim_boundary") == EXPECTED_CLAIM_BOUNDARY,
                f"unexpected claim boundary: {run_id}")
        require(row.get("baseline_id") == seal.get("baseline_id"),
                f"index/seal baseline mismatch: {run_id}")
        require(row.get("variant_id") == seal.get("variant_id"),
                f"index/seal variant mismatch: {run_id}")
        require(row.get("operation") == seal.get("operation"),
                f"index/seal operation mismatch: {run_id}")
        checks = seal.get("checks", {})
        require(checks.get("live_chain_repetitions") == 10,
                f"seal repetition mismatch: {run_id}")
        require(checks.get("checker_status") == "PASS",
                f"seal checker status is not PASS: {run_id}")
        require(checks.get("invariant_violation_count") == 0,
                f"seal reports invariant violations: {run_id}")
        require(checks.get("ports_closed_after_run") is True,
                f"seal reports open ports: {run_id}")
        require(checks.get("historical_artifacts_modified") is False,
                f"seal reports historical modification: {run_id}")
        validator.validate_ref(seal["preflight"], f"seal preflight {run_id}")
        validator.validate_ref(seal["harness_manifest"], f"seal harness {run_id}")
        for root_key in ("run_root", "result_root"):
            root = seal.get(root_key, {})
            require(Path(str(root.get("path", ""))).resolve().is_dir(),
                    f"missing {root_key}: {run_id}")
            files = root.get("files")
            require(isinstance(files, list) and files,
                    f"empty {root_key} file list: {run_id}")
            for ref in files:
                validator.validate_ref(ref, f"{root_key} {run_id}")

        runner_path = Path(str(seal["run_root"]["path"])) / "runner-manifest.json"
        runner = load_json(runner_path)
        require(runner.get("status") == "PASS", f"runner status is not PASS: {run_id}")
        require(runner.get("formal_evidence_eligible") is True,
                f"runner is not formal-evidence eligible: {run_id}")
        require(runner.get("ports_closed_after_run") is True,
                f"runner ports not closed: {run_id}")
        require(runner.get("run_id") == run_id, f"runner run ID mismatch: {run_id}")

        result_path = Path(str(seal["result_root"]["path"])) / "result.json"
        checker_path = Path(str(seal["result_root"]["path"])) / "checker-report.json"
        result = load_json(result_path)
        checker = load_json(checker_path)
        require(result.get("run", {}).get("run_id") == run_id,
                f"result run ID mismatch: {run_id}")
        repetitions = int(result.get("run", {}).get("repetition", 0))
        require(repetitions == 10, f"result repetition mismatch: {run_id}")
        violations = int(result.get("invariants", {}).get("violation_count", -1))
        require(violations == 0, f"result invariant violations: {run_id}")
        require(checker.get("status") == "PASS", f"checker status is not PASS: {run_id}")
        require(checker.get("failures") == [], f"checker failures are not empty: {run_id}")
        require(checker.get("violation_counts", {}).get("invariant") == 0,
                f"checker invariant violations: {run_id}")
        require(checker.get("unit_counts", {}).get("repetitions") == 10,
                f"checker repetition mismatch: {run_id}")

        cell = batch_cell_by_id[run_id]
        require(cell.get("baseline_id") == seal.get("baseline_id"),
                f"batch/seal baseline mismatch: {run_id}")
        require(cell.get("variant_id") == seal.get("variant_id"),
                f"batch/seal variant mismatch: {run_id}")
        require(cell.get("operation") == seal.get("operation"),
                f"batch/seal operation mismatch: {run_id}")
        baseline_counts[str(seal["baseline_id"])] += 1
        operation_counts[str(seal["operation"])] += 1
        variant_counts[str(seal["variant_id"])] += 1
        checker_pass_count += 1
        completed_seal_count += 1
        ports_closed_count += 1
        invariant_violation_count += violations
        total_repetitions += repetitions

    require(index_ids == batch_ids, "batch and index run-ID sets differ")
    require(baseline_counts == Counter({"B1": 12, "B2": 12, "G3": 12}),
            f"unexpected baseline matrix: {dict(baseline_counts)}")
    require(operation_counts == Counter({"TransferMeta": 12, "IssueRight": 12,
                                         "TransferRight": 12}),
            f"unexpected operation matrix: {dict(operation_counts)}")

    report = {
        "schema_id": "assetxchain-exp2-r3-x1-evidence-validation",
        "schema_version": 1,
        "status": "PASS",
        "created_at": datetime.now(ZoneInfo("Asia/Shanghai")).replace(microsecond=0).isoformat(),
        "timezone": "Asia/Shanghai",
        "experiment_id": "X1",
        "evidence_level": "extended-experiment-candidate",
        "index_validity_level": "paper-candidate",
        "claim_boundary": EXPECTED_CLAIM_BOUNDARY,
        "batch": file_ref(batch_path),
        "evidence_index": file_ref(index_path),
        "validator": file_ref(Path(__file__)),
        "checks": {
            "batch_cells": len(batch_ids),
            "index_runs": len(index_ids),
            "completed_seals": completed_seal_count,
            "checker_passes": checker_pass_count,
            "empty_checker_failure_lists": checker_pass_count,
            "ports_closed_after_run": ports_closed_count,
            "invariant_violation_count": invariant_violation_count,
            "total_live_repetitions": total_repetitions,
            "unique_referenced_files_hashed": len(validator.validated),
            "unique_referenced_bytes_hashed": validator.validated_bytes,
            "batch_index_run_ids_equal": True,
            "protected_paper_final_unchanged": True,
        },
        "matrix": {
            "baseline_counts": dict(sorted(baseline_counts.items())),
            "operation_counts": dict(sorted(operation_counts.items())),
            "variant_counts": dict(sorted(variant_counts.items())),
        },
        "protected_paper_final": {
            "final_id": EXPECTED_PAPER_FINAL_ID,
            "manifest_sha256": EXPECTED_PAPER_FINAL_SHA256,
            "mutable": False,
        },
    }
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with output_path.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(report, handle, ensure_ascii=True, indent=2, sort_keys=True)
        handle.write("\n")
    print(json.dumps({"status": "PASS", "report": str(output_path),
                      "checks": report["checks"]}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
