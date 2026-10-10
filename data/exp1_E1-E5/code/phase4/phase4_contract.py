"""Frozen contracts shared by the AssetXChain Phase 4 formal tooling."""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import sys
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence


TOOLING_VERSION = "phase4-formal-tooling-v1"
EXP_ROOT = Path(__file__).resolve().parents[2]
COMMON_DIR = EXP_ROOT / "scripts" / "common"
if str(COMMON_DIR) not in sys.path:
    sys.path.insert(0, str(COMMON_DIR))

from phase3_contract import ContractValidationError, load_json, validate_instance


class Phase4ValidationError(ValueError):
    """A frozen Phase 4 contract was violated."""


BASELINES = ("B1", "B2", "G3")
FAMILIES = ("E0", "E1", "E2", "E3", "E4", "E5", "E6")
PORTS = (9944, 30333, 9615)
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
RUN_ID_RE = re.compile(
    r"^p4-(e0|e1|e2|e3|e4|e5|e6)-(b1|b2|g3)-"
    r"[0-9]{8}T[0-9]{6}\+0800-[0-9a-f]{8}-[0-9a-f]{8}$"
)
CATALOG_PATH = Path("configs/phase4/formal-scenarios-v1.json")
PREFLIGHT_SCHEMA_PATH = Path("schemas/phase4/run-preflight.schema.json")
SEAL_SCHEMA_PATH = Path("schemas/phase4/run-seal.schema.json")
RESULT_SCHEMA_PATH = Path("schemas/phase4/formal-result.schema.json")
BLOCK_EVIDENCE_SCHEMA_PATH = Path("schemas/phase4/attempt-block-evidence.schema.json")
SNAPSHOT_SCHEMA_PATH = Path("schemas/phase3/normalized-snapshot.schema.json")
CORRECTNESS_WORKLOAD = Path("configs/workloads/formal_correctness.json")
COST_WORKLOAD = Path("configs/workloads/formal_cost_v2.json")
DEV_CONFIG = Path("configs/dev.json")
PHASE3_GATE = Path("reports/planning/phase3-gate-review.md")

SETTLEMENT_RECORD_FIELDS = (
    "settlement_id",
    "order_id",
    "operation",
    "asset_id",
    "right_id",
    "authorization_id",
    "market_id",
    "policy_hash",
    "order_commitment_hash",
    "seller",
    "buyer",
    "currency",
    "gross_price",
    "seller_amount",
    "royalty_recipient",
    "royalty_amount",
    "market_fee_recipient",
    "market_fee_amount",
    "safety_fee_amount",
    "pre_state_version",
    "post_state_version",
    "pre_state_commitment",
    "block_number",
    "settlement_protocol_version",
)


def canonical_json_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("utf-8")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def relative_posix(path: Path, root: Path) -> str:
    try:
        value = path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError as error:
        raise Phase4ValidationError(f"path is outside expected root: {path}") from error
    if value in {"", "."} or value.startswith("../"):
        raise Phase4ValidationError(f"invalid relative artifact path: {value}")
    return value


def write_json_exclusive(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = canonical_json_bytes(value)
    try:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o444)
    except FileExistsError as error:
        raise Phase4ValidationError(f"refusing to overwrite immutable artifact: {path}") from error
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
    except Exception:
        try:
            path.unlink()
        except FileNotFoundError:
            pass
        raise


def write_derivation(path: Path, value: Any) -> None:
    """Create a deterministic derivative, accepting an identical existing file."""

    payload = canonical_json_bytes(value)
    if path.exists():
        if path.read_bytes() != payload:
            raise Phase4ValidationError(f"existing derivative differs: {path}")
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    write_json_exclusive(path, value)


def schema_validate(instance: Any, schema_path: Path, *, exp_root: Path = EXP_ROOT) -> None:
    try:
        validate_instance(instance, load_json(exp_root / schema_path))
    except ContractValidationError as error:
        raise Phase4ValidationError(str(error)) from error


