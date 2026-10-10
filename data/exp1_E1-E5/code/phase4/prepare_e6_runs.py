"""Build or exclusively create the three S4.9 E6 preflight manifests."""

from __future__ import annotations

import argparse
import json
from datetime import datetime
from pathlib import Path
from typing import Any, Iterable
from zoneinfo import ZoneInfo

from phase4_contract import Phase4ValidationError, sha256_file, validate_preflight, write_json_exclusive
from phase4_manifest import build_preflight


EXP_ROOT = Path("/home/gz785/code/exp2")
CANDIDATE = EXP_ROOT / "results/candidates/p4-paper-candidate-20260806T004403+0800/candidate.manifest.json"
BASELINES = ("B1", "B2", "G3")
NODE = Path("/home/gz785/.local/node-v24.15.0-linux-x64/bin/node")
WRAPPER = EXP_ROOT / "scripts/phase4/formal_e6_executor.js"
FROZEN_EXECUTOR = EXP_ROOT / "scripts/phase4/formal_e0_executor.js"
VALIDATOR = EXP_ROOT / "scripts/phase4/validate_e6_cost.py"
PLAN_BUILDER = Path(__file__).resolve()
RUNNER = EXP_ROOT / "scripts/phase4/phase4_runner.py"
CHECKER = EXP_ROOT / "scripts/phase4/phase4_checker.py"
DEPENDENCIES = tuple(EXP_ROOT / value for value in (
    "scripts/phase4/formal_e0_executor.js",
    "scripts/scenarios/s314_live_common.js",
    "scripts/scenarios/s314_b1_live_smoke.js",
    "scripts/scenarios/s314_b2_live_smoke.js",
    "scripts/scenarios/s314_g3_live_smoke.js",
    "scripts/phase4/node/package.json",
    "scripts/phase4/node/package-lock.json",
    "configs/phase4/formal-toolchain-v1.json",
))


def load(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def collect_run_ids(value: Any) -> list[str]:
    rows: list[str] = []

    def visit(current: Any) -> None:
        if isinstance(current, dict):
            for key, item in current.items():
                if key == "run_id" and isinstance(item, str):
                    rows.append(item)
                else:
                    visit(item)
        elif isinstance(current, list):
            for item in current:
                visit(item)

    visit(value)
    return list(dict.fromkeys(rows))


def verify_candidate() -> dict[str, Any]:
    candidate = load(CANDIDATE)
    if candidate.get("status") != "frozen" or candidate.get("validity_level") != "paper-candidate":
        raise Phase4ValidationError("S4.8 candidate is not frozen paper-candidate")
    authorization = candidate.get("authorization", {})
    if authorization.get("e6_authorized") is not True or authorization.get("performance_claims_authorized") is not False:
        raise Phase4ValidationError("S4.8 candidate authorization boundary differs")
    if (CANDIDATE.stat().st_mode & 0o777) != 0o444:
        raise Phase4ValidationError("S4.8 candidate manifest is not mode 0444")
    frozen = candidate["frozen_identity"]
    expected = {
        FROZEN_EXECUTOR: frozen["executor_sha256"],
        RUNNER: frozen["runner_sha256"],
        CHECKER: frozen["checker_sha256"],
    }
    for path, digest in expected.items():
        if sha256_file(path) != digest:
            raise Phase4ValidationError(f"frozen correctness tool drift: {path}")
    return candidate


def executor_command(candidate: dict[str, Any]) -> list[str]:
    command = [
        str(NODE), str(WRAPPER),
        "--frozen-executor", str(FROZEN_EXECUTOR),
        "--frozen-executor-sha256", candidate["frozen_identity"]["executor_sha256"],
        "--candidate-manifest", str(CANDIDATE),
        "--candidate-sha256", sha256_file(CANDIDATE),
        "--validator", str(VALIDATOR),
        "--plan-builder", str(PLAN_BUILDER),
    ]
    for dependency in DEPENDENCIES:
        command.extend(("--dependency", str(dependency)))
    return command


def build_all(created_at: datetime | None = None) -> list[dict[str, Any]]:
    candidate = verify_candidate()
    upstream = collect_run_ids({
        "e0": candidate["e0_reproducibility_runs"],
        "correctness": candidate["correctness_runs"],
    })
    if len(upstream) != 18:
        raise Phase4ValidationError(f"candidate must bind 18 E0-E5 runs, got {len(upstream)}")
    timestamp = created_at or datetime.now(ZoneInfo("Asia/Shanghai")).replace(microsecond=0)
    manifests = [
        build_preflight(
            exp_root=EXP_ROOT,
            baseline_id=baseline,
            family="E6",
            executor_command=executor_command(candidate),
            created_at=timestamp,
            approved_candidate_id=candidate["candidate_id"],
            upstream_run_ids=upstream,
        )
        for baseline in BASELINES
    ]
    for manifest in manifests:
        validate_preflight(manifest, exp_root=EXP_ROOT)
        if manifest["inputs"]["repetitions"] != 10 or len(manifest["planned_execution"]) != 30:
            raise Phase4ValidationError("E6 preflight does not freeze R=10 x three operations")
    return manifests


def summary(manifests: Iterable[dict[str, Any]]) -> dict[str, Any]:
    rows = []
    for manifest in manifests:
        rows.append({
            "baseline_id": manifest["baseline"]["baseline_id"],
            "run_id": manifest["run_id"],
            "preflight_path": str(Path(manifest["paths"]["raw"]) / "manifest.preflight.json"),
            "units": len(manifest["planned_execution"]),
            "repetitions": manifest["inputs"]["repetitions"],
            "seed": manifest["inputs"]["seed"],
            "candidate_id": manifest["prerequisites"]["approved_s4_8_candidate_id"],
            "executor_sha256": sha256_file(WRAPPER),
            "validator_sha256": sha256_file(VALIDATOR),
            "plan_builder_sha256": sha256_file(PLAN_BUILDER),
        })
    return {
        "status": "PASS",
        "family": "E6",
        "candidate_manifest_sha256": sha256_file(CANDIDATE),
        "runs": rows,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "render", "create-all"))
    parser.add_argument("--baseline", choices=BASELINES)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    manifests = build_all()
    if args.command == "render":
        if args.baseline is None or args.output is None:
            parser.error("render requires --baseline and --output")
        manifest = next(item for item in manifests if item["baseline"]["baseline_id"] == args.baseline)
        write_json_exclusive(args.output, manifest)
    elif args.command == "create-all":
        for manifest in manifests:
            path = Path(manifest["paths"]["raw"]) / "manifest.preflight.json"
            write_json_exclusive(path, manifest)
    print(json.dumps(summary(manifests), sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Phase4ValidationError as error:
        print(f"ERROR: {error}", file=__import__("sys").stderr)
        raise SystemExit(2)
