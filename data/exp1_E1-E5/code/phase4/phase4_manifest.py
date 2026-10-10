"""Preflight generation, sealing, and immutable artifact validation."""

from __future__ import annotations

import argparse
import json
import os
import platform
import shlex
import shutil
import socket
import subprocess
from datetime import datetime
from pathlib import Path
from typing import Any, Mapping, Sequence
from zoneinfo import ZoneInfo

from phase4_contract import (
    BASELINES,
    CATALOG_PATH,
    CORRECTNESS_WORKLOAD,
    COST_WORKLOAD,
    DEV_CONFIG,
    EXP_ROOT,
    PORTS,
    TOOLING_VERSION,
    Phase4ValidationError,
    load_catalog,
    load_json,
    planned_execution,
    relative_posix,
    select_scenarios,
    sha256_file,
    tool_paths,
    validate_phase3_gate,
    validate_preflight,
    validate_seal,
    write_json_exclusive,
)


BASELINE_NAMES = {"B1": "Contract-only", "B2": "Runtime-only", "G3": "AssetXChain-CRS"}
TOOL_IDS = {
    "phase4_contract.py": "contract",
    "phase4_manifest.py": "manifest",
    "phase4_checker.py": "checker",
    "phase4_runner.py": "runner",
    "process_formal.py": "processor-reporter",
    "validate_phase4_readiness.py": "readiness-validator",
    "run-preflight.schema.json": "preflight-schema",
    "run-seal.schema.json": "seal-schema",
    "formal-result.schema.json": "result-schema",
    "attempt-block-evidence.schema.json": "block-evidence-schema",
    "normalized-snapshot.schema.json": "snapshot-schema",
    "formal-scenarios-v1.json": "scenario-catalog",
}


def _run(command: Sequence[str], *, cwd: Path | None = None, required: bool = True) -> str:
    completed = subprocess.run(command, cwd=cwd, text=True, capture_output=True, check=False)
    if required and completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise Phase4ValidationError(f"command failed ({' '.join(command)}): {detail}")
    return completed.stdout.strip()


def _git(worktree: Path, *args: str, required: bool = True) -> str:
    return _run(("git", "-C", str(worktree), *args), required=required)


def _version(command: Sequence[str], fallback: str | None = None) -> str:
    try:
        value = _run(command).splitlines()[0]
    except (FileNotFoundError, Phase4ValidationError):
        if fallback is None:
            raise
        value = fallback
    if not value:
        raise Phase4ValidationError(f"empty version from {command[0]}")
    return value


def _memory_total() -> int:
    for line in Path("/proc/meminfo").read_text(encoding="ascii").splitlines():
        if line.startswith("MemTotal:"):
            return int(line.split()[1]) * 1024
    raise Phase4ValidationError("cannot read total memory")


def _node_versions() -> tuple[str, str]:
    candidates = []
    explicit = os.environ.get("ASSETX_NODEJS")
    if explicit:
        candidates.append(Path(explicit))
    found = shutil.which("node")
    if found:
        candidates.append(Path(found))
    local_node = Path.home() / ".local/node-v24.15.0-linux-x64/bin/node"
    if local_node.is_file():
        candidates.append(local_node)
    windows_node = Path("/mnt/d/nodejs/node.exe")
    if windows_node.is_file():
        candidates.append(windows_node)
    for candidate in candidates:
        try:
            node = _version((str(candidate), "--version"))
            npm_script = candidate.parent.parent / "lib" / "node_modules" / "npm" / "bin" / "npm-cli.js"
            if not npm_script.is_file():
                npm_script = candidate.parent / "node_modules" / "npm" / "bin" / "npm-cli.js"
            if npm_script.is_file():
                npm = _version((str(candidate), str(npm_script), "--version"))
            else:
                npm_command = shutil.which("npm")
                npm = _version((npm_command, "--version")) if npm_command else "unavailable"
            return f"{candidate}:{node}", npm
        except (OSError, Phase4ValidationError):
            continue
    raise Phase4ValidationError("no usable Node.js executable is identifiable")


