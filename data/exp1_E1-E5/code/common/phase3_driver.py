"""Shared Phase 3 scenario driver for B1, B2, and G3.

The driver owns orchestration and normalized result assembly only. Baseline
adapters retain their architecture-specific call shapes and authority reads.
"""

from __future__ import annotations

import copy
import re
from dataclasses import dataclass, field
from pathlib import Path
from types import MappingProxyType
from typing import Any, Mapping, Sequence

from phase3_contract import (
    ARCHITECTURE_APPLICABLE_METRICS,
    BASELINE_NAMES,
    CONTRACT_VERSION,
    BaselineAdapter,
    ContractValidationError,
    Operation,
    Phase,
    PhaseInvocation,
    load_json,
    validate_instance,
    validate_invocation_semantics,
    validate_result_semantics,
)


BASELINE_IDS = tuple(BASELINE_NAMES)
COMMON_COST_UNITS = {
    "latency_ms": "ms",
    "completion_blocks": "blocks",
    "partial_fee": "plancks",
    "weight_ref_time": "ref_time",
    "weight_proof_size": "bytes",
    "storage_writes": "count",
    "events_count": "count",
    "rpc_response_ms": "ms",
    "phase_call_count": "count",
}
ARCHITECTURE_COST_UNITS = {
    "contract_gas_or_ref_time": "ref_time",
    "contract_storage_writes": "count",
    "runtime_dispatch_count": "count",
    "runtime_storage_writes": "count",
    "typed_ce_call_count": "count",
    "projection_storage_writes": "count",
}
_DECIMAL_RE = re.compile(r"^[0-9]+(?:\.[0-9]+)?$")
_HEX32_RE = re.compile(r"^0x[0-9a-f]{64}$")


def _nonnegative(value: int, label: str) -> None:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ContractValidationError(f"{label} must be a nonnegative integer")


def _exact_keys(value: Mapping[str, Any], expected: Sequence[str], label: str) -> None:
    actual = set(value)
    required = set(expected)
    if actual != required:
        missing = sorted(required - actual)
        extra = sorted(actual - required)
        raise ContractValidationError(
            f"{label} keys differ; missing={missing}, extra={extra}"
        )


def _deep_freeze(value: Any) -> Any:
    if isinstance(value, Mapping):
        return MappingProxyType(
            {key: _deep_freeze(item) for key, item in copy.deepcopy(dict(value)).items()}
        )
    if isinstance(value, list):
        return tuple(_deep_freeze(item) for item in value)
    if isinstance(value, tuple):
        return tuple(_deep_freeze(item) for item in value)
    return value


def _deep_thaw(value: Any) -> Any:
    if isinstance(value, Mapping):
        return {key: _deep_thaw(item) for key, item in value.items()}
    if isinstance(value, tuple):
        return [_deep_thaw(item) for item in value]
    return copy.deepcopy(value)


@dataclass(frozen=True)
class MetricObservation:
    """One explicit metric observation; unavailable values are always null."""

    availability: str
    value: str | None
    unit: str

    def __post_init__(self) -> None:
        if self.availability not in {"measured", "not-collected", "N/A"}:
            raise ContractValidationError("unsupported metric availability")
        if not isinstance(self.unit, str) or not self.unit:
            raise ContractValidationError("metric unit must be a nonempty string")
        if self.availability == "measured":
            if not isinstance(self.value, str) or _DECIMAL_RE.fullmatch(self.value) is None:
                raise ContractValidationError(
                    "measured metric value must be a nonnegative decimal string"
                )
            if self.unit != "ms" and not self.value.isdigit():
                raise ContractValidationError(
                    f"measured {self.unit} metric must be an integer string"
                )
        elif self.value is not None:
            raise ContractValidationError(
                f"{self.availability} metric must have a null value"
            )

    def to_mapping(self) -> dict[str, Any]:
        return {
            "availability": self.availability,
            "value": self.value,
            "unit": self.unit,
        }


