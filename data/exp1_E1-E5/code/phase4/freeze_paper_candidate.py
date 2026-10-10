#!/usr/bin/env python3
"""Validate the complete E0-E5 correctness chain and freeze S4.8."""

from __future__ import annotations

import argparse
import json
import socket
import stat
import subprocess
from collections import Counter
from datetime import datetime
from pathlib import Path
from typing import Any, Mapping
from zoneinfo import ZoneInfo

from phase4_checker import check_run, seal_summary
from phase4_contract import (
    BASELINES,
    EXP_ROOT,
    PORTS,
    Phase4ValidationError,
    canonical_json_bytes,
    load_catalog,
    load_json,
    schema_validate,
    select_scenarios,
    sha256_bytes,
    sha256_file,
    write_json_exclusive,
)
from phase4_manifest import validate_artifacts


FAMILIES = ("E1", "E2", "E3", "E4", "E5")
ZERO_COUNTERS = (
    "expected_outcome_mismatches",
    "invariant_violations",
    "false_accepts",
    "false_rejects",
    "duplicates",
    "dirty_state",
    "event_mismatches",
    "timeouts",
)
SUMMARY_COUNTERS = ("completed_units", "completed_attempts", *ZERO_COUNTERS)
CANDIDATE_SCHEMA = Path("schemas/phase4/paper-candidate.schema.json")
EXPECTED_UNITS = 705
EXPECTED_ATTEMPTS = 1110
EXPECTED_SETTLEMENT_RECORDS = 75
STABLE_ENVIRONMENT_FIELDS = (
    "os", "kernel", "wsl_version", "cpu_logical", "rustc", "cargo",
    "python", "protoc", "nodejs", "npm",
)


def _parse_runs(values: list[str]) -> dict[str, dict[str, str]]:
    runs = {family: {} for family in FAMILIES}
    for value in values:
        key, separator, run_id = value.partition("=")
        family, colon, baseline = key.partition(":")
        if separator != "=" or colon != ":" or family not in FAMILIES or baseline not in BASELINES or not run_id:
            raise Phase4ValidationError(f"invalid --run value: {value}")
        if baseline in runs[family]:
            raise Phase4ValidationError(f"duplicate run binding: {family}:{baseline}")
        runs[family][baseline] = run_id
    missing = [f"{family}:{baseline}" for family in FAMILIES for baseline in BASELINES if baseline not in runs[family]]
    if missing:
        raise Phase4ValidationError(f"missing run bindings: {', '.join(missing)}")
    return runs


def _git(worktree: str, *args: str) -> str:
    return subprocess.run(
        ["git", "-C", worktree, *args], text=True, capture_output=True, check=True,
    ).stdout.strip()


def _validate_git(preflight: Mapping[str, Any]) -> None:
    baseline = preflight["baseline"]
    worktree = baseline["worktree"]
    actual = {
        "head": _git(worktree, "rev-parse", "HEAD"),
        "tree": _git(worktree, "rev-parse", "HEAD^{tree}"),
        "branch": _git(worktree, "branch", "--show-current"),
        "dirty": bool(_git(worktree, "status", "--porcelain=v1")),
    }
    expected = {key: baseline[key] for key in actual}
    if actual != expected or actual["dirty"]:
        raise Phase4ValidationError(f"{baseline['baseline_id']} Git identity drift: {actual}")


def _ports_open() -> list[int]:
    open_ports = []
    for port in PORTS:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as handle:
            handle.settimeout(0.25)
            if handle.connect_ex(("127.0.0.1", port)) == 0:
                open_ports.append(port)
    return open_ports


def _common_identity(preflight: Mapping[str, Any]) -> dict[str, Any]:
    tools = [item for item in preflight["tools"] if item["tool_id"] != "baseline-adapter"]
    environment = {field: preflight["environment"][field] for field in STABLE_ENVIRONMENT_FIELDS}
    return {
        "protocol": preflight["protocol"],
        "fixture": preflight["fixture"],
        "environment_config": preflight["inputs"]["environment_config"],
        "workload": preflight["inputs"]["workload"],
        "scenario_catalog": preflight["inputs"]["scenario_catalog"],
        "seed": preflight["inputs"]["seed"],
        "required_invariants": preflight["inputs"]["required_invariants"],
        "deferred_invariants": preflight["inputs"]["deferred_invariants"],
        "tooling_version": preflight["tooling_version"],
        "common_tools": tools,
        "executor_artifacts": preflight["execution"]["executor_artifacts"],
        "executor_command": preflight["execution"]["executor_command"],
        "chain_mode": preflight["execution"]["chain_mode"],
        "ports": preflight["execution"]["ports"],
        "reset_policy": preflight["execution"]["reset_policy"],
        "teardown_policy": preflight["execution"]["teardown_policy"],
        "environment": environment,
    }