def capture_environment(exp_root: Path) -> dict[str, Any]:
    nodejs, npm = _node_versions()
    cargo_home = Path.home() / ".cargo/bin"
    rustc_path = Path(shutil.which("rustc") or cargo_home / "rustc")
    cargo_path = Path(shutil.which("cargo") or cargo_home / "cargo")
    protoc_path = Path(shutil.which("protoc") or "/usr/bin/protoc")
    usage = shutil.disk_usage(exp_root)
    os_release = {}
    for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
        if "=" in line:
            key, value = line.split("=", 1)
            os_release[key] = value.strip('"')
    return {
        "os": os_release.get("PRETTY_NAME", platform.platform()),
        "kernel": platform.release(),
        "wsl_version": os_release.get("VERSION", "unknown"),
        "cpu_logical": os.cpu_count() or 1,
        "memory_total_bytes": _memory_total(),
        "disk_available_bytes": usage.free,
        "rustc": f"{rustc_path}:{_version((str(rustc_path), '--version'))}",
        "cargo": f"{cargo_path}:{_version((str(cargo_path), '--version'))}",
        "python": f"{Path(os.sys.executable).resolve()}:{platform.python_version()}",
        "protoc": f"{protoc_path}:{_version((str(protoc_path), '--version'))}",
        "nodejs": nodejs,
        "npm": npm,
    }


def _path_hash(exp_root: Path, relative: Path) -> dict[str, str]:
    path = exp_root / relative
    if not path.is_file():
        raise Phase4ValidationError(f"required frozen file is missing: {relative}")
    return {"path": relative.as_posix(), "sha256": sha256_file(path)}


def _baseline_identity(exp_root: Path, baseline_id: str) -> tuple[dict[str, Any], dict[str, Any]]:
    manifest_path = exp_root / f"configs/baselines/{baseline_id.lower()}.json"
    frozen = load_json(manifest_path)
    worktree = Path(frozen["git"]["worktree"])
    actual = {
        "baseline_id": baseline_id,
        "baseline_name": BASELINE_NAMES[baseline_id],
        "baseline_profile_version": frozen["baseline_profile_version"],
        "worktree": str(worktree),
        "branch": _git(worktree, "branch", "--show-current"),
        "head": _git(worktree, "rev-parse", "HEAD"),
        "tree": _git(worktree, "rev-parse", "HEAD^{tree}"),
        "parent": _git(worktree, "rev-parse", "HEAD^"),
        "dirty": bool(_git(worktree, "status", "--porcelain=v1")),
        "formal_base_commit": frozen["git"]["formal_base_commit"],
        "formal_base_is_ancestor": subprocess.run(
            ["git", "-C", str(worktree), "merge-base", "--is-ancestor", frozen["git"]["formal_base_commit"], "HEAD"],
            check=False,
        ).returncode == 0,
    }
    expected = frozen["git"]
    if actual["head"] != expected["head"] or actual["branch"] != expected["branch"]:
        raise Phase4ValidationError(f"{baseline_id} Git identity differs from the frozen baseline manifest")
    if actual["dirty"] or not actual["formal_base_is_ancestor"]:
        raise Phase4ValidationError(f"{baseline_id} is not a clean formal-base descendant")
    return actual, frozen


def _fixture(dev: Mapping[str, Any], exp_root: Path) -> dict[str, Any]:
    fixture = dev["shared_fixture"]
    common = load_json(exp_root / "configs/baselines/phase3-common.json")
    vector_relative = Path(common["fixture_contract"]["vector_bundle"])
    roles = {role: value["account_id_hex"] for role, value in fixture["roles"].items()}
    tx = fixture["transactions"]
    return {
        "fixture_profile_id": fixture["fixture_profile_id"],
        "fixture_config_sha256": sha256_file(exp_root / DEV_CONFIG),
        "vector_bundle_id": common["fixture_contract"]["vector_bundle_id"],
        "vector_bundle": vector_relative.as_posix(),
        "vector_bundle_sha256": sha256_file(exp_root / vector_relative),
        "role_account_ids": roles,
        "quantity": tx["quantity"],
        "prices_plancks": {
            "TransferMeta": tx["meta_sale_price_plancks"],
            "IssueRight": tx["right_issue_price_plancks"],
            "TransferRight": tx["right_resale_price_plancks"],
        },
        "market_fee_bps": tx["market_fee_bps"],
        "safety_fee_bps": tx["safety_fee_bps"],
        "timing": fixture["timing"],
        "capacity": {
            "issue_right_authorization_capacity": 4,
            "transfer_meta_active_rights": 0,
            "transfer_right_active_rights": 4,
        },
        "funding_policy": fixture["funding"],
    }