def _exact_keys(value: Mapping[str, Any], expected: Iterable[str], label: str) -> None:
    actual = set(value)
    frozen = set(expected)
    if actual != frozen:
        raise Phase4ValidationError(
            f"{label} keys differ; missing={sorted(frozen - actual)}, extra={sorted(actual - frozen)}"
        )


def _scenario_ids_from_phase3(exp_root: Path) -> list[str]:
    conformance = load_json(exp_root / "configs/phase3/capability-conformance-v1.json")
    return [scenario for group in ("LEG", "NEG", "CLEAN", "DUP", "RACE") for scenario in conformance["scenarios"][group]]


def validate_catalog(catalog: Mapping[str, Any], *, exp_root: Path = EXP_ROOT) -> dict[str, Any]:
    required_top = {
        "schema_id", "schema_version", "catalog_id", "protocol_id",
        "settlement_protocol_version", "fixture_profile_id", "baseline_ids",
        "required_invariants", "deferred_invariants", "execution_policy", "scenarios",
    }
    _exact_keys(catalog, required_top, "scenario catalog")
    if catalog["schema_id"] != "assetxchain-exp2-formal-scenario-catalog" or catalog["schema_version"] != 1:
        raise Phase4ValidationError("scenario catalog schema identity drift")
    if catalog["catalog_id"] != "formal-scenarios-v1":
        raise Phase4ValidationError("scenario catalog version drift")
    if catalog["protocol_id"] != "assetxchain-paper2-shared" or catalog["settlement_protocol_version"] != 1:
        raise Phase4ValidationError("scenario protocol drift")
    if tuple(catalog["baseline_ids"]) != BASELINES:
        raise Phase4ValidationError("formal baseline order must be B1/B2/G3")
    if catalog["required_invariants"] != [f"I{index}" for index in range(1, 11)]:
        raise Phase4ValidationError("required invariant set must be I1-I10")
    if catalog["deferred_invariants"] != ["I11"]:
        raise Phase4ValidationError("I11 must remain deferred")

    scenarios = catalog["scenarios"]
    if not isinstance(scenarios, list) or len(scenarios) != 42:
        raise Phase4ValidationError("formal catalog must contain exactly 42 scenarios")
    ids = [item.get("scenario_id") for item in scenarios]
    expected_ids = _scenario_ids_from_phase3(exp_root)
    if ids != expected_ids:
        raise Phase4ValidationError("formal scenario order/identity differs from Phase 3 freeze")
    if len(ids) != len(set(ids)):
        raise Phase4ValidationError("scenario IDs are not unique")

    common_errors = set(load_json(exp_root / "configs/baselines/phase3-common.json")["common_error_codes"])
    category_counts: dict[str, int] = {}
    mode_counts = {"live-chain": 0, "test-gate": 0}
    attempt_count = 0
    for scenario in scenarios:
        minimum = {
            "scenario_id", "family", "category", "operation", "fixture",
            "execution_mode", "invariants", "metrics", "attempts",
        }
        optional = {"invalid_delta", "cardinality"}
        if not minimum.issubset(scenario) or set(scenario) - minimum - optional:
            raise Phase4ValidationError(f"{scenario.get('scenario_id')}: malformed scenario fields")
        scenario_id = scenario["scenario_id"]
        if scenario["family"] not in {"E1", "E2", "E3", "E4"}:
            raise Phase4ValidationError(f"{scenario_id}: invalid family")
        if scenario["operation"] not in {"TransferMeta", "IssueRight", "TransferRight"}:
            raise Phase4ValidationError(f"{scenario_id}: invalid operation")
        mode = scenario["execution_mode"]
        if mode not in mode_counts:
            raise Phase4ValidationError(f"{scenario_id}: invalid execution mode")
        mode_counts[mode] += 1
        category_counts[scenario["category"]] = category_counts.get(scenario["category"], 0) + 1
        if mode == "test-gate" and not scenario_id.startswith("CLEAN-ABORT-"):
            raise Phase4ValidationError(f"{scenario_id}: only CLEAN-ABORT may be a test gate")
        if scenario_id.startswith("CLEAN-ABORT-") and mode != "test-gate":
            raise Phase4ValidationError(f"{scenario_id}: production failpoint exposure is forbidden")
        if scenario["category"] == "illegal" and not isinstance(scenario.get("invalid_delta"), str):
            raise Phase4ValidationError(f"{scenario_id}: negative scenario lacks its single invalid delta")
        attempts = scenario["attempts"]
        if not attempts or len({item.get("attempt_id") for item in attempts}) != len(attempts):
            raise Phase4ValidationError(f"{scenario_id}: attempt IDs are missing or duplicate")
        attempt_count += len(attempts)
        for attempt in attempts:
            _exact_keys(attempt, {"attempt_id", "phase", "expected_outcome", "expected_error_code"}, f"{scenario_id} attempt")
            outcome = attempt["expected_outcome"]
            error = attempt["expected_error_code"]
            if outcome == "reject" and error not in common_errors:
                raise Phase4ValidationError(f"{scenario_id}: unfrozen error {error!r}")
            if outcome in {"accept", "test-pass"} and error is not None:
                raise Phase4ValidationError(f"{scenario_id}: successful expectation contains an error")
            if outcome not in {"accept", "reject", "winner-or-loser", "test-pass"}:
                raise Phase4ValidationError(f"{scenario_id}: invalid expected outcome")
        cardinality = scenario.get("cardinality")
        if any(item["expected_outcome"] == "winner-or-loser" for item in attempts):
            if not isinstance(cardinality, dict):
                raise Phase4ValidationError(f"{scenario_id}: race cardinality is missing")
            if cardinality["accepted"] + cardinality["rejected"] != len(attempts):
                raise Phase4ValidationError(f"{scenario_id}: race cardinality differs from attempts")
            if cardinality["rejected_error_code"] not in common_errors:
                raise Phase4ValidationError(f"{scenario_id}: race loser error is unfrozen")

    expected_categories = {
        "legal": 6,
        "illegal": 24,
        "failure_cleanliness_and_rollback": 5,
        "duplicate": 3,
        "competition": 4,
    }
    if category_counts != expected_categories:
        raise Phase4ValidationError(f"scenario category counts drift: {category_counts}")
    if mode_counts != {"live-chain": 39, "test-gate": 3}:
        raise Phase4ValidationError(f"scenario execution mode counts drift: {mode_counts}")
    policy = catalog["execution_policy"]
    if (policy["live_scenario_count"], policy["test_gate_scenario_count"], policy["total_scenario_count"]) != (39, 3, 42):
        raise Phase4ValidationError("catalog execution-policy counts drift")
    return {"status": "PASS", "scenarios": 42, "live": 39, "test_gates": 3, "attempts_per_repetition": attempt_count}


