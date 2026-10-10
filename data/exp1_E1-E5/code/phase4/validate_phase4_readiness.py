"""Read-only S4.1 validator for the complete Phase 4 formal tooling surface."""

from __future__ import annotations

import argparse
import json
import socket
import subprocess
import sys
from datetime import datetime
from pathlib import Path
from zoneinfo import ZoneInfo

from phase4_contract import (
    BASELINES,
    EXP_ROOT,
    FAMILIES,
    PORTS,
    Phase4ValidationError,
    load_catalog,
    select_scenarios,
    sha256_file,
    tool_paths,
    validate_phase3_gate,
)
from phase4_manifest import build_preflight


COMMON_DIR = EXP_ROOT / "scripts/common"
if str(COMMON_DIR) not in sys.path:
    sys.path.insert(0, str(COMMON_DIR))
from phase3_contract import validate_contract_bundle


def _port_open(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(0.2)
        return sock.connect_ex(("127.0.0.1", port)) == 0


def validate_readiness(*, run_tests: bool = True) -> dict[str, object]:
    checks: list[dict[str, object]] = []
    blockers: list[str] = []

    def check(name: str, action) -> None:
        try:
            detail = action()
            checks.append({"name": name, "status": "PASS", "detail": detail})
        except Exception as error:
            blockers.append(f"{name}: {error}")
            checks.append({"name": name, "status": "FAIL", "detail": str(error)})

    check("phase3-gate", lambda: (validate_phase3_gate(EXP_ROOT), "PHASE_3_GATE=PASS; READY_FOR_PHASE_4=true")[1])
    check("phase3-contract-and-git", lambda: validate_contract_bundle(EXP_ROOT, check_git=True))
    catalog_holder: dict[str, object] = {}

    def catalog_check():
        catalog = load_catalog()
        catalog_holder["catalog"] = catalog
        return {
            "scenarios": len(catalog["scenarios"]),
            "live": sum(item["execution_mode"] == "live-chain" for item in catalog["scenarios"]),
            "test_gates": sum(item["execution_mode"] == "test-gate" for item in catalog["scenarios"]),
            "family_counts": {family: len(select_scenarios(catalog, family)) for family in FAMILIES},
        }

    check("scenario-catalog", catalog_check)

    def tooling_check():
        rows = []
        for relative in tool_paths():
            path = EXP_ROOT / relative
            if not path.is_file():
                raise Phase4ValidationError(f"missing {relative}")
            rows.append({"path": relative.as_posix(), "sha256": sha256_file(path)})
        return rows

    check("tool-and-schema-inventory", tooling_check)

    def manifest_check():
        fixed = datetime(2026, 7, 29, 15, 0, 0, tzinfo=ZoneInfo("Asia/Shanghai"))
        rows = []
        executor = [sys.executable, str(EXP_ROOT / "scripts/phase4/tests/fake_executor.py")]
        for baseline in BASELINES:
            for family in FAMILIES:
                candidate = "readiness-only-candidate" if family == "E6" else None
                manifest = build_preflight(
                    exp_root=EXP_ROOT,
                    baseline_id=baseline,
                    family=family,
                    executor_command=executor,
                    created_at=fixed,
                    approved_candidate_id=candidate,
                )
                rows.append({
                    "baseline": baseline,
                    "family": family,
                    "units": len(manifest["planned_execution"]),
                    "repetitions": manifest["inputs"]["repetitions"],
                })
        return rows

    check("preflight-semantic-matrix", manifest_check)

    if run_tests:
        def tests_check():
            completed = subprocess.run(
                [sys.executable, "-m", "unittest", "discover", "-s", str(EXP_ROOT / "scripts/phase4/tests"), "-p", "test_*.py", "-v"],
                cwd=EXP_ROOT,
                text=True,
                capture_output=True,
                check=False,
            )
            if completed.returncode != 0:
                raise Phase4ValidationError((completed.stdout + "\n" + completed.stderr).strip())
            return completed.stderr.strip() or completed.stdout.strip()
        check("phase4-tooling-tests", tests_check)

    open_ports = [port for port in PORTS if _port_open(port)]
    if open_ports:
        blockers.append(f"formal ports unexpectedly open during read-only S4.1 audit: {open_ports}")
        checks.append({"name": "no-formal-listeners", "status": "FAIL", "detail": open_ports})
    else:
        checks.append({"name": "no-formal-listeners", "status": "PASS", "detail": list(PORTS)})

    return {
        "schema_id": "assetxchain-exp2-phase4-readiness-report",
        "schema_version": 1,
        "current_step": "S4.1",
        "status": "PASS" if not blockers else "BLOCKED",
        "validity_level": "legacy-pilot",
        "formal_run_ids": [],
        "checks": checks,
        "blockers": blockers,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-tests", action="store_true")
    args = parser.parse_args()
    report = validate_readiness(run_tests=not args.skip_tests)
    print(json.dumps(report, sort_keys=True))
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