def _execution(frozen: Mapping[str, Any], executor_command: Sequence[str]) -> dict[str, Any]:
    if not executor_command or any(not isinstance(item, str) or not item for item in executor_command):
        raise Phase4ValidationError("executor command must be a nonempty argv array")
    worktree = Path(frozen["git"]["worktree"])
    node_binary = worktree / "target/release/assetxchain-node"
    artifact_paths = [str(node_binary)]
    baseline_id = frozen["baseline_id"]
    if baseline_id == "B1":
        artifact_paths.extend([
            str(worktree / "contracts/market_pure_contract/target/ink/market_pure_contract.wasm"),
            str(worktree / "contracts/market_pure_contract/target/ink/market_pure_contract.json"),
        ])
    elif baseline_id == "G3":
        artifact_paths.extend([
            str(worktree / "target/ink/market_orderbook/market_orderbook.wasm"),
            str(worktree / "target/ink/market_orderbook/market_orderbook.json"),
        ])
    hashes = [{"path": path, "sha256": sha256_file(Path(path))} for path in artifact_paths if Path(path).is_file()]
    executor_artifacts = []
    for token in executor_command:
        candidate = Path(token)
        if candidate.is_file():
            executor_artifacts.append({"path": str(candidate.resolve()), "sha256": sha256_file(candidate)})
    if not executor_artifacts:
        resolved = shutil.which(executor_command[0])
        if resolved:
            executor_artifacts.append({"path": str(Path(resolved).resolve()), "sha256": sha256_file(Path(resolved))})
    if not executor_artifacts:
        raise Phase4ValidationError("executor command has no hashable executable or script")
    build_commands = [entry["command"] for entry in frozen["build"]["entries"]]
    contract_build = {
        "B1": "cargo contract build --release --manifest-path contracts/market_pure_contract/Cargo.toml",
        "G3": "cargo contract build --release --manifest-path contracts/market_orderbook/Cargo.toml",
    }.get(baseline_id)
    if contract_build and contract_build not in build_commands:
        build_commands.append(contract_build)
    node_build = "cargo build --release -p assetxchain-node"
    if node_build not in build_commands:
        build_commands.append(node_build)
    return {
        "executor_command": list(executor_command),
        "executor_artifacts": executor_artifacts,
        "node_command": [str(node_binary), "--dev", "--tmp", "--rpc-cors", "all", "--rpc-methods", "unsafe"],
        "node_cwd": str(worktree),
        "chain_mode": "dev-tmp",
        "rpc_endpoint": "ws://127.0.0.1:9944",
        "ports": list(PORTS),
        "readiness_timeout_seconds": 90,
        "reset_policy": "fresh-owned-node-before-each-live-scenario-repetition",
        "teardown_policy": "terminate-owned-process-and-require-9944-30333-9615-closed",
        "build_commands": build_commands,
        "artifact_paths": artifact_paths,
        "artifact_hashes": hashes,
    }