def _baseline_identity(preflight: Mapping[str, Any]) -> dict[str, Any]:
    baseline = preflight["baseline"]
    adapter = [item for item in preflight["tools"] if item["tool_id"] == "baseline-adapter"]
    if len(adapter) != 1:
        raise Phase4ValidationError(f"{baseline['baseline_id']} must freeze exactly one adapter")
    return {
        "baseline_id": baseline["baseline_id"],
        "name": baseline["baseline_name"],
        "profile_version": baseline["baseline_profile_version"],
        "worktree": baseline["worktree"],
        "branch": baseline["branch"],
        "head": baseline["head"],
        "tree": baseline["tree"],
        "parent": baseline["parent"],
        "dirty": baseline["dirty"],
        "formal_base_commit": baseline["formal_base_commit"],
        "formal_base_is_ancestor": baseline["formal_base_is_ancestor"],
        "baseline_manifest": preflight["inputs"]["baseline_manifest"],
        "baseline_adapter": adapter[0],
        "node_command": preflight["execution"]["node_command"],
    }


def _validate_run(run_id: str, family: str, baseline_id: str) -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    raw = EXP_ROOT / "results/raw" / run_id
    validation = validate_artifacts(raw)
    preflight = load_json(raw / "manifest.preflight.json")
    seal = load_json(raw / "manifest.seal.json")
    stored_checker = load_json(raw / "checker-report.json")
    rechecked = check_run(raw)
    if preflight["experiment_family"] != family or preflight["baseline"]["baseline_id"] != baseline_id:
        raise Phase4ValidationError(f"{run_id} identity differs from {family}:{baseline_id}")
    if preflight["claim_boundary"] != "formal-correctness-only":
        raise Phase4ValidationError(f"{run_id} claim boundary is not correctness-only")
    if seal["status"] != "completed" or validation["status"] != "PASS":
        raise Phase4ValidationError(f"{run_id} is not a completed artifact-valid run")
    if stored_checker != rechecked or rechecked["summary"]["status"] != "PASS" or rechecked["failures"]:
        raise Phase4ValidationError(f"{run_id} independent checker re-run differs or fails")
    if seal["checker"] != seal_summary(rechecked):
        raise Phase4ValidationError(f"{run_id} seal/checker summary mismatch")
    counts = seal["attempt_counts"]
    if counts["planned_units"] != counts["completed_units"] or counts["planned_attempts"] != counts["completed_attempts"]:
        raise Phase4ValidationError(f"{run_id} has incomplete units or attempts")
    if not seal["teardown"]["owned_processes_stopped"] or seal["teardown"]["residual_listener_count"]:
        raise Phase4ValidationError(f"{run_id} teardown gate failed")
    _validate_git(preflight)
    record = {
        "run_id": run_id,
        "preflight_sha256": sha256_file(raw / "manifest.preflight.json"),
        "build_artifacts_sha256": sha256_file(raw / "build-artifacts.json"),
        "checker_sha256": sha256_file(raw / "checker-report.json"),
        "seal_sha256": sha256_file(raw / "manifest.seal.json"),
        "independent_recheck_sha256": sha256_bytes(canonical_json_bytes(rechecked)),
        "independent_recheck_status": rechecked["summary"]["status"],
        "seal_status": seal["status"],
        "artifact_validation": validation["status"],
        "artifact_count": validation["artifacts"],
        "result_count": validation["results"],
        "planned_units": counts["planned_units"],
        "completed_units": counts["completed_units"],
        "planned_attempts": counts["planned_attempts"],
        "completed_attempts": counts["completed_attempts"],
    }
    return preflight, rechecked, record


