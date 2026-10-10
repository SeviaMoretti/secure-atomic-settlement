#!/usr/bin/env python3
"""Run one E2 scenario on an isolated throwaway chain outside formal raw."""

from __future__ import annotations

import argparse
import json
import subprocess
import traceback
from datetime import datetime
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo

from phase4_checker import CheckFailure, _check_attempt
from phase4_contract import EXP_ROOT, Phase4ValidationError, load_catalog, load_json, scenario_map, write_json_exclusive
from phase4_manifest import build_preflight
from phase4_runner import (
    RunnerLog,
    _check_git,
    _formal_environment,
    _open_ports,
    _run_executor,
    _stop_node,
    _wait_rpc,
)


NODE = Path.home() / ".local/node-v24.15.0-linux-x64/bin/node"
EXECUTOR = EXP_ROOT / "scripts/phase4/formal_e0_executor.js"
DEPENDENCIES = (
    EXECUTOR,
    EXP_ROOT / "scripts/scenarios/s314_live_common.js",
    EXP_ROOT / "scripts/scenarios/s314_b1_live_smoke.js",
    EXP_ROOT / "scripts/scenarios/s314_b2_live_smoke.js",
    EXP_ROOT / "scripts/scenarios/s314_g3_live_smoke.js",
    EXP_ROOT / "scripts/phase4/node/package.json",
    EXP_ROOT / "scripts/phase4/node/package-lock.json",
    EXP_ROOT / "configs/phase4/formal-toolchain-v1.json",
)


def _executor_command() -> list[str]:
    command = [str(NODE), str(EXECUTOR)]
    for dependency in DEPENDENCIES:
        command.extend(("--dependency", str(dependency)))
    return command


def _timestamp() -> datetime:
    return datetime.now(ZoneInfo("Asia/Shanghai")).replace(microsecond=0)


def _diagnostic_manifest(baseline: str, scenario_id: str) -> tuple[dict[str, Any], Path, Path]:
    created_at = _timestamp()
    manifest = build_preflight(
        exp_root=EXP_ROOT,
        baseline_id=baseline,
        family="E2",
        executor_command=_executor_command(),
        created_at=created_at,
    )
    run_id = (
        f"diag-s4.4-e2-{baseline.lower()}-{scenario_id.lower()}-"
        f"{created_at.strftime('%Y%m%dT%H%M%S+0800')}-{manifest['baseline']['head'][:8]}"
    )
    root = EXP_ROOT / "logs/diagnostics" / run_id
    raw = root / "raw"
    if root.exists():
        raise Phase4ValidationError(f"diagnostic path already exists: {root}")
    root.mkdir(parents=True)
    raw.mkdir()
    manifest["run_id"] = run_id
    manifest["claim_boundary"] = "diagnostics-only-not-formal-evidence"
    manifest["paths"] = {
        "raw": str(raw),
        "log": str(root),
        "processed": str(root / "processed"),
        "report": str(root / "report.md"),
    }
    manifest_path = root / "manifest.diagnostic.json"
    write_json_exclusive(manifest_path, manifest)
    return manifest, manifest_path, root


def _attempt_summary(attempt_dir: Path) -> dict[str, Any]:
    result = load_json(attempt_dir / "result.json")
    evidence = load_json(attempt_dir / "block-events.json")
    return {
        "attempt_id": result["request"]["attempt_id"],
        "common_error_code": result["outcome"]["common_error_code"],
        "native_error": result["outcome"]["native_error"],
        "dispatch_fee": result["outcome"]["dispatch_fee_amount"],
        "pre_block": result["snapshots"]["pre"]["observed_at"]["block_number"],
        "post_block": result["snapshots"]["post"]["observed_at"]["block_number"],
        "parent_block_hash": evidence["parent_block_hash"],
        "included_block_hash": evidence["included_block_hash"],
        "extrinsic_index": evidence["extrinsic_index"],
        "transaction_event_indexes": evidence["transaction_event_indexes"],
        "complete_block_event_count": evidence["complete_block_event_count"],
        "pre_buyer_balance": result["snapshots"]["pre"]["payment"]["buyer_balance"],
        "post_buyer_balance": result["snapshots"]["post"]["payment"]["buyer_balance"],
        "environmental_balance_adjustments": result["outcome"]["environmental_balance_adjustments"],
        "state_commitment_match": (
            result["snapshots"]["pre"]["state_commitment"]
            == result["snapshots"]["post"]["state_commitment"]
        ),
    }