def load_catalog(*, exp_root: Path = EXP_ROOT) -> dict[str, Any]:
    catalog = load_json(exp_root / CATALOG_PATH)
    validate_catalog(catalog, exp_root=exp_root)
    return catalog


def scenario_map(catalog: Mapping[str, Any]) -> dict[str, Mapping[str, Any]]:
    return {item["scenario_id"]: item for item in catalog["scenarios"]}


def select_scenarios(catalog: Mapping[str, Any], family: str) -> list[Mapping[str, Any]]:
    if family not in FAMILIES:
        raise Phase4ValidationError(f"unsupported family: {family}")
    scenarios = catalog["scenarios"]
    if family == "E0":
        wanted = {"LEG-META-01", "LEG-ISSUE-01", "LEG-RESALE-01"}
        return [item for item in scenarios if item["scenario_id"] in wanted]
    if family == "E5":
        wanted = {"LEG-META-01", "LEG-ISSUE-01", "LEG-RESALE-01", "RACE-RELAYER-01", "RACE-QUOTA-01"}
        return [item for item in scenarios if item["scenario_id"] in wanted]
    if family == "E6":
        wanted = {"LEG-META-01", "LEG-ISSUE-01", "LEG-RESALE-01"}
        return [item for item in scenarios if item["scenario_id"] in wanted]
    return [item for item in scenarios if item["family"] == family]