@dataclass(frozen=True)
class CorrectnessObservation:
    """Non-derivable correctness counts supplied by an invariant checker."""

    invariant_violation_count: int
    duplicate_settlement_count: int
    dirty_order_count: int
    dirty_authorization_count: int
    event_mismatch_count: int
    critical_state_diff_count: int
    unexpected_balance_delta_count: int
    unexpected_index_delta_count: int

    def __post_init__(self) -> None:
        for name, value in self.__dict__.items():
            _nonnegative(value, f"correctness.{name}")

    def to_mapping(self) -> dict[str, int]:
        return dict(self.__dict__)


@dataclass(frozen=True)
class AttemptObservation:
    """Explicit observations not owned by a baseline adapter response."""

    correctness: CorrectnessObservation
    included_block: int | None = None
    completion_block: int | None = None
    success_event_count: int = 0
    post_state_commitment: str | None = None
    refund_recipient: str | None = None
    refund_amount: str | None = None
    common_cost: Mapping[str, MetricObservation] = field(default_factory=dict)
    architecture_cost: Mapping[str, MetricObservation] = field(default_factory=dict)

    def __post_init__(self) -> None:
        for name in ("included_block", "completion_block"):
            value = getattr(self, name)
            if value is not None:
                _nonnegative(value, name)
        _nonnegative(self.success_event_count, "success_event_count")
        if self.post_state_commitment is not None and _HEX32_RE.fullmatch(
            self.post_state_commitment
        ) is None:
            raise ContractValidationError("post_state_commitment must be a hex32 value")
        if self.refund_recipient is not None and _HEX32_RE.fullmatch(
            self.refund_recipient
        ) is None:
            raise ContractValidationError("refund_recipient must be a hex32 value")
        if self.refund_amount is not None and (
            not isinstance(self.refund_amount, str) or not self.refund_amount.isdigit()
        ):
            raise ContractValidationError("refund_amount must be an integer string")
        for label, metrics in (
            ("common_cost", self.common_cost),
            ("architecture_cost", self.architecture_cost),
        ):
            if not isinstance(metrics, Mapping):
                raise ContractValidationError(f"{label} must be a mapping")
            for name, metric in metrics.items():
                if not isinstance(metric, MetricObservation):
                    raise ContractValidationError(
                        f"{label}.{name} must be a MetricObservation"
                    )
        object.__setattr__(self, "common_cost", MappingProxyType(dict(self.common_cost)))
        object.__setattr__(
            self, "architecture_cost", MappingProxyType(dict(self.architecture_cost))
        )


@dataclass(frozen=True)
class AttemptDefinition:
    """One logical attempt with explicit native payloads for all baselines."""

    attempt_id: str
    phase: Phase
    operation: Operation | None
    actor_role: str
    expected_outcome: str
    expected_error_code: str | None
    payload_by_baseline: Mapping[str, Mapping[str, Any]]

    def __post_init__(self) -> None:
        if not self.attempt_id:
            raise ContractValidationError("attempt_id must not be empty")
        _exact_keys(self.payload_by_baseline, BASELINE_IDS, "payload_by_baseline")
        payloads: dict[str, Mapping[str, Any]] = {}
        for baseline_id, payload in self.payload_by_baseline.items():
            if not isinstance(payload, Mapping):
                raise ContractValidationError(
                    f"{baseline_id} attempt payload must be an object"
                )
            payloads[baseline_id] = _deep_freeze(payload)
        object.__setattr__(self, "payload_by_baseline", MappingProxyType(payloads))


@dataclass(frozen=True)
class ScenarioDefinition:
    """Ordered logical attempt sequence shared by B1, B2, and G3."""

    scenario_id: str
    attempts: tuple[AttemptDefinition, ...]

    def __post_init__(self) -> None:
        if not self.attempts:
            raise ContractValidationError("scenario must contain at least one attempt")
        ids = [attempt.attempt_id for attempt in self.attempts]
        if len(ids) != len(set(ids)):
            raise ContractValidationError("scenario attempt IDs must be unique")
        object.__setattr__(self, "attempts", tuple(self.attempts))


