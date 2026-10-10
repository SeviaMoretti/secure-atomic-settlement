"""Shared Phase 3 adapter and normalized observation contract.

This module defines interfaces and validation shared by the independently
implemented B1, B2, and G3 adapters.
"""

from __future__ import annotations

import json
import re
import subprocess
from abc import ABC, abstractmethod
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Any, Mapping

from shared_fixture_codec import validate_bundle_semantics


CONTRACT_VERSION = "phase3-adapter-contract-v1"
BASELINE_NAMES = {
    "B1": "Contract-only",
    "B2": "Runtime-only",
    "G3": "AssetXChain-CRS",
}
FORMAL_BASE = "393bc52af495ccd72ff883059d7715cbb153f964"
PRESERVATION_COMMIT = "cf3859eeadfc7286a1f0656a148a9fa245f82605"
PHASE2_G3_COMMIT = "628803038743ac3b5bfa90e685ac38a1163d1482"
ARCHITECTURE_APPLICABLE_METRICS = {
    "B1": frozenset(
        {
            "contract_gas_or_ref_time",
            "contract_storage_writes",
        }
    ),
    "B2": frozenset(
        {
            "runtime_dispatch_count",
            "runtime_storage_writes",
        }
    ),
    "G3": frozenset(
        {
            "contract_gas_or_ref_time",
            "contract_storage_writes",
            "runtime_dispatch_count",
            "runtime_storage_writes",
            "typed_ce_call_count",
            "projection_storage_writes",
        }
    ),
}


class ContractValidationError(ValueError):
    """Raised when a manifest, invocation, snapshot, or result is invalid."""


class Phase(str, Enum):
    SETUP = "setup"
    CREATE_ORDER = "create_order"
    FUND_ORDER = "fund_order"
    SETTLE = "settle"
    CANCEL_OPEN = "cancel_open"
    EXPIRE_OPEN = "expire_open"
    REFUND_FUNDED = "refund_funded"
    QUERY = "query"


class Operation(str, Enum):
    TRANSFER_META = "TransferMeta"
    ISSUE_RIGHT = "IssueRight"
    TRANSFER_RIGHT = "TransferRight"


@dataclass(frozen=True)
class PhaseInvocation:
    run_id: str
    baseline_id: str
    scenario_id: str
    attempt_id: str
    repetition: int
    phase: Phase
    operation: Operation | None
    actor_role: str
    expected_outcome: str
    expected_error_code: str | None
    payload: Mapping[str, Any]

    @classmethod
    def from_mapping(cls, value: Mapping[str, Any]) -> "PhaseInvocation":
        return cls(
            run_id=value["run_id"],
            baseline_id=value["baseline_id"],
            scenario_id=value["scenario_id"],
            attempt_id=value["attempt_id"],
            repetition=value["repetition"],
            phase=Phase(value["phase"]),
            operation=(Operation(value["operation"]) if value["operation"] else None),
            actor_role=value["actor_role"],
            expected_outcome=value["expected_outcome"],
            expected_error_code=value["expected_error_code"],
            payload=value["payload"],
        )