def planned_execution(scenarios: Sequence[Mapping[str, Any]], repetitions: int, seed: int) -> list[dict[str, Any]]:
    if repetitions < 1:
        raise Phase4ValidationError("repetitions must be positive")
    rows = []
    for repetition in range(1, repetitions + 1):
        for scenario in scenarios:
            material = f"{seed}:{repetition}:{scenario['scenario_id']}".encode("ascii")
            rows.append({
                "scenario_id": scenario["scenario_id"],
                "repetition": repetition,
                "execution_mode": scenario["execution_mode"],
                "order_key": sha256_bytes(material),
            })
    rows.sort(key=lambda row: (row["order_key"], row["scenario_id"], row["repetition"]))
    return [{"sequence": index, **row} for index, row in enumerate(rows, 1)]


def validate_phase3_gate(exp_root: Path = EXP_ROOT) -> None:
    text = (exp_root / PHASE3_GATE).read_text(encoding="utf-8")
    if "PHASE_3_GATE = PASS" not in text or "READY_FOR_PHASE_4 = true" not in text:
        raise Phase4ValidationError("Phase 3 gate does not authorize Phase 4")


def validate_preflight(manifest: Mapping[str, Any], *, exp_root: Path = EXP_ROOT, check_files: bool = True) -> None:
    schema_validate(manifest, PREFLIGHT_SCHEMA_PATH, exp_root=exp_root)
    if RUN_ID_RE.fullmatch(manifest["run_id"]) is None:
        raise Phase4ValidationError("run ID does not satisfy the frozen format")
    baseline = manifest["baseline"]
    if baseline["baseline_id"].lower() not in manifest["run_id"]:
        raise Phase4ValidationError("run ID baseline token differs from manifest")
    if manifest["experiment_family"].lower() not in manifest["run_id"]:
        raise Phase4ValidationError("run ID family token differs from manifest")
    if baseline["dirty"] or not baseline["formal_base_is_ancestor"]:
        raise Phase4ValidationError("formal runs require a clean descendant of the formal base")
    if len(set(manifest["inputs"]["selected_scenario_ids"])) != len(manifest["inputs"]["selected_scenario_ids"]):
        raise Phase4ValidationError("selected scenario IDs are duplicated")
    if manifest["inputs"]["required_invariants"] != [f"I{i}" for i in range(1, 11)] or manifest["inputs"]["deferred_invariants"] != ["I11"]:
        raise Phase4ValidationError("manifest invariant boundary drift")
    if tuple(manifest["execution"]["ports"]) != PORTS:
        raise Phase4ValidationError("formal port set drift")
    if manifest["experiment_family"] == "E6" and not manifest["prerequisites"]["approved_s4_8_candidate_id"]:
        raise Phase4ValidationError("E6 requires an approved S4.8 candidate")
    if manifest["experiment_family"] != "E6" and manifest["prerequisites"]["approved_s4_8_candidate_id"] is not None:
        raise Phase4ValidationError("only E6 may reference an approved S4.8 candidate")
    workload_entry = manifest["inputs"]["workload"]
    workload = load_json(exp_root / workload_entry["path"])
    metadata = workload["metadata"]
    expected_repetitions = 2 if manifest["experiment_family"] == "E0" else metadata["repetitions"]
    if manifest["inputs"]["workload_id"] != metadata["workload_id"] or manifest["inputs"]["workload_class"] != metadata["workload_class"]:
        raise Phase4ValidationError("workload identity differs from frozen workload bytes")
    if manifest["inputs"]["seed"] != metadata["seed"] or manifest["inputs"]["repetitions"] != expected_repetitions:
        raise Phase4ValidationError("seed/repetition policy differs from the frozen workload")
    catalog = load_catalog(exp_root=exp_root)
    expected_scenarios = [item["scenario_id"] for item in select_scenarios(catalog, manifest["experiment_family"])]
    if manifest["inputs"]["selected_scenario_ids"] != expected_scenarios:
        raise Phase4ValidationError("selected scenarios differ from the frozen family selection")
    executable = manifest["execution"]["executor_command"][0]
    if not (Path(executable).is_file() or shutil.which(executable)):
        raise Phase4ValidationError(f"executor executable is not identifiable: {executable}")
    if "--dev" not in manifest["execution"]["node_command"] or "--tmp" not in manifest["execution"]["node_command"]:
        raise Phase4ValidationError("node command must use --dev --tmp")
    expected_paths = {
        "raw": exp_root / "results/raw" / manifest["run_id"],
        "log": exp_root / "logs" / manifest["run_id"],
        "processed": exp_root / "results/processed" / manifest["run_id"],
        "report": exp_root / "reports" / f"{manifest['run_id']}.md",
    }
    if {key: Path(value) for key, value in manifest["paths"].items()} != expected_paths:
        raise Phase4ValidationError("run artifact paths differ from the frozen layout")
    if check_files:
        for group in ("environment_config", "workload", "scenario_catalog", "baseline_manifest"):
            entry = manifest["inputs"][group]
            path = exp_root / entry["path"]
            if not path.is_file() or sha256_file(path) != entry["sha256"]:
                raise Phase4ValidationError(f"input hash drift: {entry['path']}")
        for tool in manifest["tools"]:
            path = exp_root / tool["path"]
            if not path.is_file() or sha256_file(path) != tool["sha256"]:
                raise Phase4ValidationError(f"tool hash drift: {tool['path']}")
        for artifact in manifest["execution"]["executor_artifacts"]:
            path = Path(artifact["path"])
            if not path.is_file() or sha256_file(path) != artifact["sha256"]:
                raise Phase4ValidationError(f"executor artifact hash drift: {path}")
    expected_units = planned_execution(
        [scenario_map(catalog)[item] for item in manifest["inputs"]["selected_scenario_ids"]],
        manifest["inputs"]["repetitions"],
        manifest["inputs"]["seed"],
    )
    if manifest["planned_execution"] != expected_units:
        raise Phase4ValidationError("planned execution order is not the frozen deterministic order")