@dataclass(frozen=True)
class RunContext:
    """Immutable metadata for one baseline's in-memory driver execution."""

    run_id: str
    timestamp: str
    baseline_id: str
    implementation_commit: str
    git_dirty: bool
    config_path: str
    workload_id: str
    seed: int
    repetition: int
    claim_boundary: str
    timezone: str = "Asia/Shanghai"
    baseline_profile_version: str = "1"
    settlement_protocol_version: int = 1
    validity_level: str = "legacy-pilot"

    def __post_init__(self) -> None:
        if self.baseline_id not in BASELINE_NAMES:
            raise ContractValidationError("unknown baseline in run context")
        for label in ("run_id", "timestamp", "config_path", "workload_id", "claim_boundary"):
            if not isinstance(getattr(self, label), str) or not getattr(self, label):
                raise ContractValidationError(f"{label} must be a nonempty string")
        if re.fullmatch(r"[0-9a-f]{40}", self.implementation_commit) is None:
            raise ContractValidationError("implementation_commit must be a Git SHA-1")
        if not isinstance(self.git_dirty, bool):
            raise ContractValidationError("git_dirty must be boolean")
        _nonnegative(self.seed, "seed")
        _nonnegative(self.repetition, "repetition")

    def to_mapping(self) -> dict[str, Any]:
        return {
            "run_id": self.run_id,
            "timestamp": self.timestamp,
            "timezone": self.timezone,
            "baseline_id": self.baseline_id,
            "baseline_name": BASELINE_NAMES[self.baseline_id],
            "baseline_profile_version": self.baseline_profile_version,
            "settlement_protocol_version": self.settlement_protocol_version,
            "validity_level": self.validity_level,
            "implementation_commit": self.implementation_commit,
            "git_dirty": self.git_dirty,
            "config_path": self.config_path,
            "workload_id": self.workload_id,
            "seed": self.seed,
            "repetition": self.repetition,
        }


