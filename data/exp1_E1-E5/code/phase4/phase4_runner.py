"""Execute one sealed Phase 4 baseline/family batch from an immutable preflight."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import socket
import subprocess
import time
from datetime import datetime
from pathlib import Path
from typing import Any, Mapping, Sequence
from zoneinfo import ZoneInfo

from phase4_checker import check_run, seal_summary
from phase4_contract import (
    PORTS,
    Phase4ValidationError,
    load_catalog,
    load_json,
    scenario_map,
    sha256_file,
    validate_preflight,
    write_json_exclusive,
)
from phase4_manifest import create_seal


TOOLCHAIN_PROFILE = Path(__file__).resolve().parents[2] / "configs/phase4/formal-toolchain-v1.json"


def _now() -> str:
    return datetime.now(ZoneInfo("Asia/Shanghai")).replace(microsecond=0).isoformat()


def _port_open(port: int) -> bool:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.settimeout(0.2)
        return sock.connect_ex(("127.0.0.1", port)) == 0


def _open_ports() -> list[int]:
    return [port for port in PORTS if _port_open(port)]


def _wait_rpc(process: subprocess.Popen[bytes], timeout: int) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise Phase4ValidationError(f"node exited before RPC readiness with code {process.returncode}")
        if _port_open(9944):
            return
        time.sleep(0.2)
    raise Phase4ValidationError("RPC readiness timeout")


def _stop_node(process: subprocess.Popen[bytes], timeout: int = 15) -> None:
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=timeout)
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not _open_ports():
            return
        time.sleep(0.2)
    raise Phase4ValidationError(f"owned node teardown left listeners on {_open_ports()}")


def _check_git(preflight: Mapping[str, Any]) -> None:
    baseline = preflight["baseline"]
    worktree = baseline["worktree"]
    head = subprocess.run(["git", "-C", worktree, "rev-parse", "HEAD"], text=True, capture_output=True, check=True).stdout.strip()
    status = subprocess.run(["git", "-C", worktree, "status", "--porcelain=v1"], text=True, capture_output=True, check=True).stdout.strip()
    if head != baseline["head"] or status:
        raise Phase4ValidationError("baseline Git identity changed after preflight")


def _formal_environment(preflight: Mapping[str, Any]) -> tuple[dict[str, str], dict[str, Any]]:
    profile = load_json(TOOLCHAIN_PROFILE)
    if profile.get("schema_id") != "assetxchain-exp2-phase4-toolchain-profile" or profile.get("schema_version") != 1:
        raise Phase4ValidationError("formal toolchain profile identity drift")
    baseline_id = preflight["baseline"]["baseline_id"]
    family = preflight["experiment_family"]
    if baseline_id not in profile["applies_to_baselines"] or family not in profile["applies_to_families"]:
        raise Phase4ValidationError("formal toolchain profile does not apply to this run")

    profile_path = TOOLCHAIN_PROFILE.resolve()
    captured = [
        item for item in preflight["execution"]["executor_artifacts"]
        if Path(item["path"]).resolve() == profile_path
    ]
    if len(captured) != 1 or captured[0]["sha256"] != sha256_file(profile_path):
        raise Phase4ValidationError("formal toolchain profile is not uniquely hash-captured")

    versions: dict[str, str] = {}
    for name in ("rustc", "cargo", "cargo_contract"):
        tool = profile[name]
        tool_path = Path(tool["path"])
        if not tool_path.is_file() or sha256_file(tool_path) != tool["sha256"]:
            raise Phase4ValidationError(f"formal {name} executable hash drift")
        completed = subprocess.run(
            [str(tool_path), "--version"], text=True, capture_output=True, check=False,
        )
        actual = completed.stdout.strip()
        if completed.returncode != 0 or actual != tool["version"]:
            raise Phase4ValidationError(f"formal {name} version drift: {actual}")
        versions[name] = actual

    environment = os.environ.copy()
    environment["RUSTUP_TOOLCHAIN"] = profile["environment"]["RUSTUP_TOOLCHAIN"]
    prefixes = [str(Path(value)) for value in profile["environment"]["path_prefixes"]]
    environment["PATH"] = os.pathsep.join([*prefixes, environment.get("PATH", "")])
    for command, expected in (("rustc", profile["rustc"]["version"]), ("cargo", profile["cargo"]["version"])):
        completed = subprocess.run(
            [str(Path.home() / f".cargo/bin/{command}"), "--version"],
            env=environment, text=True, capture_output=True, check=False,
        )
        if completed.returncode != 0 or completed.stdout.strip() != expected:
            raise Phase4ValidationError(f"formal rustup proxy did not select {expected}")

    evidence = {
        "path": str(profile_path),
        "sha256": sha256_file(profile_path),
        "profile_id": profile["profile_id"],
        "environment": {
            "RUSTUP_TOOLCHAIN": environment["RUSTUP_TOOLCHAIN"],
            "path_prefixes": prefixes,
        },
        "versions": versions,
    }
    return environment, evidence


class RunnerLog:
    def __init__(self, path: Path) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        try:
            self.handle = path.open("x", encoding="utf-8", newline="\n")
        except FileExistsError as error:
            raise Phase4ValidationError(f"runner log already exists: {path}") from error

    def write(self, event: str, **fields: Any) -> None:
        row = {"timestamp": _now(), "event": event, **fields}
        self.handle.write(json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n")
        self.handle.flush()
        os.fsync(self.handle.fileno())

    def close(self) -> None:
        self.handle.close()


def _run_builds(
    preflight: Mapping[str, Any],
    log: RunnerLog,
    environment: Mapping[str, str],
    toolchain: Mapping[str, Any],
) -> None:
    cwd = Path(preflight["execution"]["node_cwd"])
    for command_text in preflight["execution"]["build_commands"]:
        argv = shlex.split(command_text)
        if not argv:
            raise Phase4ValidationError("empty build command")
        if argv[0] == "cargo":
            argv[0] = str(Path.home() / ".cargo/bin/cargo")
        log.write("build-start", argv=argv, cwd=str(cwd))
        completed = subprocess.run(
            argv, cwd=cwd, env=environment, text=True, capture_output=True, check=False,
        )
        log.write("build-finish", argv=argv, returncode=completed.returncode, stdout=completed.stdout, stderr=completed.stderr)
        if completed.returncode != 0:
            raise Phase4ValidationError(f"build failed: {command_text}")
    artifacts = []
    for value in preflight["execution"]["artifact_paths"]:
        artifact = Path(value)
        if not artifact.is_file():
            raise Phase4ValidationError(f"required build artifact is missing: {artifact}")
        artifacts.append({
            "path": str(artifact),
            "bytes": artifact.stat().st_size,
            "sha256": sha256_file(artifact),
        })
    write_json_exclusive(Path(preflight["paths"]["raw"]) / "build-artifacts.json", {
        "schema_id": "assetxchain-exp2-phase4-build-artifacts",
        "schema_version": 1,
        "run_id": preflight["run_id"],
        "baseline_id": preflight["baseline"]["baseline_id"],
        "implementation_commit": preflight["baseline"]["head"],
        "commands": preflight["execution"]["build_commands"],
        "toolchain": toolchain,
        "artifacts": artifacts,
    })


def _run_executor(
    preflight_path: Path,
    preflight: Mapping[str, Any],
    unit: Mapping[str, Any],
    output_root: Path,
    log: RunnerLog,
    environment: Mapping[str, str],
) -> None:
    output_root.mkdir(parents=True, exist_ok=False)
    argv = [
        *preflight["execution"]["executor_command"],
        "--preflight", str(preflight_path),
        "--scenario-id", unit["scenario_id"],
        "--repetition", str(unit["repetition"]),
        "--execution-mode", unit["execution_mode"],
        "--output-root", str(output_root),
        "--rpc", preflight["execution"]["rpc_endpoint"],
    ]
    log.write("executor-start", sequence=unit["sequence"], argv=argv)
    completed = subprocess.run(
        argv, cwd=preflight["execution"]["node_cwd"], env=environment,
        text=True, capture_output=True, check=False,
    )
    log.write("executor-finish", sequence=unit["sequence"], returncode=completed.returncode, stdout=completed.stdout, stderr=completed.stderr)
    if completed.returncode != 0:
        raise Phase4ValidationError(f"scenario executor failed for {unit['scenario_id']} r={unit['repetition']}")
    unit_manifest = output_root / "unit-manifest.json"
    if not unit_manifest.is_file():
        raise Phase4ValidationError(f"scenario executor omitted {unit_manifest}")
    metadata = load_json(unit_manifest)
    required_fields = {
        "schema_id", "schema_version", "run_id", "baseline_id",
        "scenario_id", "repetition", "execution_mode", "genesis_hash",
        "fixture_anchor_block", "start_block", "end_block", "metadata_hash",
        "fixture_state_hash", "scenario_result_hash", "shared_input_hash",
        "attempt_count", "artifact_inventory",
    }
    if set(metadata) != required_fields:
        raise Phase4ValidationError("unit manifest fields differ from the frozen E0 contract")
    if metadata["schema_id"] != "assetxchain-exp2-phase4-e0-unit-manifest" or metadata["schema_version"] != 1:
        raise Phase4ValidationError("unit manifest schema identity drift")
    expected = {
        "run_id": preflight["run_id"],
        "baseline_id": preflight["baseline"]["baseline_id"],
        "scenario_id": unit["scenario_id"],
        "repetition": unit["repetition"],
        "execution_mode": unit["execution_mode"],
    }
    for field, value in expected.items():
        if metadata.get(field) != value:
            raise Phase4ValidationError(f"unit manifest {field} cross-reference mismatch")
    if unit["execution_mode"] == "live-chain":
        for field in ("genesis_hash", "fixture_anchor_block", "start_block", "end_block"):
            if metadata.get(field) is None:
                raise Phase4ValidationError(f"live unit manifest omitted {field}")
        for field in ("genesis_hash", "metadata_hash", "fixture_state_hash", "scenario_result_hash", "shared_input_hash"):
            value = metadata.get(field)
            if not isinstance(value, str) or not value.startswith("0x") or len(value) != 66:
                raise Phase4ValidationError(f"live unit manifest has invalid {field}")
        if metadata["attempt_count"] != len(scenario_map(load_catalog())[unit["scenario_id"]]["attempts"]):
            raise Phase4ValidationError("unit manifest attempt count differs from scenario contract")
        actual_inventory = []
        for artifact in sorted(path for path in output_root.rglob("*") if path.is_file() and path.name != "unit-manifest.json"):
            relative = artifact.relative_to(output_root).as_posix()
            digest = hashlib.sha256(artifact.read_bytes()).hexdigest()
            actual_inventory.append({"path": relative, "bytes": artifact.stat().st_size, "sha256": digest})
        if metadata["artifact_inventory"] != actual_inventory:
            raise Phase4ValidationError("unit manifest artifact inventory differs from emitted bytes")
    elif any(metadata.get(field) is not None for field in ("genesis_hash", "fixture_anchor_block", "start_block", "end_block")):
        raise Phase4ValidationError("test-gate unit must not fabricate chain metadata")


def plan(preflight_path: Path) -> dict[str, Any]:
    preflight = load_json(preflight_path)
    validate_preflight(preflight)
    if Path(preflight["paths"]["raw"]) != preflight_path.parent:
        raise Phase4ValidationError("preflight path differs from its frozen raw path")
    planned_attempts = 0
    catalog = scenario_map(load_catalog())
    for unit in preflight["planned_execution"]:
        planned_attempts += len(catalog[unit["scenario_id"]]["attempts"])
    return {
        "status": "PASS",
        "run_id": preflight["run_id"],
        "units": len(preflight["planned_execution"]),
        "attempts": planned_attempts,
        "live_units": sum(unit["execution_mode"] == "live-chain" for unit in preflight["planned_execution"]),
        "test_gate_units": sum(unit["execution_mode"] == "test-gate" for unit in preflight["planned_execution"]),
    }


def run(preflight_path: Path, *, execute_builds: bool = True) -> dict[str, Any]:
    preflight = load_json(preflight_path)
    planned = plan(preflight_path)
    raw_root = Path(preflight["paths"]["raw"])
    log_root = Path(preflight["paths"]["log"])
    if (raw_root / "manifest.seal.json").exists():
        raise Phase4ValidationError("sealed run cannot be resumed")
    if _open_ports():
        raise Phase4ValidationError(f"formal ports are already occupied: {_open_ports()}")
    log = RunnerLog(log_root / "runner.log")
    started_at = _now()
    failure_reason: str | None = None
    genesis_hashes: dict[str, str] = {}
    anchors: dict[str, int] = {}
    owned_stopped = True
    residual_ports: list[int] = []
    try:
        log.write("run-start", run_id=preflight["run_id"], planned=planned)
        _check_git(preflight)
        environment, toolchain = _formal_environment(preflight)
        log.write("toolchain-profile", **toolchain)
        if execute_builds:
            _run_builds(preflight, log, environment, toolchain)
        _check_git(preflight)
        for unit in preflight["planned_execution"]:
            key = f"{unit['scenario_id']}/r-{unit['repetition']:03d}"
            output_root = raw_root / "attempts" / unit["scenario_id"] / f"r-{unit['repetition']:03d}"
            process: subprocess.Popen[bytes] | None = None
            node_handle = None
            try:
                if unit["execution_mode"] == "live-chain":
                    if _open_ports():
                        raise Phase4ValidationError(f"ports occupied before reset unit {key}: {_open_ports()}")
                    node_path = log_root / "node" / f"{unit['scenario_id']}-r-{unit['repetition']:03d}.log"
                    node_path.parent.mkdir(parents=True, exist_ok=True)
                    node_handle = node_path.open("xb")
                    process = subprocess.Popen(
                        preflight["execution"]["node_command"],
                        cwd=preflight["execution"]["node_cwd"],
                        env=environment,
                        stdout=node_handle,
                        stderr=subprocess.STDOUT,
                    )
                    _wait_rpc(process, preflight["execution"]["readiness_timeout_seconds"])
                _run_executor(preflight_path, preflight, unit, output_root, log, environment)
                unit_manifest = load_json(output_root / "unit-manifest.json")
                if unit["execution_mode"] == "live-chain":
                    genesis_hashes[key] = unit_manifest["genesis_hash"]
                    anchors[key] = unit_manifest["fixture_anchor_block"]
            finally:
                if process is not None:
                    try:
                        _stop_node(process)
                    except Phase4ValidationError:
                        owned_stopped = False
                        raise
                if node_handle is not None:
                    node_handle.close()
                residual_ports = _open_ports()
                if residual_ports:
                    raise Phase4ValidationError(f"ports remain open after unit {key}: {residual_ports}")
            _check_git(preflight)
        report = check_run(raw_root, write_report=True)
        if report["summary"]["status"] != "PASS":
            failure_reason = "formal checker failed"
    except Exception as error:
        failure_reason = str(error)
        log.write("run-error", error=type(error).__name__, detail=failure_reason)
        try:
            report = check_run(raw_root, write_report=True)
        except Exception as checker_error:
            report = {
                "summary": {
                    "status": "FAIL", "expected_outcome_mismatches": 0,
                    "invariant_violations": 0, "false_accepts": 0, "false_rejects": 0,
                    "duplicates": 0, "dirty_state": 0, "event_mismatches": 0,
                    "timeouts": 0, "completed_attempts": 0, "completed_units": 0,
                }
            }
            log.write("checker-error", detail=str(checker_error))
    finished_at = _now()
    residual_ports = _open_ports()
    if residual_ports:
        failure_reason = failure_reason or f"residual formal listeners: {residual_ports}"
    status = "completed" if failure_reason is None and report["summary"]["status"] == "PASS" else "invalid"
    attempt_counts = {
        "planned_units": planned["units"],
        "completed_units": report["summary"]["completed_units"],
        "planned_attempts": planned["attempts"],
        "completed_attempts": report["summary"]["completed_attempts"],
    }
    log.write("run-finish", status=status, failure_reason=failure_reason)
    log.close()
    seal, seal_path = create_seal(
        preflight=preflight,
        status=status,
        started_at=started_at,
        finished_at=finished_at,
        genesis_hashes=genesis_hashes,
        fixture_anchor_blocks=anchors,
        attempt_counts=attempt_counts,
        teardown={"owned_processes_stopped": owned_stopped, "closed_ports": [port for port in PORTS if port not in residual_ports], "residual_listener_count": len(residual_ports)},
        checker=seal_summary(report),
        failure_reason=failure_reason,
    )
    return {"run_id": preflight["run_id"], "status": status, "seal": str(seal_path), "checker": report["summary"]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("plan", "run"))
    parser.add_argument("preflight", type=Path)
    parser.add_argument("--skip-builds", action="store_true", help="test-only; forbidden for formal invocation")
    args = parser.parse_args()
    if args.command == "plan":
        result = plan(args.preflight)
    else:
        if args.skip_builds and os.environ.get("ASSETX_PHASE4_TEST_MODE") != "1":
            raise Phase4ValidationError("--skip-builds is allowed only in explicit tooling tests")
        result = run(args.preflight, execute_builds=not args.skip_builds)
    print(json.dumps(result, sort_keys=True))
    return 0 if result.get("status") in {"PASS", "completed"} else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Phase4ValidationError as error:
        print(f"ERROR: {error}", file=os.sys.stderr)
        raise SystemExit(2)