def _e5_audit(correctness_runs: Mapping[str, Mapping[str, str]]) -> dict[str, Any]:
    record_count = 0
    g3_snapshot_count = 0
    g3_projection_count = 0
    for baseline in BASELINES:
        raw = EXP_ROOT / "results/raw" / correctness_runs["E5"][baseline]
        for path in raw.glob("attempts/**/result.json"):
            result = load_json(path)
            if result["settlement"]["record"] is not None:
                record_count += 1
            if baseline == "G3":
                for snapshot in result["snapshots"].values():
                    g3_snapshot_count += 1
                    projection = snapshot["projection"]
                    if snapshot["authority_source"] != "runtime":
                        raise Phase4ValidationError(f"G3 non-Runtime authority in {path}")
                    if projection["authoritative"] or projection["availability"] != "unavailable":
                        raise Phase4ValidationError(f"G3 contract projection used as authority in {path}")
                    g3_projection_count += 1
    if record_count != EXPECTED_SETTLEMENT_RECORDS:
        raise Phase4ValidationError(f"E5 SettlementRecord count is {record_count}, expected {EXPECTED_SETTLEMENT_RECORDS}")
    return {
        "status": "PASS",
        "successful_settlement_records": record_count,
        "settlement_record_field_count": 24,
        "g3_runtime_authoritative_snapshots": g3_snapshot_count,
        "g3_non_authoritative_unavailable_projections": g3_projection_count,
        "complete_proof": "Deferred",
        "i11": "Deferred",
    }