def build_preflight(
    *,
    exp_root: Path,
    baseline_id: str,
    family: str,
    executor_command: Sequence[str],
    created_at: datetime | None = None,
    approved_candidate_id: str | None = None,
    upstream_run_ids: Sequence[str] = (),
) -> dict[str, Any]:
    validate_phase3_gate(exp_root)
    if baseline_id not in BASELINES:
        raise Phase4ValidationError(f"unsupported baseline: {baseline_id}")
    catalog = load_catalog(exp_root=exp_root)
    scenarios = select_scenarios(catalog, family)
    workload_path = COST_WORKLOAD if family == "E6" else CORRECTNESS_WORKLOAD
    workload = load_json(exp_root / workload_path)
    metadata = workload["metadata"]
    repetitions = 2 if family == "E0" else metadata["repetitions"]
    identity, frozen = _baseline_identity(exp_root, baseline_id)
    dev = load_json(exp_root / DEV_CONFIG)
    timestamp = created_at or datetime.now(ZoneInfo("Asia/Shanghai"))
    if timestamp.tzinfo is None:
        timestamp = timestamp.replace(tzinfo=ZoneInfo("Asia/Shanghai"))
    timestamp = timestamp.astimezone(ZoneInfo("Asia/Shanghai")).replace(microsecond=0)
    workload_hash = sha256_file(exp_root / workload_path)
    stamp = timestamp.strftime("%Y%m%dT%H%M%S+0800")
    run_id = f"p4-{family.lower()}-{baseline_id.lower()}-{stamp}-{identity['head'][:8]}-{workload_hash[:8]}"
    raw = exp_root / "results/raw" / run_id
    log = exp_root / "logs" / run_id
    processed = exp_root / "results/processed" / run_id
    report = exp_root / "reports" / f"{run_id}.md"
    for path in (raw, log, processed, report):
        if path.exists():
            raise Phase4ValidationError(f"run ID/path already exists: {path}")
    selected_ids = [item["scenario_id"] for item in scenarios]
    tools = []
    for relative in tool_paths():
        path = exp_root / relative
        if not path.is_file():
            raise Phase4ValidationError(f"formal tool is missing: {relative}")
        tools.append({
            "tool_id": TOOL_IDS[path.name],
            "path": relative.as_posix(),
            "version": TOOLING_VERSION,
            "sha256": sha256_file(path),
        })
    adapter_relative = Path(frozen["adapter"]["entrypoint"].split("#", 1)[0])
    tools.append({
        "tool_id": "baseline-adapter",
        "path": adapter_relative.as_posix(),
        "version": frozen["contract_version"],
        "sha256": sha256_file(exp_root / adapter_relative),
    })
    manifest = {
        "schema_id": "assetxchain-exp2-phase4-run-preflight",
        "schema_version": 1,
        "tooling_version": TOOLING_VERSION,
        "run_id": run_id,
        "status": "planned",
        "created_at": timestamp.isoformat(),
        "timezone": "Asia/Shanghai",
        "validity_level": "provisional-candidate",
        "experiment_family": family,
        "claim_boundary": "formal-correctness-only" if family != "E6" else "formal-cost-after-approved-correctness-candidate",
        "baseline": identity,
        "inputs": {
            "environment_config": _path_hash(exp_root, DEV_CONFIG),
            "workload": _path_hash(exp_root, workload_path),
            "scenario_catalog": _path_hash(exp_root, CATALOG_PATH),
            "baseline_manifest": _path_hash(exp_root, Path(f"configs/baselines/{baseline_id.lower()}.json")),
            "workload_id": metadata["workload_id"],
            "workload_class": metadata["workload_class"],
            "seed": metadata["seed"],
            "repetitions": repetitions,
            "selected_scenario_ids": selected_ids,
            "required_invariants": catalog["required_invariants"],
            "deferred_invariants": catalog["deferred_invariants"],
        },
        "protocol": dev["protocol"],
        "fixture": _fixture(dev, exp_root),
        "tools": tools,
        "execution": _execution(frozen, executor_command),
        "environment": capture_environment(exp_root),
        "paths": {"raw": str(raw), "log": str(log), "processed": str(processed), "report": str(report)},
        "prerequisites": {
            "phase3_gate": "PASS",
            "ready_for_phase4": True,
            "upstream_run_ids": list(upstream_run_ids),
            "approved_s4_8_candidate_id": approved_candidate_id,
        },
        "planned_execution": planned_execution(scenarios, repetitions, metadata["seed"]),
        "capture_at_run": {"started_at": None, "finished_at": None, "genesis_hashes": None, "fixture_anchor_blocks": None, "start_blocks": None, "end_blocks": None},
    }
    validate_preflight(manifest, exp_root=exp_root)
    return manifest


def create_preflight(**kwargs: Any) -> tuple[dict[str, Any], Path]:
    manifest = build_preflight(**kwargs)
    path = Path(manifest["paths"]["raw"]) / "manifest.preflight.json"
    write_json_exclusive(path, manifest)
    return manifest, path


def inventory_artifacts(raw_root: Path, log_root: Path) -> list[dict[str, Any]]:
    rows = []
    for label, root in (("raw", raw_root), ("log", log_root)):
        if not root.exists():
            continue
        for path in root.rglob("*"):
            if path.is_symlink():
                raise Phase4ValidationError(f"symlink is forbidden in formal artifacts: {path}")
            if not path.is_file() or (label == "raw" and path.name == "manifest.seal.json"):
                continue
            rows.append({"root": label, "path": relative_posix(path, root), "bytes": path.stat().st_size, "sha256": sha256_file(path)})
    return sorted(rows, key=lambda row: (row["root"], row["path"]))


def create_seal(
    *,
    preflight: Mapping[str, Any],
    status: str,
    started_at: str,
    finished_at: str,
    genesis_hashes: Mapping[str, str],
    fixture_anchor_blocks: Mapping[str, int],
    attempt_counts: Mapping[str, int],
    teardown: Mapping[str, Any],
    checker: Mapping[str, Any],
    failure_reason: str | None,
) -> tuple[dict[str, Any], Path]:
    raw_root = Path(preflight["paths"]["raw"])
    log_root = Path(preflight["paths"]["log"])
    preflight_path = raw_root / "manifest.preflight.json"
    if not preflight_path.is_file():
        raise Phase4ValidationError("preflight manifest is missing before sealing")
    seal = {
        "schema_id": "assetxchain-exp2-phase4-run-seal",
        "schema_version": 1,
        "tooling_version": TOOLING_VERSION,
        "run_id": preflight["run_id"],
        "status": status,
        "started_at": started_at,
        "finished_at": finished_at,
        "timezone": "Asia/Shanghai",
        "preflight_sha256": sha256_file(preflight_path),
        "genesis_hashes": dict(genesis_hashes),
        "fixture_anchor_blocks": dict(fixture_anchor_blocks),
        "attempt_counts": dict(attempt_counts),
        "teardown": dict(teardown),
        "checker": dict(checker),
        "failure_reason": failure_reason,
        "artifact_inventory": inventory_artifacts(raw_root, log_root),
    }
    validate_seal(seal, preflight)
    path = raw_root / "manifest.seal.json"
    write_json_exclusive(path, seal)
    for root in (raw_root, log_root):
        if root.exists():
            for artifact in root.rglob("*"):
                if artifact.is_file():
                    artifact.chmod(0o444)
            for directory in sorted((item for item in root.rglob("*") if item.is_dir()), reverse=True):
                directory.chmod(0o555)
            root.chmod(0o555)
    return seal, path


