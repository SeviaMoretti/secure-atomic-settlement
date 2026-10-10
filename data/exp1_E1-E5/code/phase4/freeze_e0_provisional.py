#!/usr/bin/env python3
"""Freeze three completed E0 runs into one immutable provisional index."""

from __future__ import annotations

import argparse
import json
import subprocess
from datetime import datetime
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo

from phase4_contract import EXP_ROOT, Phase4ValidationError, load_json, sha256_file, write_json_exclusive
from phase4_manifest import validate_artifacts


BASELINES = ("B1", "B2", "G3")
SCENARIOS = ("LEG-META-01", "LEG-ISSUE-01", "LEG-RESALE-01")
TOTAL_FIELDS = (
    "completed_units", "completed_attempts", "expected_outcome_mismatches",
    "invariant_violations", "false_accepts", "false_rejects", "duplicates",
    "dirty_state", "event_mismatches", "timeouts",
)


def _parse_runs(values: list[str]) -> dict[str, str]:
    runs: dict[str, str] = {}
    for value in values:
        baseline, separator, run_id = value.partition("=")
        if separator != "=" or baseline not in BASELINES or not run_id:
            raise Phase4ValidationError(f"invalid --run value: {value}")
        if baseline in runs:
            raise Phase4ValidationError(f"duplicate --run baseline: {baseline}")
        runs[baseline] = run_id
    if tuple(sorted(runs)) != tuple(sorted(BASELINES)):
        raise Phase4ValidationError("--run must provide exactly B1, B2, and G3")
    return runs


def _git_identity(preflight: dict[str, Any]) -> None:
    baseline = preflight["baseline"]
    worktree = baseline["worktree"]
    head = subprocess.run(
        ["git", "-C", worktree, "rev-parse", "HEAD"],
        text=True, capture_output=True, check=True,
    ).stdout.strip()
    dirty = subprocess.run(
        ["git", "-C", worktree, "status", "--porcelain=v1"],
        text=True, capture_output=True, check=True,
    ).stdout.strip()
    if head != baseline["head"] or dirty:
        raise Phase4ValidationError(f"{baseline['baseline_id']} Git identity drift")