def validate_seal(seal: Mapping[str, Any], preflight: Mapping[str, Any], *, exp_root: Path = EXP_ROOT) -> None:
    schema_validate(seal, SEAL_SCHEMA_PATH, exp_root=exp_root)
    if seal["run_id"] != preflight["run_id"]:
        raise Phase4ValidationError("seal/preflight run ID mismatch")
    if seal["status"] == "completed":
        if seal["failure_reason"] is not None or seal["checker"]["status"] != "PASS":
            raise Phase4ValidationError("completed seal contradicts failure/checker state")
        if seal["attempt_counts"]["planned_units"] != seal["attempt_counts"]["completed_units"]:
            raise Phase4ValidationError("completed seal has missing execution units")
        if seal["attempt_counts"]["planned_attempts"] != seal["attempt_counts"]["completed_attempts"]:
            raise Phase4ValidationError("completed seal has missing attempts")
        if not seal["teardown"]["owned_processes_stopped"] or seal["teardown"]["residual_listener_count"]:
            raise Phase4ValidationError("completed seal has an unsafe teardown state")
    elif not seal["failure_reason"]:
        raise Phase4ValidationError("invalid seal requires a failure reason")
    inventory = seal["artifact_inventory"]
    keys = [(item["root"], item["path"]) for item in inventory]
    if keys != sorted(keys) or len(keys) != len(set(keys)):
        raise Phase4ValidationError("artifact inventory must be unique and sorted")


def tool_paths() -> tuple[Path, ...]:
    return (
        Path("scripts/phase4/phase4_contract.py"),
        Path("scripts/phase4/phase4_manifest.py"),
        Path("scripts/phase4/phase4_checker.py"),
        Path("scripts/phase4/phase4_runner.py"),
        Path("scripts/phase4/process_formal.py"),
        Path("scripts/phase4/validate_phase4_readiness.py"),
        PREFLIGHT_SCHEMA_PATH,
        SEAL_SCHEMA_PATH,
        RESULT_SCHEMA_PATH,
        BLOCK_EVIDENCE_SCHEMA_PATH,
        SNAPSHOT_SCHEMA_PATH,
        CATALOG_PATH,
    )