def run(baseline: str, scenario_id: str) -> tuple[int, Path]:
    if _open_ports():
        raise Phase4ValidationError(f"diagnostic requires closed formal ports, found {_open_ports()}")
    catalog = scenario_map(load_catalog())
    scenario = catalog.get(scenario_id)
    if scenario is None or scenario["family"] != "E2" or scenario["execution_mode"] != "live-chain":
        raise Phase4ValidationError(f"scenario is not a live E2 scenario: {scenario_id}")

    manifest, manifest_path, root = _diagnostic_manifest(baseline, scenario_id)
    unit = next(
        item for item in manifest["planned_execution"]
        if item["scenario_id"] == scenario_id and item["repetition"] == 1
    )
    runner_log = RunnerLog(root / "runner.log")
    process: subprocess.Popen[bytes] | None = None
    node_handle = None
    status = "FAIL"
    error_text = None
    check_rows: list[dict[str, Any]] = []
    attempts: list[dict[str, Any]] = []
    try:
        _check_git(manifest)
        environment, _ = _formal_environment(manifest)
        node_path = root / "node" / f"{scenario_id}.log"
        node_path.parent.mkdir()
        node_handle = node_path.open("xb")
        process = subprocess.Popen(
            manifest["execution"]["node_command"],
            cwd=manifest["execution"]["node_cwd"],
            env=environment,
            stdout=node_handle,
            stderr=subprocess.STDOUT,
        )
        _wait_rpc(process, manifest["execution"]["readiness_timeout_seconds"])
        output_root = Path(manifest["paths"]["raw"]) / "attempts" / scenario_id / "r-001"
        _run_executor(manifest_path, manifest, unit, output_root, runner_log, environment)

        failures = CheckFailure()
        for index, expected in enumerate(scenario["attempts"], start=1):
            attempt_dir = output_root / f"attempt-{index:03d}"
            _check_attempt(attempt_dir, manifest, scenario, expected, 1, failures)
            attempts.append(_attempt_summary(attempt_dir))
        check_rows = failures.rows
        if check_rows:
            raise Phase4ValidationError(f"diagnostic attempt checker failed: {json.dumps(check_rows, sort_keys=True)}")
        status = "PASS"
    except Exception as error:  # Preserve the complete diagnostic failure without entering formal raw.
        error_text = f"{error}\n{traceback.format_exc()}"
    finally:
        if process is not None:
            try:
                _stop_node(process)
            except Exception as stop_error:
                status = "FAIL"
                suffix = f"teardown failed: {stop_error}\n{traceback.format_exc()}"
                error_text = f"{error_text}\n{suffix}" if error_text else suffix
        if node_handle is not None:
            node_handle.close()
        runner_log.close()

    summary = {
        "schema_id": "assetxchain-exp2-s4.4-e2-diagnostic",
        "schema_version": 1,
        "run_id": manifest["run_id"],
        "baseline": baseline,
        "scenario_id": scenario_id,
        "status": status,
        "validity_level": "diagnostics-only",
        "formal_raw": False,
        "checker_failures": check_rows,
        "attempts": attempts,
        "residual_ports": _open_ports(),
        "error": error_text,
    }
    write_json_exclusive(root / "diagnostic-result.json", summary)
    return (0 if status == "PASS" and not summary["residual_ports"] else 1), root


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, choices=("B1", "B2", "G3"))
    parser.add_argument("--scenario", required=True)
    args = parser.parse_args()
    code, root = run(args.baseline, args.scenario)
    print(json.dumps({"status": "PASS" if code == 0 else "FAIL", "path": str(root)}, sort_keys=True))
    return code


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Phase4ValidationError as error:
        print(f"ERROR: {error}")
        raise SystemExit(2)
