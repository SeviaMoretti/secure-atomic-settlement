#!/usr/bin/env python3
"""Independently recompute every frozen R3 X1 diagnostic cell."""

from __future__ import annotations

import argparse
import importlib.util
import json
from datetime import datetime
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo


BASELINES = ("B1", "B2", "G3")
VARIANTS = (
    "control-fixed-price-zero-fee-fifo",
    "pricing-demand-markup-500bps",
    "fee-market-fee-100bps",
    "matching-highest-offer-then-arrival",
)
OPERATIONS = ("TransferMeta", "IssueRight", "TransferRight")


class Blocked(RuntimeError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise Blocked(message)


def load_module(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path)
    require(spec is not None and spec.loader is not None, f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def dump_exclusive(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(value, handle, ensure_ascii=True, indent=2, sort_keys=True)
        handle.write("\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--plan-root", type=Path, required=True)
    parser.add_argument("--result-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    root = args.root.resolve()
    failures: list[str] = []
    rows: list[dict[str, Any]] = []
    try:
        checker = load_module("x1_checker", root / "scripts/r2/x1_checker.py")
        runner = load_module("x1_live_runner", root / "scripts/r2/x1_live_runner.py")
        fixture = checker.load(root / "fixtures/phase3/assetxchain-paper2-shared-fixture-v1.json")
        executor = (root / "scripts/r2/x1_live_unit_executor.js").resolve()
        plan_paths = sorted(args.plan_root.resolve().glob("*.json"))
        require(len(plan_paths) == 36, f"diagnostic plan count drift: {len(plan_paths)}")

        cells: set[tuple[str, str, str]] = set()
        for plan_path in plan_paths:
            plan = load(plan_path)
            node = Path(plan["artifacts"]["node"]["path"]).resolve()
            runner.validate_plan(plan, node, executor)
            require(plan.get("repetition_count") == 1, "diagnostic repetition drift")
            require(plan.get("evidence_level") == "diagnostic-only", "diagnostic evidence level drift")
            require(plan.get("formal_evidence_eligible") is False,
                    "diagnostic plan is incorrectly formal-eligible")
            cell = (plan["baseline_id"], plan["variant_id"], plan["operation"])
            require(cell not in cells, f"duplicate diagnostic cell: {cell}")
            cells.add(cell)

            run_root = args.result_root.resolve() / plan["run_id"]
            live_path = run_root / "live-evidence.json"
            runner_path = run_root / "runner-manifest.json"
            copied_plan = run_root / "live-unit-plan.json"
            require(live_path.is_file(), f"missing live evidence: {plan['run_id']}")
            require(runner_path.is_file(), f"missing runner manifest: {plan['run_id']}")
            require(copied_plan.is_file(), f"missing copied plan: {plan['run_id']}")
            require(checker.sha256(copied_plan) == checker.sha256(plan_path),
                    f"copied plan drift: {plan['run_id']}")

            live = load(live_path)
            runner_manifest = load(runner_path)
            require(live.get("status") == "PASS", f"live evidence blocked: {plan['run_id']}")
            require(live.get("run_id") == plan["run_id"], f"live run ID drift: {plan['run_id']}")
            require(live.get("repetition_count") == 1 and len(live.get("attempts", [])) == 1,
                    f"live unit count drift: {plan['run_id']}")
            require(runner_manifest.get("status") == "PASS",
                    f"runner manifest blocked: {plan['run_id']}")
            require(runner_manifest.get("ports_closed_after_run") is True,
                    f"runner left ports open: {plan['run_id']}")
            require(runner_manifest.get("plan", {}).get("sha256") == checker.sha256(plan_path),
                    f"runner plan hash drift: {plan['run_id']}")
            require(runner_manifest.get("live_evidence", {}).get("sha256") == checker.sha256(live_path),
                    f"runner live-evidence hash drift: {plan['run_id']}")

            attempt = live["attempts"][0]
            unit_path = Path(attempt["unit_evidence"]["path"]).resolve()
            run = {
                "run_id": live["run_id"],
                "baseline_id": live["baseline_id"],
                "implementation_commit": live["implementation_commit"],
            }
            derived = checker.verify_unit(
                root, unit_path, attempt, run, live["variant_id"], live["operation"], 1, fixture
            )
            rows.append({
                "run_id": plan["run_id"],
                "baseline_id": plan["baseline_id"],
                "variant_id": plan["variant_id"],
                "operation": plan["operation"],
                "attempt_count": derived["attempt_count"],
                "setup_fee": derived["setup_fee"],
                "unit_evidence": {
                    "path": str(unit_path),
                    "sha256": checker.sha256(unit_path),
                },
            })

        expected = {(baseline, variant, operation) for baseline in BASELINES
                    for variant in VARIANTS for operation in OPERATIONS}
        require(cells == expected, "diagnostic cross-product is incomplete")
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError, Blocked) as error:
        failures.append(str(error))

    payload = {
        "schema_id": "assetxchain-exp2-r3-x1-diagnostic-checker-report",
        "schema_version": 1,
        "status": "PASS" if not failures else "BLOCKED",
        "created_at": datetime.now(ZoneInfo("Asia/Shanghai")).replace(microsecond=0).isoformat(),
        "timezone": "Asia/Shanghai",
        "evidence_level": "diagnostic-only",
        "formal_evidence_eligible": False,
        "cell_count": len(rows),
        "unit_count": len(rows),
        "invariant_violation_count": 0 if not failures else None,
        "failures": failures,
        "cells": sorted(rows, key=lambda item: (
            item["baseline_id"], item["variant_id"], item["operation"]
        )),
        "claim_boundary": "diagnostic harness validation only; excluded from formal X1 evidence",
    }
    dump_exclusive(args.out.resolve(), payload)
    print(json.dumps({
        "status": payload["status"], "cell_count": payload["cell_count"], "failures": failures
    }, ensure_ascii=True, sort_keys=True))
    return 0 if not failures else 2


if __name__ == "__main__":
    raise SystemExit(main())
