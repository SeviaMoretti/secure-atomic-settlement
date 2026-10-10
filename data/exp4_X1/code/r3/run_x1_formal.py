#!/usr/bin/env python3
"""Execute and seal the complete frozen R3 X1 formal cross-product."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo


BASELINES = {
    "B1": {
        "control-fixed-price-zero-fee-fifo": "reports/r3/x1-e0-v1/b1-control.json",
        "pricing-demand-markup-500bps": "reports/r3/x1-e0-v1/b1-pricing.json",
        "fee-market-fee-100bps": "reports/r3/x1-e0-v2/b1-fee.json",
        "matching-highest-offer-then-arrival": "reports/r3/x1-e0-v2/b1-matching.json",
    },
    "B2": {
        "control-fixed-price-zero-fee-fifo": "reports/r3/x1-e0-v1/b2-control.json",
        "pricing-demand-markup-500bps": "reports/r3/x1-e0-v1/b2-pricing.json",
        "fee-market-fee-100bps": "reports/r3/x1-e0-v1/b2-fee.json",
        "matching-highest-offer-then-arrival": "reports/r3/x1-e0-v1/b2-matching.json",
    },
    "G3": {
        "control-fixed-price-zero-fee-fifo": "reports/r3/x1-e0-v1/g3-control.json",
        "pricing-demand-markup-500bps": "reports/r3/x1-e0-v1/g3-pricing.json",
        "fee-market-fee-100bps": "reports/r3/x1-e0-v1/g3-fee.json",
        "matching-highest-offer-then-arrival": "reports/r3/x1-e0-v1/g3-matching.json",
    },
}
OPERATIONS = ("TransferMeta", "IssueRight", "TransferRight")
VARIANT_LABELS = {
    "control-fixed-price-zero-fee-fifo": "control",
    "pricing-demand-markup-500bps": "pricing",
    "fee-market-fee-100bps": "fee",
    "matching-highest-offer-then-arrival": "matching",
}


class Blocked(RuntimeError):
    pass


def load(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise Blocked(message)


def run(command: list[str], cwd: Path) -> None:
    print(json.dumps({"event": "command", "argv": command}, sort_keys=True), flush=True)
    completed = subprocess.run(command, cwd=str(cwd), text=True, capture_output=True, check=False)
    if completed.stdout:
        print(completed.stdout, end="" if completed.stdout.endswith("\n") else "\n", flush=True)
    if completed.stderr:
        print(completed.stderr, end="" if completed.stderr.endswith("\n") else "\n",
              file=sys.stderr, flush=True)
    require(completed.returncode == 0,
            f"command failed ({completed.returncode}): {' '.join(command)}")


def dump_exclusive(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8", newline="\n") as handle:
        json.dump(value, handle, ensure_ascii=True, indent=2, sort_keys=True)
        handle.write("\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--harness-manifest", type=Path, required=True)
    parser.add_argument("--version-tag", required=True)
    parser.add_argument("--batch-manifest", type=Path, required=True)
    args = parser.parse_args()

    root = args.root.resolve()
    harness = args.harness_manifest.resolve()
    executor = (root / "scripts/r2/x1_live_unit_executor.js").resolve()
    preflight_root = root / "reports/r3" / f"x1-formal-preflights-{args.version_tag}"
    plan_root = root / "reports/r3" / f"x1-formal-plans-{args.version_tag}"
    result_root = root / "reports/r3" / f"x1-formal-results-{args.version_tag}"
    seal_root = root / "reports/r3" / f"x1-formal-seals-{args.version_tag}"

    try:
        require(root.is_dir(), f"experiment root is missing: {root}")
        require(harness.is_file(), f"harness manifest is missing: {harness}")
        require(load(harness).get("status") == "PASS", "harness manifest is not PASS")
        require(executor.is_file(), f"executor is missing: {executor}")
        for path in (preflight_root, plan_root, result_root, seal_root, args.batch_manifest):
            require(not path.exists(), f"formal batch target already exists: {path}")

        cells: list[dict[str, Any]] = []
        seen_run_ids: set[str] = set()
        first = True
        for baseline, variants in BASELINES.items():
            for variant, e0_relative in variants.items():
                for operation in OPERATIONS:
                    if not first:
                        time.sleep(1.05)
                    first = False
                    slug = f"{baseline.lower()}-{VARIANT_LABELS[variant]}-{operation.lower()}"
                    preflight = preflight_root / f"{slug}.preflight.json"
                    plan = plan_root / f"{slug}.plan.json"
                    e0 = (root / e0_relative).resolve()
                    run([
                        "python3", "scripts/r2/x1_preflight.py", "--root", str(root),
                        "--baseline", baseline, "--variant", variant,
                        "--operation", operation, "--e0-manifest", str(e0),
                        "--out", str(preflight),
                    ], root)
                    run([
                        "python3", "scripts/r2/x1_plan.py", "--root", str(root),
                        "--preflight", str(preflight), "--out", str(plan),
                    ], root)
                    plan_value = load(plan)
                    run_id = str(plan_value["run_id"])
                    require(run_id not in seen_run_ids, f"formal run ID collision: {run_id}")
                    seen_run_ids.add(run_id)
                    cells.append({
                        "baseline_id": baseline,
                        "variant_id": variant,
                        "operation": operation,
                        "run_id": run_id,
                        "preflight": preflight,
                        "plan": plan,
                        "node": Path(plan_value["artifacts"]["node"]["path"]).resolve(),
                    })

        require(len(cells) == 36 and len(seen_run_ids) == 36,
                "formal X1 plan cross-product is incomplete")

        completed_cells: list[dict[str, Any]] = []
        for index, cell in enumerate(cells, start=1):
            run_id = cell["run_id"]
            live_root = root / "results/raw" / run_id
            cell_result_root = result_root / run_id
            result = cell_result_root / "result.json"
            checker = cell_result_root / "checker-report.json"
            seal = seal_root / f"{run_id}.seal.json"
            print(json.dumps({
                "event": "cell-start", "index": index, "total": len(cells),
                "run_id": run_id,
            }, sort_keys=True), flush=True)
            run([
                "python3", "scripts/r2/x1_live_runner.py", "--plan", str(cell["plan"]),
                "--node", str(cell["node"]), "--executor", str(executor),
                "--output-root", str(live_root),
            ], root)
            run([
                "python3", "scripts/r2/x1_driver.py", "--root", str(root),
                "--preflight", str(cell["preflight"]), "--operation", cell["operation"],
                "--repetitions", "10", "--out", str(result),
                "--live-evidence", str(live_root / "live-evidence.json"),
            ], root)
            run([
                "python3", "scripts/r2/x1_checker.py", "--root", str(root),
                "--result", str(result), "--out", str(checker),
            ], root)
            checker_value = load(checker)
            require(checker_value.get("status") == "PASS" and not checker_value.get("failures"),
                    f"formal checker blocked: {run_id}")
            run([
                "python3", "scripts/r2/x1_artifacts.py", "seal",
                "--run-root", str(live_root), "--result-root", str(cell_result_root),
                "--preflight", str(cell["preflight"]),
                "--harness-manifest", str(harness), "--out", str(seal),
            ], root)
            completed_cells.append({
                "run_id": run_id,
                "baseline_id": cell["baseline_id"],
                "variant_id": cell["variant_id"],
                "operation": cell["operation"],
                "preflight": {"path": str(cell["preflight"]),
                              "sha256": sha256(cell["preflight"])},
                "plan": {"path": str(cell["plan"]), "sha256": sha256(cell["plan"])},
                "live_root": str(live_root),
                "result": {"path": str(result), "sha256": sha256(result)},
                "checker": {"path": str(checker), "sha256": sha256(checker)},
                "seal": {"path": str(seal), "sha256": sha256(seal)},
            })
            print(json.dumps({
                "event": "cell-complete", "index": index, "total": len(cells),
                "run_id": run_id,
            }, sort_keys=True), flush=True)

        payload = {
            "schema_id": "assetxchain-exp2-r3-x1-formal-batch",
            "schema_version": 1,
            "status": "completed",
            "created_at": datetime.now(ZoneInfo("Asia/Shanghai")).replace(microsecond=0).isoformat(),
            "timezone": "Asia/Shanghai",
            "experiment_id": "X1",
            "evidence_level": "extended-experiment-candidate",
            "claim_boundary": "evolvability-only; no performance or architecture ranking",
            "cell_count": len(completed_cells),
            "repetitions_per_cell": 10,
            "harness_manifest": {"path": str(harness), "sha256": sha256(harness)},
            "cells": completed_cells,
        }
        dump_exclusive(args.batch_manifest.resolve(), payload)
        print(json.dumps({
            "status": "PASS", "cell_count": len(completed_cells),
            "batch_manifest": str(args.batch_manifest.resolve()),
        }, sort_keys=True), flush=True)
        return 0
    except (OSError, KeyError, TypeError, ValueError, json.JSONDecodeError, Blocked) as error:
        print(json.dumps({"status": "BLOCKED", "reason": str(error)}, sort_keys=True),
              file=sys.stderr, flush=True)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