def validate_artifacts(raw_root: Path, *, verify_cross_references: bool = True) -> dict[str, Any]:
    preflight_path = raw_root / "manifest.preflight.json"
    seal_path = raw_root / "manifest.seal.json"
    if not preflight_path.is_file() or not seal_path.is_file():
        raise Phase4ValidationError("sealed raw requires preflight and seal manifests")
    preflight = load_json(preflight_path)
    seal = load_json(seal_path)
    validate_preflight(preflight, exp_root=EXP_ROOT)
    validate_seal(seal, preflight)
    if seal["preflight_sha256"] != sha256_file(preflight_path):
        raise Phase4ValidationError("preflight bytes changed after sealing")
    log_root = Path(preflight["paths"]["log"])
    actual = inventory_artifacts(raw_root, log_root)
    if actual != seal["artifact_inventory"]:
        expected_map = {(row["root"], row["path"]): row for row in seal["artifact_inventory"]}
        actual_map = {(row["root"], row["path"]): row for row in actual}
        raise Phase4ValidationError(
            f"artifact inventory drift; missing={sorted(set(expected_map)-set(actual_map))}, "
            f"extra={sorted(set(actual_map)-set(expected_map))}, "
            f"changed={sorted(key for key in set(expected_map)&set(actual_map) if expected_map[key] != actual_map[key])}"
        )
    result_files = sorted(raw_root.glob("attempts/*/r-*/attempt-*/result.json"))
    if verify_cross_references:
        selected = set(preflight["inputs"]["selected_scenario_ids"])
        for path in result_files:
            result = load_json(path)
            parts = path.relative_to(raw_root).parts
            scenario_id = parts[1]
            repetition = int(parts[2][2:])
            if result["run"]["run_id"] != preflight["run_id"] or result["run"]["baseline_id"] != preflight["baseline"]["baseline_id"]:
                raise Phase4ValidationError(f"result run/baseline cross-reference mismatch: {path}")
            if result["request"]["scenario_id"] != scenario_id or result["run"]["repetition"] != repetition:
                raise Phase4ValidationError(f"result path/content cross-reference mismatch: {path}")
            if scenario_id not in selected:
                raise Phase4ValidationError(f"unplanned scenario result: {scenario_id}")
    return {"status": "PASS", "run_id": preflight["run_id"], "artifacts": len(actual), "results": len(result_files), "seal_status": seal["status"]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    build = subparsers.add_parser("build", help="print a validated preflight without creating a run")
    create = subparsers.add_parser("create", help="exclusively create a formal preflight manifest")
    for child in (build, create):
        child.add_argument("--baseline", required=True, choices=BASELINES)
        child.add_argument("--family", required=True, choices=("E0", "E1", "E2", "E3", "E4", "E5", "E6"))
        child.add_argument("--executor-command", required=True, help="shell-like argv recorded after shlex parsing")
        child.add_argument("--approved-candidate-id")
    validate = subparsers.add_parser("validate")
    validate.add_argument("raw_root", type=Path)
    args = parser.parse_args()
    if args.command == "validate":
        print(json.dumps(validate_artifacts(args.raw_root), sort_keys=True))
        return 0
    kwargs = {
        "exp_root": EXP_ROOT,
        "baseline_id": args.baseline,
        "family": args.family,
        "executor_command": shlex.split(args.executor_command),
        "approved_candidate_id": args.approved_candidate_id,
    }
    if args.command == "build":
        print(json.dumps(build_preflight(**kwargs), sort_keys=True))
    else:
        manifest, path = create_preflight(**kwargs)
        print(json.dumps({"run_id": manifest["run_id"], "path": str(path)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Phase4ValidationError as error:
        print(f"ERROR: {error}", file=os.sys.stderr)
        raise SystemExit(2)