class BaselineAdapter(ABC):
    """Common phase surface implemented independently by each baseline."""

    @property
    @abstractmethod
    def baseline_id(self) -> str:
        raise NotImplementedError

    def invoke(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        if invocation.baseline_id != self.baseline_id:
            raise ContractValidationError(
                f"invocation baseline {invocation.baseline_id} does not match {self.baseline_id}"
            )
        handlers = {
            Phase.SETUP: self.setup,
            Phase.CREATE_ORDER: self.create_order,
            Phase.FUND_ORDER: self.fund_order,
            Phase.SETTLE: self.settle,
            Phase.CANCEL_OPEN: self.cancel_open,
            Phase.EXPIRE_OPEN: self.expire_open,
            Phase.REFUND_FUNDED: self.refund_funded,
            Phase.QUERY: self.query,
        }
        return handlers[invocation.phase](invocation)

    @abstractmethod
    def setup(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        raise NotImplementedError

    @abstractmethod
    def create_order(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        raise NotImplementedError

    @abstractmethod
    def fund_order(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        raise NotImplementedError

    @abstractmethod
    def settle(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        raise NotImplementedError

    @abstractmethod
    def cancel_open(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        raise NotImplementedError

    @abstractmethod
    def expire_open(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        raise NotImplementedError

    @abstractmethod
    def refund_funded(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        raise NotImplementedError

    @abstractmethod
    def query(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        raise NotImplementedError


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def _resolve_ref(root_schema: Mapping[str, Any], reference: str) -> Mapping[str, Any]:
    if not reference.startswith("#/"):
        raise ContractValidationError(f"unsupported non-local schema reference: {reference}")
    current: Any = root_schema
    for part in reference[2:].split("/"):
        current = current[part.replace("~1", "/").replace("~0", "~")]
    if not isinstance(current, Mapping):
        raise ContractValidationError(f"schema reference is not an object: {reference}")
    return current


def _matches_type(instance: Any, expected: str) -> bool:
    if expected == "null":
        return instance is None
    if expected == "object":
        return isinstance(instance, dict)
    if expected == "array":
        return isinstance(instance, list)
    if expected == "string":
        return isinstance(instance, str)
    if expected == "integer":
        return isinstance(instance, int) and not isinstance(instance, bool)
    if expected == "number":
        return isinstance(instance, (int, float)) and not isinstance(instance, bool)
    if expected == "boolean":
        return isinstance(instance, bool)
    raise ContractValidationError(f"unsupported schema type: {expected}")


def validate_instance(
    instance: Any,
    schema: Mapping[str, Any],
    *,
    path: str = "$",
    root_schema: Mapping[str, Any] | None = None,
) -> None:
    """Validate the JSON Schema subset used by the frozen Phase 3 schemas."""

    root = root_schema or schema
    if "$ref" in schema:
        validate_instance(
            instance,
            _resolve_ref(root, schema["$ref"]),
            path=path,
            root_schema=root,
        )
        return

    if "const" in schema and instance != schema["const"]:
        raise ContractValidationError(f"{path}: expected constant {schema['const']!r}")
    if "enum" in schema and instance not in schema["enum"]:
        raise ContractValidationError(f"{path}: {instance!r} is not in the allowed enum")

    declared_type = schema.get("type")
    if declared_type is not None:
        accepted = declared_type if isinstance(declared_type, list) else [declared_type]
        if not any(_matches_type(instance, item) for item in accepted):
            raise ContractValidationError(f"{path}: expected type {accepted}, got {type(instance).__name__}")

    if isinstance(instance, dict):
        required = schema.get("required", [])
        missing = [key for key in required if key not in instance]
        if missing:
            raise ContractValidationError(f"{path}: missing required fields {missing}")
        properties = schema.get("properties", {})
        if schema.get("additionalProperties") is False:
            extras = sorted(set(instance) - set(properties))
            if extras:
                raise ContractValidationError(f"{path}: unexpected fields {extras}")
        for key, child_schema in properties.items():
            if key in instance:
                validate_instance(
                    instance[key],
                    child_schema,
                    path=f"{path}.{key}",
                    root_schema=root,
                )

    if isinstance(instance, list):
        if len(instance) < schema.get("minItems", 0):
            raise ContractValidationError(f"{path}: expected at least {schema['minItems']} items")
        item_schema = schema.get("items")
        if item_schema:
            for index, item in enumerate(instance):
                validate_instance(item, item_schema, path=f"{path}[{index}]", root_schema=root)

    if isinstance(instance, str):
        if len(instance) < schema.get("minLength", 0):
            raise ContractValidationError(f"{path}: string is shorter than required")
        pattern = schema.get("pattern")
        if pattern and re.search(pattern, instance) is None:
            raise ContractValidationError(f"{path}: value does not match {pattern}")

    if isinstance(instance, (int, float)) and not isinstance(instance, bool):
        minimum = schema.get("minimum")
        if minimum is not None and instance < minimum:
            raise ContractValidationError(f"{path}: value is below {minimum}")


def validate_invocation_semantics(invocation: Mapping[str, Any], common: Mapping[str, Any]) -> None:
    business_phases = set(common["phase_contract"]["allowed_phases"]) - {"setup", "query"}
    if invocation["phase"] in business_phases and invocation["operation"] is None:
        raise ContractValidationError("business phases require an operation")
    if invocation["phase"] == "setup" and invocation["operation"] is not None:
        raise ContractValidationError("setup must not declare an operation")
    if invocation["expected_outcome"] == "accept" and invocation["expected_error_code"] is not None:
        raise ContractValidationError("accepted invocation must not expect an error")
    if invocation["expected_outcome"] == "reject":
        code = invocation["expected_error_code"]
        if code not in common["common_error_codes"]:
            raise ContractValidationError("rejected invocation requires a frozen common error")


def _validate_metric(metric: Mapping[str, Any], path: str) -> None:
    availability = metric["availability"]
    value = metric["value"]
    if availability in {"N/A", "not-collected"} and value is not None:
        raise ContractValidationError(f"{path}: {availability} metric must have null value")
    if availability == "measured" and value is None:
        raise ContractValidationError(f"{path}: measured metric requires a value")
    if availability == "N/A" and value == "0":
        raise ContractValidationError(f"{path}: N/A must never be encoded as zero")


def validate_result_semantics(
    result: Mapping[str, Any],
    common: Mapping[str, Any],
    snapshot_schema: Mapping[str, Any],
) -> None:
    phase = result["request"]["phase"]
    pre = result["snapshots"]["pre"]
    post = result["snapshots"]["post"]
    if phase == "setup" and pre is not None:
        raise ContractValidationError("setup result must use a null pre snapshot")
    if phase != "setup" and pre is None:
        raise ContractValidationError("non-setup result requires a pre snapshot")
    if pre is not None:
        validate_instance(pre, snapshot_schema, path="$.snapshots.pre")
    validate_instance(post, snapshot_schema, path="$.snapshots.post")

    baseline_id = result["run"]["baseline_id"]
    if result["run"]["baseline_name"] != BASELINE_NAMES[baseline_id]:
        raise ContractValidationError("result baseline name does not match baseline id")
    allowed_validity = common["implementation_boundary"]["allowed_validity_level"]
    if result["run"]["validity_level"] != allowed_validity:
        raise ContractValidationError(
            f"Phase 3 result validity must be {allowed_validity}"
        )
    expected_authority = "contract" if baseline_id == "B1" else "runtime"
    for label, snapshot in (("pre", pre), ("post", post)):
        if snapshot is None:
            continue
        if snapshot["baseline_id"] != baseline_id:
            raise ContractValidationError(f"{label} snapshot baseline mismatch")
        if snapshot["authority_source"] != expected_authority:
            raise ContractValidationError(f"{label} snapshot reads the wrong authority")
        projection = snapshot["projection"]
        if projection["authoritative"] is not False:
            raise ContractValidationError(
                f"{label} contract projection cannot claim normalized authority"
            )
        projected_values = (
            projection["order_state"],
            projection["settlement_record_count"],
        )
        if baseline_id in {"B1", "B2"}:
            if projection["availability"] != "N/A" or any(
                value is not None for value in projected_values
            ):
                raise ContractValidationError(
                    f"{label} {baseline_id} projection fields must be structural N/A"
                )
        elif projection["availability"] == "N/A":
            raise ContractValidationError(
                f"{label} G3 projection is applicable and cannot be N/A"
            )
        elif projection["availability"] == "unavailable" and any(
            value is not None for value in projected_values
        ):
            raise ContractValidationError(
                f"{label} unavailable G3 projection must contain null values"
            )

    expected = result["request"]["expected_outcome"]
    expected_error = result["request"]["expected_error_code"]
    actual = result["outcome"]["protocol_outcome"]
    code = result["outcome"]["common_error_code"]
    dispatch_success = result["outcome"]["dispatch_success"]
    if code is not None and code not in common["common_error_codes"]:
        raise ContractValidationError("result uses an unfrozen common error")
    if actual == "accepted" and code is not None:
        raise ContractValidationError("accepted result must not contain a common error")
    if actual == "rejected" and code is None:
        raise ContractValidationError("rejected result requires a common error")
    if dispatch_success != (actual == "accepted"):
        raise ContractValidationError("dispatch success contradicts protocol outcome")
    if actual == "timeout" and result["correctness"]["timeout_count"] == 0:
        raise ContractValidationError("timeout outcome must increment timeout_count")
    if expected == "reject" and actual == "timeout":
        raise ContractValidationError("timeout cannot satisfy an expected rejection")

    expected_match = (expected == "accept" and actual == "accepted") or (
        expected == "reject" and actual == "rejected" and code == expected_error
    )
    correctness = result["correctness"]
    if correctness["expected_outcome_match"] != expected_match:
        raise ContractValidationError("expected outcome/error match is inconsistent")
    if correctness["false_accept_count"] != int(
        expected == "reject" and actual == "accepted"
    ):
        raise ContractValidationError("false_accept_count is inconsistent")
    if correctness["false_reject_count"] != int(
        expected == "accept" and actual == "rejected"
    ):
        raise ContractValidationError("false_reject_count is inconsistent")
    if actual != "timeout" and correctness["timeout_count"] != 0:
        raise ContractValidationError("non-timeout result has a timeout count")

    included = result["outcome"]["included_block"]
    completed = result["outcome"]["completion_block"]
    completion_blocks = result["outcome"]["completion_blocks"]
    if (included is None) != (completed is None):
        raise ContractValidationError("inclusion and completion blocks must be paired")
    expected_completion = None if included is None else completed - included
    if expected_completion is not None and expected_completion < 0:
        raise ContractValidationError("completion block precedes inclusion block")
    if completion_blocks != expected_completion:
        raise ContractValidationError("completion block delta is inconsistent")

    evidence = post["evidence"]
    settlement = result["settlement"]
    if settlement["settlement_id"] != evidence["settlement_id"]:
        raise ContractValidationError("settlement ID differs from authoritative snapshot")
    if settlement["settlement_record_count"] != evidence["settlement_record_count"]:
        raise ContractValidationError(
            "settlement record count differs from authoritative snapshot"
        )

    for name, metric in result["cost"].items():
        _validate_metric(metric, f"cost.{name}")
        if metric["availability"] == "N/A":
            raise ContractValidationError(f"common cost metric {name} cannot be N/A")
    applicable = ARCHITECTURE_APPLICABLE_METRICS[baseline_id]
    for name, metric in result["architecture_cost"].items():
        _validate_metric(metric, f"architecture_cost.{name}")
        if name in applicable and metric["availability"] == "N/A":
            raise ContractValidationError(
                f"applicable architecture metric {name} cannot be N/A"
            )
        if name not in applicable and metric["availability"] != "N/A":
            raise ContractValidationError(
                f"inapplicable architecture metric {name} must be N/A"
            )


def _git(path: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", "-C", str(path), *args],
        check=check,
        capture_output=True,
        text=True,
    )


def _path_is_owned(changed_path: str, owned_paths: tuple[str, ...]) -> bool:
    """Return whether a dirty path is an owned file or directory descendant."""

    return any(
        changed_path == owned_path or changed_path.startswith(owned_path + "/")
        for owned_path in owned_paths
    )


def validate_manifest_semantics(
    manifest: Mapping[str, Any],
    *,
    check_git: bool,
) -> None:
    baseline_id = manifest["baseline_id"]
    if manifest["baseline_name"] != BASELINE_NAMES[baseline_id]:
        raise ContractValidationError(f"{baseline_id}: baseline name mismatch")

    expected_authority = {
        "B1": {
            "domain_authority": "contract",
            "market_logic": "contract",
            "payment_authority": "contract-order-escrow-ledger",
            "order_authority": "contract",
            "settlement_evidence_authority": "contract",
            "projection_role": "not-applicable",
            "normalized_authority_source": "contract",
        },
        "B2": {
            "domain_authority": "runtime",
            "market_logic": "runtime",
            "payment_authority": "runtime-escrow",
            "order_authority": "runtime",
            "settlement_evidence_authority": "runtime",
            "projection_role": "not-applicable",
            "normalized_authority_source": "runtime",
        },
        "G3": {
            "domain_authority": "runtime",
            "market_logic": "contract",
            "payment_authority": "runtime-escrow",
            "order_authority": "runtime",
            "settlement_evidence_authority": "runtime",
            "projection_role": "recoverable-contract-projection",
            "normalized_authority_source": "runtime",
        },
    }[baseline_id]
    if manifest["authority"] != expected_authority:
        raise ContractValidationError(f"{baseline_id}: authority map drift")

    adapter = manifest["adapter"]
    expected_adapter = {
        "B1": ("implemented", "scripts/baselines/b1_adapter.py#B1Adapter", "S3.5"),
        "B2": ("implemented", "scripts/baselines/b2_adapter.py#B2Adapter", "S3.8"),
        "G3": ("implemented", "scripts/baselines/g3_adapter.py#G3Adapter", "S3.9"),
    }[baseline_id]
    if (
        adapter["implementation_status"],
        adapter["entrypoint"],
        adapter["planned_step"],
    ) != expected_adapter:
        raise ContractValidationError(f"{baseline_id}: adapter implementation state drift")
    if adapter["entrypoint"] is not None:
        entrypoint_path = adapter["entrypoint"].split("#", 1)[0]
        exp_root = Path(__file__).resolve().parents[2]
        if not (exp_root / entrypoint_path).is_file():
            raise ContractValidationError(f"{baseline_id}: adapter entrypoint is missing")

    worktree = Path(manifest["git"]["worktree"])
    if not worktree.is_dir():
        raise ContractValidationError(f"{baseline_id}: worktree does not exist: {worktree}")
    if Path(manifest["build"]["source_root"]) != worktree:
        raise ContractValidationError(f"{baseline_id}: build root differs from worktree")
    for entry in manifest["build"]["entries"]:
        if not Path(entry["cwd"]).is_dir():
            raise ContractValidationError(f"{baseline_id}: build cwd missing: {entry['cwd']}")
    for item in manifest["ownership"]:
        if not (worktree / item["path"]).exists():
            raise ContractValidationError(f"{baseline_id}: owned path missing: {item['path']}")

    git_info = manifest["git"]
    if baseline_id in {"B1", "B2"}:
        if git_info["source_preservation_commit"] != PRESERVATION_COMMIT:
            raise ContractValidationError(f"{baseline_id}: preservation identity missing")
        if git_info["phase2_g3_commit"] is not None:
            raise ContractValidationError(f"{baseline_id}: G3 commit must not be mixed in")
    else:
        if git_info["phase2_g3_commit"] != PHASE2_G3_COMMIT:
            raise ContractValidationError("G3 must retain the Phase 2 freeze identity")
        if git_info["source_preservation_commit"] is not None:
            raise ContractValidationError("G3 must not inherit the preservation commit")

    if not check_git:
        return
    actual_head = _git(worktree, "rev-parse", "HEAD").stdout.strip()
    actual_branch = _git(worktree, "branch", "--show-current").stdout.strip()
    actual_status = _git(worktree, "status", "--porcelain=v1").stdout.rstrip()
    if actual_head != git_info["head"]:
        raise ContractValidationError(f"{baseline_id}: HEAD differs from manifest")
    if actual_branch != git_info["branch"]:
        raise ContractValidationError(f"{baseline_id}: branch differs from manifest")
    dirty_expected = git_info["dirty_expected"]
    if bool(actual_status) != dirty_expected:
        expected = "dirty" if dirty_expected else "clean"
        raise ContractValidationError(f"{baseline_id}: worktree is not {expected} as declared")
    if actual_status:
        owned_paths = tuple(item["path"].rstrip("/") for item in manifest["ownership"])
        for line in actual_status.splitlines():
            changed_path = line[3:].split(" -> ")[-1]
            if not _path_is_owned(changed_path, owned_paths):
                raise ContractValidationError(
                    f"{baseline_id}: dirty path is outside declared ownership: {changed_path}"
                )
    if _git(worktree, "merge-base", "--is-ancestor", FORMAL_BASE, actual_head, check=False).returncode != 0:
        raise ContractValidationError(f"{baseline_id}: formal baseline is not an ancestor")
    if baseline_id == "G3" and _git(
        worktree,
        "merge-base",
        "--is-ancestor",
        PHASE2_G3_COMMIT,
        actual_head,
        check=False,
    ).returncode != 0:
        raise ContractValidationError("G3 does not descend from the Phase 2 freeze")
    preservation_is_ancestor = _git(
        worktree,
        "merge-base",
        "--is-ancestor",
        PRESERVATION_COMMIT,
        actual_head,
        check=False,
    ).returncode == 0
    if baseline_id == "G3" and preservation_is_ancestor:
        raise ContractValidationError("G3 contains the preservation commit")
    if baseline_id in {"B1", "B2"} and not preservation_is_ancestor:
        raise ContractValidationError(f"{baseline_id}: preservation commit is not an ancestor")


def validate_contract_bundle(exp_root: Path, *, check_git: bool = True) -> dict[str, Any]:
    common = load_json(exp_root / "configs/baselines/phase3-common.json")
    boundary = common["implementation_boundary"]
    if boundary.get("unified_driver_implemented") is not True:
        raise ContractValidationError("Phase 3 unified driver is not declared implemented")
    driver_entrypoint = boundary.get("unified_driver_entrypoint")
    if driver_entrypoint != "scripts/common/phase3_driver.py#Phase3ScenarioDriver":
        raise ContractValidationError("Phase 3 unified driver entrypoint drift")
    if not (exp_root / driver_entrypoint.split("#", 1)[0]).is_file():
        raise ContractValidationError("Phase 3 unified driver entrypoint is missing")
    schema_paths = {
        name: exp_root / relative for name, relative in common["schemas"].items()
    }
    schemas = {name: load_json(path) for name, path in schema_paths.items()}
    for name, schema in schemas.items():
        if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
            raise ContractValidationError(f"{name}: unsupported schema declaration")

    manifests = []
    for baseline_id in common["baseline_order"]:
        path = exp_root / f"configs/baselines/{baseline_id.lower()}.json"
        manifest = load_json(path)
        validate_instance(manifest, schemas["baseline_manifest"])
        validate_manifest_semantics(manifest, check_git=check_git)
        manifests.append(manifest)
    if [item["baseline_id"] for item in manifests] != ["B1", "B2", "G3"]:
        raise ContractValidationError("baseline order drift")

    fixture_contract = common["fixture_contract"]
    expected_consumer = {
        "fixture_profile_id": fixture_contract["fixture_profile_id"],
        "vector_bundle_id": fixture_contract["vector_bundle_id"],
        "vector_bundle": fixture_contract["vector_bundle"],
        "generator": fixture_contract["generator"],
        "consumer": fixture_contract["consumer"],
    }
    for manifest in manifests:
        if manifest["shared_fixture"] != expected_consumer:
            raise ContractValidationError(
                f"{manifest['baseline_id']}: shared fixture consumer drift"
            )

    errors = common["common_error_codes"]
    if len(errors) != 33 or len(set(errors)) != 33:
        raise ContractValidationError("common error vocabulary must contain 33 unique values")
    if set(common["phase_contract"]["allowed_phases"]) != {item.value for item in Phase}:
        raise ContractValidationError("phase invocation vocabulary drift")
    if set(common["phase_contract"]["allowed_operations"]) != {item.value for item in Operation}:
        raise ContractValidationError("operation vocabulary drift")

    dev = load_json(exp_root / "configs/dev.json")
    protocol = dev["protocol"]
    for key, value in common["protocol"].items():
        if protocol[key] != value:
            raise ContractValidationError(f"protocol field drift: {key}")
    fixture = dev["shared_fixture"]
    frozen = common["fixture_contract"]
    if fixture["fixture_profile_id"] != frozen["fixture_profile_id"]:
        raise ContractValidationError("fixture profile drift")
    if fixture["funding"]["unit_plancks"] != frozen["unit_plancks"]:
        raise ContractValidationError("UNIT drift")
    if fixture["transactions"]["quantity"] != frozen["quantity"]:
        raise ContractValidationError("quantity drift")
    price_fields = {
        "TransferMeta": "meta_sale_price_plancks",
        "IssueRight": "right_issue_price_plancks",
        "TransferRight": "right_resale_price_plancks",
    }
    for operation, field in price_fields.items():
        if fixture["transactions"][field] != frozen["prices_plancks"][operation]:
            raise ContractValidationError(f"{operation} price drift")
    timing_fields = {
        "order_expiry": "order_expiry_offset_blocks",
        "funded_grace": "funded_grace_offset_blocks",
        "right_validity": "right_validity_offset_blocks",
    }
    for name, field in timing_fields.items():
        if fixture["timing"][field] != frozen["timing_offsets_blocks"][name]:
            raise ContractValidationError(f"{name} timing drift")
    if dev["execution"]["common_seed"] != frozen["seed"]:
        raise ContractValidationError("seed drift")

    vector_bundle = load_json(exp_root / frozen["vector_bundle"])
    validate_instance(vector_bundle, schemas["shared_fixture_vectors"])
    validate_bundle_semantics(vector_bundle, exp_root)
    if list(vector_bundle["baseline_consumers"]) != common["baseline_order"]:
        raise ContractValidationError("vector consumer order drift")
    if [item["operation"] for item in vector_bundle["vectors"]] != common["phase_contract"]["allowed_operations"]:
        raise ContractValidationError("fixture vector operation order drift")

    return {
        "status": "PASS",
        "contract_version": common["contract_version"],
        "baselines": [item["baseline_id"] for item in manifests],
        "schemas": sorted(schemas),
        "phases": len(common["phase_contract"]["allowed_phases"]),
        "common_errors": len(errors),
        "fixture_vectors": len(vector_bundle["vectors"]),
        "fixture_consumers": len(vector_bundle["baseline_consumers"]),
        "shared_input_hash": vector_bundle["baseline_consumers"]["B1"]["shared_input_hash"],
        "git_checked": check_git,
        "validity_level": common["implementation_boundary"]["allowed_validity_level"],
    }