def build_manifest(
    provisional_id: str,
    correctness_runs: dict[str, dict[str, str]],
) -> dict[str, Any]:
    phase3_gate = (EXP_ROOT / "reports/planning/phase3-gate-review.md").read_text(encoding="utf-8")
    if "PHASE_3_GATE = PASS" not in phase3_gate or "READY_FOR_PHASE_4 = true" not in phase3_gate:
        raise Phase4ValidationError("Phase 3 gate does not authorize S4.8")
    progress = (EXP_ROOT / "reports/planning/phase4-progress.md").read_text(encoding="utf-8")
    ledger: dict[str, str] = {}
    for line in progress.splitlines():
        if line.startswith("| S4."):
            columns = [item.strip() for item in line.strip("|").split("|")]
            ledger[columns[0].split()[0]] = columns[1]
    for step in range(1, 8):
        if ledger.get(f"S4.{step}") != "completed":
            raise Phase4ValidationError(f"phase4 progress does not mark S4.{step} completed")
    if "S4.7 is `completed`" not in progress:
        raise Phase4ValidationError("S4.7 completion marker is absent")

    provisional_path = EXP_ROOT / "results/provisional" / provisional_id / "provisional-candidate.manifest.json"
    provisional = load_json(provisional_path)
    if provisional["candidate_id"] != provisional_id or provisional["validity_level"] != "provisional-candidate":
        raise Phase4ValidationError("source provisional candidate identity drift")
    if provisional["status"] != "frozen" or provisional["authorization"]["e6_authorized"]:
        raise Phase4ValidationError("source provisional candidate has an invalid authorization state")
    if stat.S_IMODE(provisional_path.stat().st_mode) & 0o222:
        raise Phase4ValidationError("source provisional manifest is writable")

    preflights: dict[str, dict[str, dict[str, Any]]] = {"E0": {}}
    records: dict[str, dict[str, dict[str, Any]]] = {family: {} for family in FAMILIES}
    reports: dict[str, dict[str, dict[str, Any]]] = {family: {} for family in FAMILIES}
    e0_records: dict[str, dict[str, Any]] = {}
    for baseline in BASELINES:
        source = provisional["runs"][baseline]
        preflight, report, record = _validate_run(source["run_id"], "E0", baseline)
        for field in ("preflight_sha256", "seal_sha256"):
            if record[field] != source[field]:
                raise Phase4ValidationError(f"source provisional {baseline} {field} drift")
        preflights["E0"][baseline] = preflight
        e0_records[baseline] = record

    for family in FAMILIES:
        preflights[family] = {}
        for baseline in BASELINES:
            preflight, report, record = _validate_run(correctness_runs[family][baseline], family, baseline)
            if preflight["inputs"]["repetitions"] != 5:
                raise Phase4ValidationError(f"{family}:{baseline} repetitions are not R=5")
            preflights[family][baseline] = preflight
            reports[family][baseline] = report
            records[family][baseline] = record

    reference = _common_identity(preflights["E0"]["B1"])
    baseline_identities = {baseline: _baseline_identity(preflights["E0"][baseline]) for baseline in BASELINES}
    for family in ("E0", *FAMILIES):
        for baseline in BASELINES:
            preflight = preflights[family][baseline]
            if _common_identity(preflight) != reference:
                raise Phase4ValidationError(f"common identity drift at {family}:{baseline}")
            if _baseline_identity(preflight) != baseline_identities[baseline]:
                raise Phase4ValidationError(f"baseline identity drift at {family}:{baseline}")

    for baseline in BASELINES:
        expected_artifacts = load_json(
            EXP_ROOT / "results/raw" / provisional["runs"][baseline]["run_id"] / "build-artifacts.json"
        )["artifacts"]
        for family in FAMILIES:
            actual_artifacts = load_json(
                EXP_ROOT / "results/raw" / correctness_runs[family][baseline] / "build-artifacts.json"
            )["artifacts"]
            if actual_artifacts != expected_artifacts:
                raise Phase4ValidationError(f"executed build artifact drift at {family}:{baseline}")
        baseline_identities[baseline]["executed_build_artifacts"] = expected_artifacts

    catalog = load_catalog()
    family_scenarios = {
        family: [item["scenario_id"] for item in select_scenarios(catalog, family)]
        for family in FAMILIES
    }
    covered = set().union(*(set(family_scenarios[family]) for family in ("E1", "E2", "E3", "E4")))
    catalog_ids = {item["scenario_id"] for item in catalog["scenarios"]}
    if covered != catalog_ids or len(catalog_ids) != 42:
        raise Phase4ValidationError("E1-E4 do not cover the exact frozen 42-scenario catalog")
    mode_counts = Counter(item["execution_mode"] for item in catalog["scenarios"])
    if mode_counts != {"live-chain": 39, "test-gate": 3}:
        raise Phase4ValidationError(f"scenario execution-mode coverage drift: {dict(mode_counts)}")

    totals = {field: 0 for field in SUMMARY_COUNTERS}
    artifact_total = 0
    result_total = 0
    for family in FAMILIES:
        for baseline in BASELINES:
            summary = reports[family][baseline]["summary"]
            for field in SUMMARY_COUNTERS:
                totals[field] += summary[field]
            artifact_total += records[family][baseline]["artifact_count"]
            result_total += records[family][baseline]["result_count"]
    if totals["completed_units"] != EXPECTED_UNITS or totals["completed_attempts"] != EXPECTED_ATTEMPTS:
        raise Phase4ValidationError(f"aggregate completion totals drift: {totals}")
    if any(totals[field] for field in ZERO_COUNTERS):
        raise Phase4ValidationError(f"candidate checker thresholds failed: {totals}")
    open_ports = _ports_open()
    if open_ports:
        raise Phase4ValidationError(f"formal ports remain open: {open_ports}")

    now = datetime.now(ZoneInfo("Asia/Shanghai")).replace(microsecond=0)
    candidate_id = f"p4-paper-candidate-{now.strftime('%Y%m%dT%H%M%S+0800')}"
    common_tool_map = {item["tool_id"]: item for item in reference["common_tools"]}
    manifest = {
        "schema_id": "assetxchain-exp2-phase4-paper-candidate",
        "schema_version": 1,
        "candidate_id": candidate_id,
        "created_at": now.isoformat(),
        "timezone": "Asia/Shanghai",
        "phase_step": "S4.8",
        "status": "frozen",
        "validity_level": "paper-candidate",
        "claim_boundary": "E1-E5 formal correctness evidence only; authorizes E6 execution but contains no performance result, ranking, statistical conclusion, or paper-final promotion",
        "source_provisional_candidate": {
            "candidate_id": provisional_id,
            "path": provisional_path.relative_to(EXP_ROOT).as_posix(),
            "sha256": sha256_file(provisional_path),
            "validity_level": provisional["validity_level"],
        },
        "frozen_identity": {
            "protocol": reference["protocol"],
            "fixture_profile_id": reference["fixture"]["fixture_profile_id"],
            "fixture_config_sha256": reference["fixture"]["fixture_config_sha256"],
            "vector_bundle_sha256": reference["fixture"]["vector_bundle_sha256"],
            "shared_input_hash": provisional["frozen_inputs"]["shared_input_hash"],
            "environment_config_sha256": reference["environment_config"]["sha256"],
            "correctness_workload_sha256": reference["workload"]["sha256"],
            "scenario_catalog_sha256": reference["scenario_catalog"]["sha256"],
            "toolchain_profile_sha256": provisional["frozen_inputs"]["toolchain_profile_sha256"],
            "executor_sha256": provisional["frozen_inputs"]["executor_sha256"],
            "runner_sha256": common_tool_map["runner"]["sha256"],
            "checker_sha256": common_tool_map["checker"]["sha256"],
            "result_schema_sha256": common_tool_map["result-schema"]["sha256"],
            "snapshot_schema_sha256": common_tool_map["snapshot-schema"]["sha256"],
            "candidate_schema_sha256": sha256_file(EXP_ROOT / CANDIDATE_SCHEMA),
            "candidate_freezer_sha256": sha256_file(Path(__file__)),
            "seed": reference["seed"],
            "correctness_repetitions": 5,
            "scenario_count": 42,
            "live_scenario_count": 39,
            "test_gate_scenario_count": 3,
            "required_invariants": reference["required_invariants"],
            "deferred_invariants": reference["deferred_invariants"],
            "stable_environment": reference["environment"],
        },
        "baselines": baseline_identities,
        "e0_reproducibility_runs": e0_records,
        "correctness_runs": records,
        "candidate_gate": {
            "status": "PASS",
            "phase3_gate": "PASS",
            "ready_for_phase4": True,
            "predecessors_s4_1_through_s4_7": "completed",
            "families": list(FAMILIES),
            "scenario_coverage": {
                "status": "PASS",
                "catalog_scenarios": 42,
                "covered_scenarios": 42,
                "live_scenarios": 39,
                "test_gate_scenarios": 3,
                "family_scenarios": family_scenarios,
            },
            "checker_totals": totals,
            "thresholds": {field: 0 for field in ZERO_COUNTERS},
            "artifact_integrity": {
                "status": "PASS",
                "sealed_runs_validated": 18,
                "correctness_artifacts": artifact_total,
                "correctness_result_files": result_total,
                "source_raw_modified": False,
            },
            "identity_consistency": {
                "status": "PASS",
                "same_protocol_fixture_config_seed_workload_schema_tools": True,
                "same_clean_baseline_commits_across_e0_e5": True,
                "same_executed_build_artifacts_across_e0_e5": True,
            },
            "e5_audit_closure": _e5_audit(correctness_runs),
            "teardown": {
                "status": "PASS",
                "all_owned_processes_stopped": True,
                "residual_listener_count": 0,
                "closed_ports": list(PORTS),
            },
        },
        "authorization": {
            "next_allowed_step": "S4.9",
            "e6_authorized": True,
            "paper_candidate": True,
            "paper_final": False,
            "performance_claims_authorized": False,
        },
        "immutability": {
            "source_raw_must_not_be_modified": True,
            "manifest_must_not_be_modified_after_creation": True,
            "semantic_change_requires_return_to": "S4.2",
        },
    }
    schema_validate(manifest, CANDIDATE_SCHEMA)
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provisional-candidate", required=True)
    parser.add_argument("--run", action="append", required=True, help="FAMILY:BASELINE=RUN_ID; provide E1-E5 x B1/B2/G3")
    parser.add_argument("--check-only", action="store_true")
    args = parser.parse_args()
    manifest = build_manifest(args.provisional_candidate, _parse_runs(args.run))
    output = None
    if not args.check_only:
        output = EXP_ROOT / "results/candidates" / manifest["candidate_id"] / "candidate.manifest.json"
        write_json_exclusive(output, manifest)
    print(json.dumps({
        "candidate_id": manifest["candidate_id"],
        "status": manifest["candidate_gate"]["status"],
        "validity_level": manifest["validity_level"],
        "checker_totals": manifest["candidate_gate"]["checker_totals"],
        "path": str(output) if output else None,
        "sha256": sha256_file(output) if output else sha256_bytes(canonical_json_bytes(manifest)),
    }, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (Phase4ValidationError, subprocess.CalledProcessError) as error:
        print(f"ERROR: {error}")
        raise SystemExit(2)