class Phase3ScenarioDriver:
    """Validate, dispatch, and normalize one shared scenario definition."""

    def __init__(self, exp_root: Path | None = None) -> None:
        self.exp_root = exp_root or Path(__file__).resolve().parents[2]
        self.common = load_json(self.exp_root / "configs/baselines/phase3-common.json")
        self.invocation_schema = load_json(
            self.exp_root / self.common["schemas"]["phase_invocation"]
        )
        self.snapshot_schema = load_json(
            self.exp_root / self.common["schemas"]["normalized_snapshot"]
        )
        self.result_schema = load_json(
            self.exp_root / self.common["schemas"]["normalized_result"]
        )
        self.manifests = {
            baseline_id: load_json(
                self.exp_root / f"configs/baselines/{baseline_id.lower()}.json"
            )
            for baseline_id in self.common["baseline_order"]
        }

    def materialize(
        self, scenario: ScenarioDefinition, context: RunContext
    ) -> tuple[PhaseInvocation, ...]:
        self._validate_context(context)
        invocations = []
        for attempt in scenario.attempts:
            mapping = {
                "schema_id": "assetxchain-exp2-phase-invocation",
                "schema_version": 1,
                "contract_version": CONTRACT_VERSION,
                "run_id": context.run_id,
                "baseline_id": context.baseline_id,
                "scenario_id": scenario.scenario_id,
                "attempt_id": attempt.attempt_id,
                "repetition": context.repetition,
                "phase": attempt.phase.value,
                "operation": attempt.operation.value if attempt.operation else None,
                "actor_role": attempt.actor_role,
                "expected_outcome": attempt.expected_outcome,
                "expected_error_code": attempt.expected_error_code,
                "payload": _deep_thaw(
                    attempt.payload_by_baseline[context.baseline_id]
                ),
            }
            validate_instance(mapping, self.invocation_schema)
            validate_invocation_semantics(mapping, self.common)
            invocations.append(PhaseInvocation.from_mapping(mapping))
        return tuple(invocations)

    def run_scenario(
        self,
        adapter: BaselineAdapter,
        scenario: ScenarioDefinition,
        context: RunContext,
        observations: Mapping[str, AttemptObservation],
    ) -> tuple[dict[str, Any], ...]:
        if adapter.baseline_id != context.baseline_id:
            raise ContractValidationError("adapter and run-context baselines differ")
        _exact_keys(
            observations,
            [attempt.attempt_id for attempt in scenario.attempts],
            "attempt observations",
        )
        results = []
        for invocation in self.materialize(scenario, context):
            response = adapter.invoke(invocation)
            result = self._assemble_result(
                invocation,
                context,
                response,
                observations[invocation.attempt_id],
            )
            validate_instance(result, self.result_schema)
            validate_result_semantics(result, self.common, self.snapshot_schema)
            results.append(result)
        return tuple(results)

    def run_matrix(
        self,
        scenario: ScenarioDefinition,
        adapters: Mapping[str, BaselineAdapter],
        contexts: Mapping[str, RunContext],
        observations: Mapping[str, Mapping[str, AttemptObservation]],
    ) -> dict[str, tuple[dict[str, Any], ...]]:
        order = self.common["baseline_order"]
        _exact_keys(adapters, order, "adapters")
        _exact_keys(contexts, order, "run contexts")
        _exact_keys(observations, order, "baseline observations")
        shared_fields = {
            (
                context.config_path,
                context.workload_id,
                context.seed,
                context.repetition,
                context.validity_level,
            )
            for context in contexts.values()
        }
        if len(shared_fields) != 1:
            raise ContractValidationError(
                "B1/B2/G3 must share config, workload, seed, repetition, and validity"
            )
        return {
            baseline_id: self.run_scenario(
                adapters[baseline_id],
                scenario,
                contexts[baseline_id],
                observations[baseline_id],
            )
            for baseline_id in order
        }

    def _validate_context(self, context: RunContext) -> None:
        manifest = self.manifests[context.baseline_id]
        if context.baseline_profile_version != manifest["baseline_profile_version"]:
            raise ContractValidationError("baseline profile version drift")
        if context.implementation_commit != manifest["git"]["head"]:
            raise ContractValidationError("run commit differs from baseline manifest")
        if context.git_dirty != manifest["git"]["dirty_expected"]:
            raise ContractValidationError("run dirty state differs from baseline manifest")
        if context.validity_level != self.common["implementation_boundary"][
            "allowed_validity_level"
        ]:
            raise ContractValidationError("run validity exceeds the Phase 3 boundary")
        if context.timezone != "Asia/Shanghai":
            raise ContractValidationError("run timezone must be Asia/Shanghai")
        if context.settlement_protocol_version != self.common["protocol"][
            "settlement_protocol_version"
        ]:
            raise ContractValidationError("settlement protocol version drift")

    def _assemble_result(
        self,
        invocation: PhaseInvocation,
        context: RunContext,
        response: Mapping[str, Any],
        observation: AttemptObservation,
    ) -> dict[str, Any]:
        response = self._validate_adapter_response(invocation, response)
        event_names = self._event_names(response["events"])
        if observation.success_event_count > len(event_names):
            raise ContractValidationError(
                "success_event_count exceeds observed event count"
            )
        completion_blocks = self._completion_blocks(observation)
        common_cost = self._cost_package(
            observation.common_cost,
            COMMON_COST_UNITS,
            applicable=set(COMMON_COST_UNITS),
            label="common cost",
        )
        architecture_cost = self._cost_package(
            observation.architecture_cost,
            ARCHITECTURE_COST_UNITS,
            applicable=set(ARCHITECTURE_APPLICABLE_METRICS[context.baseline_id]),
            label="architecture cost",
        )
        if common_cost["events_count"]["availability"] == "measured" and int(
            common_cost["events_count"]["value"]
        ) != len(event_names):
            raise ContractValidationError("measured events_count differs from receipt")
        if common_cost["completion_blocks"]["availability"] == "measured":
            if completion_blocks is None or int(
                common_cost["completion_blocks"]["value"]
            ) != completion_blocks:
                raise ContractValidationError(
                    "measured completion_blocks differs from block observation"
                )

        accepted = response["dispatch_success"]
        protocol_outcome = "accepted" if accepted else "rejected"
        refund_values = (observation.refund_recipient, observation.refund_amount)
        if invocation.phase is Phase.REFUND_FUNDED and accepted:
            if any(value is None for value in refund_values):
                raise ContractValidationError(
                    "accepted refund requires explicit recipient and amount observations"
                )
            pre = response["pre_snapshot"]
            post = response["post_snapshot"]
            if (
                pre["order"]["state"] != "Funded"
                or pre["payment"]["state"] != "Escrowed"
                or post["order"]["state"] != "Refunded"
                or post["payment"]["state"] != "Refunded"
            ):
                raise ContractValidationError(
                    "accepted refund contradicts authoritative order/payment snapshots"
                )
            if post["order"]["buyer"] != observation.refund_recipient:
                raise ContractValidationError(
                    "refund recipient differs from authoritative recorded buyer"
                )
            if pre["payment"]["escrow_amount"] != observation.refund_amount:
                raise ContractValidationError(
                    "refund amount differs from authoritative pre-refund escrow"
                )
        elif any(value is not None for value in refund_values):
            raise ContractValidationError(
                "refund observations are only valid for an accepted refund"
            )
        expected_match = (
            invocation.expected_outcome == "accept" and accepted
        ) or (
            invocation.expected_outcome == "reject"
            and not accepted
            and response["common_error_code"] == invocation.expected_error_code
        )
        correctness = observation.correctness.to_mapping()
        correctness.update(
            {
                "expected_outcome_match": expected_match,
                "false_accept_count": int(
                    invocation.expected_outcome == "reject" and accepted
                ),
                "false_reject_count": int(
                    invocation.expected_outcome == "accept" and not accepted
                ),
                "timeout_count": 0,
            }
        )
        post = response["post_snapshot"]
        evidence = post["evidence"]
        return {
            "schema_id": "assetxchain-exp2-normalized-result",
            "schema_version": 1,
            "contract_version": CONTRACT_VERSION,
            "run": context.to_mapping(),
            "request": {
                "scenario_id": invocation.scenario_id,
                "attempt_id": invocation.attempt_id,
                "phase": invocation.phase.value,
                "operation": invocation.operation.value if invocation.operation else None,
                "expected_outcome": invocation.expected_outcome,
                "expected_error_code": invocation.expected_error_code,
            },
            "outcome": {
                "dispatch_success": accepted,
                "protocol_outcome": protocol_outcome,
                "common_error_code": response["common_error_code"],
                "native_error": response["native_error"],
                "included_block": observation.included_block,
                "completion_block": observation.completion_block,
                "completion_blocks": completion_blocks,
            },
            "snapshots": {
                "pre": response["pre_snapshot"],
                "post": post,
            },
            "events": {
                "event_names": event_names,
                "success_event_count": observation.success_event_count,
                "post_state_commitment": observation.post_state_commitment,
            },
            "settlement": {
                "settlement_id": evidence["settlement_id"],
                "settlement_record_count": evidence["settlement_record_count"],
                "refund_recipient": observation.refund_recipient,
                "refund_amount": observation.refund_amount,
            },
            "correctness": correctness,
            "cost": common_cost,
            "architecture_cost": architecture_cost,
            "claim_boundary": context.claim_boundary,
        }

    def _validate_adapter_response(
        self, invocation: PhaseInvocation, response: Mapping[str, Any]
    ) -> dict[str, Any]:
        if not isinstance(response, Mapping):
            raise ContractValidationError("adapter response must be an object")
        required = (
            "baseline_id",
            "phase",
            "dispatch_success",
            "native_error",
            "common_error_code",
            "pre_snapshot",
            "post_snapshot",
            "events",
            "receipt",
        )
        _exact_keys(response, required, "adapter response")
        response = dict(response)
        if response["baseline_id"] != invocation.baseline_id:
            raise ContractValidationError("adapter response baseline mismatch")
        if response["phase"] != invocation.phase.value:
            raise ContractValidationError("adapter response phase mismatch")
        if not isinstance(response["dispatch_success"], bool):
            raise ContractValidationError("adapter dispatch_success must be boolean")
        if response["native_error"] is not None and not isinstance(
            response["native_error"], str
        ):
            raise ContractValidationError("adapter native_error must be string or null")
        common_error = response["common_error_code"]
        if common_error is not None and common_error not in self.common["common_error_codes"]:
            raise ContractValidationError("adapter returned an unfrozen common error")
        if response["dispatch_success"] and (
            response["native_error"] is not None or common_error is not None
        ):
            raise ContractValidationError("successful adapter response contains an error")
        if not response["dispatch_success"] and common_error is None:
            raise ContractValidationError("rejected adapter response lacks a common error")
        if invocation.phase is Phase.SETUP:
            if response["pre_snapshot"] is not None:
                raise ContractValidationError("setup adapter response must have null pre")
        elif not isinstance(response["pre_snapshot"], Mapping):
            raise ContractValidationError("non-setup adapter response requires pre snapshot")
        if not isinstance(response["post_snapshot"], Mapping):
            raise ContractValidationError("adapter response requires post snapshot")
        if response["pre_snapshot"] is not None:
            validate_instance(response["pre_snapshot"], self.snapshot_schema)
        validate_instance(response["post_snapshot"], self.snapshot_schema)
        if not isinstance(response["events"], list):
            raise ContractValidationError("adapter events must be an array")
        return response

    @staticmethod
    def _event_names(events: Sequence[Any]) -> list[str]:
        names = []
        for event in events:
            if isinstance(event, str) and event:
                names.append(event)
                continue
            if isinstance(event, Mapping):
                name = event.get("event_name", event.get("name"))
                if isinstance(name, str) and name:
                    names.append(name)
                    continue
            raise ContractValidationError("adapter event omitted a normalized name")
        return names

    @staticmethod
    def _completion_blocks(observation: AttemptObservation) -> int | None:
        if observation.included_block is None or observation.completion_block is None:
            if observation.included_block != observation.completion_block:
                raise ContractValidationError(
                    "included and completion blocks must both be present or both be null"
                )
            return None
        if observation.completion_block < observation.included_block:
            raise ContractValidationError("completion block precedes inclusion block")
        return observation.completion_block - observation.included_block

    @staticmethod
    def _cost_package(
        supplied: Mapping[str, MetricObservation],
        units: Mapping[str, str],
        *,
        applicable: set[str],
        label: str,
    ) -> dict[str, dict[str, Any]]:
        extras = sorted(set(supplied) - set(units))
        if extras:
            raise ContractValidationError(f"{label} has unknown metrics: {extras}")
        package = {}
        for name, unit in units.items():
            observation = supplied.get(name)
            if observation is not None and observation.unit != unit:
                raise ContractValidationError(
                    f"{label}.{name} unit must be {unit}"
                )
            if name not in applicable:
                if observation is not None and observation.availability != "N/A":
                    raise ContractValidationError(
                        f"{label}.{name} is not applicable to this baseline"
                    )
                observation = MetricObservation("N/A", None, unit)
            elif observation is None:
                observation = MetricObservation("not-collected", None, unit)
            elif observation.availability == "N/A":
                raise ContractValidationError(
                    f"{label}.{name} is applicable and cannot be N/A"
                )
            package[name] = observation.to_mapping()
        return package