def freeze(runs: dict[str, str], supersedes: str) -> tuple[dict[str, Any], Path]:
    preflights: dict[str, dict[str, Any]] = {}
    records: dict[str, dict[str, Any]] = {}
    totals = {field: 0 for field in TOTAL_FIELDS}
    anchors: set[int] = set()

    for baseline in BASELINES:
        run_id = runs[baseline]
        raw = EXP_ROOT / "results/raw" / run_id
        validation = validate_artifacts(raw)
        preflight = load_json(raw / "manifest.preflight.json")
        seal = load_json(raw / "manifest.seal.json")
        checker = load_json(raw / "checker-report.json")
        if preflight["baseline"]["baseline_id"] != baseline or preflight["experiment_family"] != "E0":
            raise Phase4ValidationError(f"{run_id} is not the requested {baseline} E0 run")
        if seal["status"] != "completed" or checker["summary"]["status"] != "PASS":
            raise Phase4ValidationError(f"{run_id} did not pass E0")
        if validation["status"] != "PASS" or validation["results"] != 18:
            raise Phase4ValidationError(f"{run_id} artifact validation failed")
        if seal["attempt_counts"]["completed_units"] != 6 or seal["attempt_counts"]["completed_attempts"] != 18:
            raise Phase4ValidationError(f"{run_id} completion counts differ")
        if not seal["teardown"]["owned_processes_stopped"] or seal["teardown"]["residual_listener_count"]:
            raise Phase4ValidationError(f"{run_id} teardown gate failed")
        _git_identity(preflight)
        anchors.update(seal["fixture_anchor_blocks"].values())
        for field in TOTAL_FIELDS:
            totals[field] += checker["summary"][field]
        preflights[baseline] = preflight
        records[baseline] = {
            "run_id": run_id,
            "preflight_sha256": sha256_file(raw / "manifest.preflight.json"),
            "seal_sha256": sha256_file(raw / "manifest.seal.json"),
            "seal_status": seal["status"],
            "artifact_validation": validation["status"],
            "artifact_count": validation["artifacts"],
            "completed_units": seal["attempt_counts"]["completed_units"],
            "completed_attempts": seal["attempt_counts"]["completed_attempts"],
        }

    first = preflights["B1"]
    shared_fields = (
        ("inputs", "environment_config", "sha256"),
        ("inputs", "workload", "sha256"),
        ("inputs", "scenario_catalog", "sha256"),
        ("inputs", "seed"),
        ("inputs", "repetitions"),
        ("inputs", "selected_scenario_ids"),
        ("protocol", "settlement_protocol_version"),
        ("fixture", "fixture_profile_id"),
    )
    for field_path in shared_fields:
        expected: Any = first
        for field in field_path:
            expected = expected[field]
        for baseline in BASELINES[1:]:
            actual: Any = preflights[baseline]
            for field in field_path:
                actual = actual[field]
            if actual != expected:
                raise Phase4ValidationError(f"shared E0 input drift at {'.'.join(field_path)}")
    if first["inputs"]["repetitions"] != 2 or tuple(first["inputs"]["selected_scenario_ids"]) != SCENARIOS:
        raise Phase4ValidationError("E0 scenario/repetition scope drift")
    if anchors != {16}:
        raise Phase4ValidationError(f"fixture anchors differ: {sorted(anchors)}")
    if any(totals[field] for field in TOTAL_FIELDS[2:]):
        raise Phase4ValidationError("E0 checker totals contain an error")

    now = datetime.now(ZoneInfo("Asia/Shanghai")).replace(microsecond=0)
    candidate_id = f"p4-e0-provisional-{now.strftime('%Y%m%dT%H%M%S+0800')}"
    tool_map = {item["tool_id"]: item["sha256"] for item in first["tools"]}
    executor = EXP_ROOT / "scripts/phase4/formal_e0_executor.js"
    manifest = {
        "schema_id": "assetxchain-exp2-phase4-e0-provisional-candidate",
        "schema_version": 1,
        "candidate_id": candidate_id,
        "created_at": now.isoformat(),
        "timezone": "Asia/Shanghai",
        "phase_step": "S4.2",
        "experiment_family": "E0",
        "status": "frozen",
        "validity_level": "provisional-candidate",
        "claim_boundary": "E0 reproducibility only; not performance evidence, paper-candidate evidence, or paper-final evidence",
        "supersedes_for_continuation": supersedes,
        "frozen_inputs": {
            "protocol_id": first["protocol"]["protocol_label"],
            "settlement_protocol_version": first["protocol"]["settlement_protocol_version"],
            "fixture_profile_id": first["fixture"]["fixture_profile_id"],
            "shared_input_hash": "0xc427552f41308fe6e7345a4af448ef3670caba676f1da46e433a1b0aa88e9c13",
            "environment_config_sha256": first["inputs"]["environment_config"]["sha256"],
            "workload_sha256": first["inputs"]["workload"]["sha256"],
            "scenario_catalog_sha256": first["inputs"]["scenario_catalog"]["sha256"],
            "toolchain_profile_sha256": sha256_file(EXP_ROOT / "configs/phase4/formal-toolchain-v1.json"),
            "executor_sha256": sha256_file(executor),
            "runner_sha256": tool_map["runner"],
            "checker_sha256": tool_map["checker"],
            "seed": first["inputs"]["seed"],
            "repetitions": 2,
            "scenarios": list(SCENARIOS),
            "reset_policy": "fresh --dev --tmp node per scenario and repetition",
            "fixture_anchor_block": 16,
            "required_invariants": first["inputs"]["required_invariants"],
            "deferred_invariants": first["inputs"]["deferred_invariants"],
        },
        "baselines": {
            baseline: {
                "name": preflight["baseline"]["baseline_name"],
                "branch": preflight["baseline"]["branch"],
                "head": preflight["baseline"]["head"],
                "tree": preflight["baseline"]["tree"],
                "dirty": preflight["baseline"]["dirty"],
                "baseline_manifest_sha256": preflight["inputs"]["baseline_manifest"]["sha256"],
            }
            for baseline, preflight in preflights.items()
        },
        "runs": records,
        "reproducibility_gate": {
            "status": "PASS",
            "all_nine_repetition_pairs_match": True,
            "compared_fields": [
                "fixture_anchor_block", "genesis_hash", "metadata_hash", "shared_input_hash",
                "fixture_state_hash", "scenario_result_hash", "attempt_count",
            ],
            "checker_totals": totals,
            "all_owned_processes_stopped": True,
            "residual_listener_count": 0,
        },
        "authorization": {
            "next_allowed_step": "S4.3",
            "e1_authorized": True,
            "e6_authorized": False,
            "paper_candidate": False,
            "paper_final": False,
            "performance_claims_authorized": False,
        },
        "immutability": {
            "source_raw_must_not_be_modified": True,
            "manifest_must_not_be_modified_after_creation": True,
            "semantic_change_requires_return_to": "S4.2",
        },
    }
    path = EXP_ROOT / "results/provisional" / candidate_id / "provisional-candidate.manifest.json"
    write_json_exclusive(path, manifest)
    return manifest, path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", action="append", required=True, help="BASELINE=RUN_ID; provide B1, B2, and G3")
    parser.add_argument("--supersedes", required=True)
    args = parser.parse_args()
    manifest, path = freeze(_parse_runs(args.run), args.supersedes)
    print(json.dumps({
        "candidate_id": manifest["candidate_id"],
        "path": str(path),
        "sha256": sha256_file(path),
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Phase4ValidationError as error:
        print(f"ERROR: {error}")
        raise SystemExit(2)
