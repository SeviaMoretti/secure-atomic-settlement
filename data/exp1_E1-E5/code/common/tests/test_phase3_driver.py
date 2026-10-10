from __future__ import annotations

import copy
import json
import sys
import unittest
from pathlib import Path


COMMON_DIR = Path(__file__).resolve().parents[1]
EXP_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(COMMON_DIR))

from phase3_contract import (  # noqa: E402
    BaselineAdapter,
    ContractValidationError,
    Operation,
    Phase,
    PhaseInvocation,
    validate_instance,
    validate_result_semantics,
)
from phase3_driver import (  # noqa: E402
    ARCHITECTURE_COST_UNITS,
    AttemptDefinition,
    AttemptObservation,
    CorrectnessObservation,
    MetricObservation,
    Phase3ScenarioDriver,
    RunContext,
    ScenarioDefinition,
)


HEX_A = "0x" + "11" * 32
HEX_B = "0x" + "22" * 32
HEX_C = "0x" + "33" * 32
MANIFESTS = {
    baseline_id: json.loads(
        (EXP_ROOT / "configs" / "baselines" / f"{baseline_id.lower()}.json").read_text(
            encoding="utf-8"
        )
    )
    for baseline_id in ("B1", "B2", "G3")
}
HEADS = {
    baseline_id: manifest["git"]["head"]
    for baseline_id, manifest in MANIFESTS.items()
}
DIRTY = {
    baseline_id: manifest["git"]["dirty_expected"]
    for baseline_id, manifest in MANIFESTS.items()
}


def snapshot(baseline_id: str, sequence: int = 0) -> dict:
    return {
        "schema_id": "assetxchain-exp2-normalized-snapshot",
        "schema_version": 1,
        "contract_version": "phase3-adapter-contract-v1",
        "baseline_id": baseline_id,
        "authority_source": "contract" if baseline_id == "B1" else "runtime",
        "observed_at": {"block_number": 10, "attempt_sequence": sequence},
        "meta": {
            "exists": True,
            "asset_id": HEX_A,
            "owner": HEX_B,
            "status": "Tradable",
            "state_version": 1,
            "owner_index_matches": True,
            "active_right_count": 0,
            "active_order_id": None,
        },
        "right": {
            "exists": False,
            "right_id": None,
            "holder": None,
            "status": None,
            "state_version": None,
            "parent_asset_id": None,
            "valid": None,
            "remaining_uses": None,
            "holder_index_matches": None,
            "parent_index_matches": None,
            "active_order_id": None,
        },
        "order": {
            "exists": False,
            "order_id": None,
            "operation": None,
            "state": None,
            "seller": None,
            "buyer": None,
            "expiry": None,
            "grace_deadline": None,
            "active_capacity_occupied": False,
            "funded_capacity_occupied": False,
        },
        "authorization": {
            "exists": True,
            "authorization_id": HEX_C,
            "status": "Active",
            "policy_hash": HEX_A,
            "issued_count": 0,
            "reserved_count": 0,
            "issued_use_units": 0,
            "reserved_use_units": 0,
            "consumed": False,
            "consumed_order_id": None,
        },
        "payment": {
            "state": "Unfunded",
            "escrow_amount": "0",
            "buyer_balance": "100000000000000000",
            "seller_balance": "100000000000000000",
            "royalty_balance": None,
            "market_balance": "100000000000000000",
            "safety_balance": None,
        },
        "evidence": {
            "settlement_record_count": 0,
            "settlement_id": None,
            "record_fields_hash": None,
            "pre_state_commitment": None,
        },
        "market": {
            "exists": True,
            "market_id": HEX_C,
            "status": "Active",
            "capabilities": ["TransferMeta", "IssueRight", "TransferRight"],
            "active_orders": 0,
            "funded_orders": 0,
            "active_exposure_value": "0",
        },
        "identity": {
            "settlement_protocol_version": 1,
            "settlement_domain_id": "axc/p2/shared",
            "order_commitment_hash": None,
            "seller_nonce": 0,
            "buyer_nonce": 0,
            "authorization_nonce": 0,
            "object_state_version": 1,
        },
        "state_commitment": HEX_A,
        "projection": {
            "availability": "observed" if baseline_id == "G3" else "N/A",
            "authoritative": False,
            "order_state": None,
            "settlement_record_count": None,
        },
    }


def zero_correctness() -> CorrectnessObservation:
    return CorrectnessObservation(
        invariant_violation_count=0,
        duplicate_settlement_count=0,
        dirty_order_count=0,
        dirty_authorization_count=0,
        event_mismatch_count=0,
        critical_state_diff_count=0,
        unexpected_balance_delta_count=0,
        unexpected_index_delta_count=0,
    )


def observation(**kwargs) -> AttemptObservation:
    return AttemptObservation(correctness=zero_correctness(), **kwargs)


def context(baseline_id: str) -> RunContext:
    return RunContext(
        run_id=f"unit-only-{baseline_id.lower()}",
        timestamp="2026-07-28T16:00:00+08:00",
        baseline_id=baseline_id,
        implementation_commit=HEADS[baseline_id],
        git_dirty=DIRTY[baseline_id],
        config_path="configs/dev.json",
        workload_id="unit-only-shared-driver",
        seed=20260726,
        repetition=0,
        claim_boundary="unit/schema verification only; not experiment evidence",
    )


def payloads(tag: str) -> dict[str, dict]:
    return {
        "B1": {"args": {"native_shape": f"b1-{tag}"}},
        "B2": {"args": {"native_shape": f"b2-{tag}"}},
        "G3": {"args": {"native_shape": f"g3-{tag}"}},
    }


def all_phase_scenario() -> ScenarioDefinition:
    attempts = []
    for phase in Phase:
        per_baseline = payloads(phase.value)
        if phase is Phase.SETUP:
            per_baseline = {
                baseline_id: {"calls": [{"native_shape": baseline_id.lower()}]}
                for baseline_id in HEADS
            }
        attempts.append(
            AttemptDefinition(
                attempt_id=phase.value,
                phase=phase,
                operation=None if phase is Phase.SETUP else Operation.TRANSFER_META,
                actor_role="OWNER",
                expected_outcome="accept",
                expected_error_code=None,
                payload_by_baseline=per_baseline,
            )
        )
    return ScenarioDefinition("LEG-META-01", tuple(attempts))


class RecordingAdapter(BaselineAdapter):
    def __init__(self, baseline_id: str) -> None:
        self._baseline_id = baseline_id
        self.seen: list[PhaseInvocation] = []
        self.response_mutator = None

    @property
    def baseline_id(self) -> str:
        return self._baseline_id

    def _respond(self, invocation: PhaseInvocation) -> dict:
        self.seen.append(invocation)
        accepted = invocation.expected_outcome == "accept"
        pre = None if invocation.phase is Phase.SETUP else snapshot(self.baseline_id, 1)
        post = snapshot(self.baseline_id, 2)
        if invocation.phase is Phase.REFUND_FUNDED and accepted:
            pre["order"].update(
                exists=True, state="Funded", buyer=HEX_B, order_id=HEX_C
            )
            pre["payment"].update(
                state="Escrowed", escrow_amount="3000000000000"
            )
            post["order"].update(
                exists=True, state="Refunded", buyer=HEX_B, order_id=HEX_C
            )
            post["payment"].update(state="Refunded", escrow_amount="0")
        response = {
            "baseline_id": self.baseline_id,
            "phase": invocation.phase.value,
            "dispatch_success": accepted,
            "native_error": None if accepted else f"Native{invocation.expected_error_code}",
            "common_error_code": None if accepted else invocation.expected_error_code,
            "pre_snapshot": pre,
            "post_snapshot": post,
            "events": [{"event_name": "PhaseObserved"}],
            "receipt": {
                "phase_call_count": 999,
                "storage_writes": 999,
            },
        }
        if self.response_mutator is not None:
            self.response_mutator(response)
        return response

    setup = _respond
    create_order = _respond
    fund_order = _respond
    settle = _respond
    cancel_open = _respond
    expire_open = _respond
    refund_funded = _respond
    query = _respond


class Phase3DriverTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.driver = Phase3ScenarioDriver(EXP_ROOT)

    def test_shared_definition_materializes_all_phases_with_only_native_payload_variation(self) -> None:
        scenario = all_phase_scenario()
        with self.assertRaises(TypeError):
            scenario.attempts[1].payload_by_baseline["B1"]["args"][
                "native_shape"
            ] = "changed"
        materialized = {
            baseline_id: self.driver.materialize(scenario, context(baseline_id))
            for baseline_id in HEADS
        }
        expected_phases = [phase.value for phase in Phase]
        for baseline_id, invocations in materialized.items():
            self.assertEqual([item.phase.value for item in invocations], expected_phases)
            self.assertTrue(all(item.baseline_id == baseline_id for item in invocations))
        for index in range(len(scenario.attempts)):
            logical = {
                (
                    item.scenario_id,
                    item.attempt_id,
                    item.phase,
                    item.operation,
                    item.actor_role,
                    item.expected_outcome,
                    item.expected_error_code,
                    item.repetition,
                )
                for item in (materialized[baseline][index] for baseline in HEADS)
            }
            self.assertEqual(len(logical), 1)
        self.assertEqual(
            materialized["B1"][1].payload["args"]["native_shape"],
            "b1-create_order",
        )
        self.assertEqual(
            materialized["B2"][1].payload["args"]["native_shape"],
            "b2-create_order",
        )
        self.assertEqual(
            materialized["G3"][1].payload["args"]["native_shape"],
            "g3-create_order",
        )

    def test_matrix_dispatches_all_attempts_and_builds_schema_valid_results(self) -> None:
        scenario = all_phase_scenario()
        adapters = {baseline: RecordingAdapter(baseline) for baseline in HEADS}
        contexts = {baseline: context(baseline) for baseline in HEADS}
        observations = {
            baseline: {
                attempt.attempt_id: observation(
                    included_block=10,
                    completion_block=11,
                    success_event_count=1,
                    refund_recipient=(
                        HEX_B if attempt.phase is Phase.REFUND_FUNDED else None
                    ),
                    refund_amount=(
                        "3000000000000"
                        if attempt.phase is Phase.REFUND_FUNDED
                        else None
                    ),
                    common_cost={
                        "events_count": MetricObservation("measured", "1", "count"),
                        "completion_blocks": MetricObservation(
                            "measured", "1", "blocks"
                        ),
                    },
                )
                for attempt in scenario.attempts
            }
            for baseline in HEADS
        }
        results = self.driver.run_matrix(scenario, adapters, contexts, observations)
        self.assertEqual(list(results), ["B1", "B2", "G3"])
        for baseline_id, baseline_results in results.items():
            self.assertEqual(len(baseline_results), 8)
            self.assertEqual(len(adapters[baseline_id].seen), 8)
            self.assertIsNone(baseline_results[0]["snapshots"]["pre"])
            self.assertTrue(
                all(item["snapshots"]["pre"] is not None for item in baseline_results[1:])
            )
            for result in baseline_results:
                validate_instance(result, self.driver.result_schema)
                validate_result_semantics(
                    result, self.driver.common, self.driver.snapshot_schema
                )
                self.assertTrue(result["correctness"]["expected_outcome_match"])
                self.assertEqual(result["events"]["event_names"], ["PhaseObserved"])

    def test_expected_rejection_uses_stable_error_and_timeout_is_not_fabricated(self) -> None:
        attempt = AttemptDefinition(
            attempt_id="fund-reject",
            phase=Phase.FUND_ORDER,
            operation=Operation.TRANSFER_META,
            actor_role="BUYER_1",
            expected_outcome="reject",
            expected_error_code="InsufficientBalance",
            payload_by_baseline=payloads("reject"),
        )
        scenario = ScenarioDefinition("NEG-BALANCE-01", (attempt,))
        result = self.driver.run_scenario(
            RecordingAdapter("B2"),
            scenario,
            context("B2"),
            {attempt.attempt_id: observation(success_event_count=0)},
        )[0]
        self.assertEqual(result["outcome"]["protocol_outcome"], "rejected")
        self.assertEqual(result["outcome"]["common_error_code"], "InsufficientBalance")
        self.assertTrue(result["correctness"]["expected_outcome_match"])
        self.assertEqual(result["correctness"]["timeout_count"], 0)

    def test_architecture_availability_matrix_and_explicit_metrics_are_preserved(self) -> None:
        applicable = {
            "B1": {"contract_gas_or_ref_time", "contract_storage_writes"},
            "B2": {"runtime_dispatch_count", "runtime_storage_writes"},
            "G3": set(ARCHITECTURE_COST_UNITS),
        }
        attempt = all_phase_scenario().attempts[-1]
        scenario = ScenarioDefinition("LEG-META-01", (attempt,))
        for baseline_id in HEADS:
            measured_name = sorted(applicable[baseline_id])[0]
            result = self.driver.run_scenario(
                RecordingAdapter(baseline_id),
                scenario,
                context(baseline_id),
                {
                    attempt.attempt_id: observation(
                        architecture_cost={
                            measured_name: MetricObservation(
                                "measured",
                                "7",
                                ARCHITECTURE_COST_UNITS[measured_name],
                            )
                        }
                    )
                },
            )[0]
            for name, metric in result["architecture_cost"].items():
                if name == measured_name:
                    self.assertEqual(metric["availability"], "measured")
                    self.assertEqual(metric["value"], "7")
                elif name in applicable[baseline_id]:
                    self.assertEqual(metric["availability"], "not-collected")
                    self.assertIsNone(metric["value"])
                else:
                    self.assertEqual(metric["availability"], "N/A")
                    self.assertIsNone(metric["value"])

    def test_receipt_costs_are_not_invented_and_common_na_is_blocked(self) -> None:
        attempt = all_phase_scenario().attempts[-1]
        scenario = ScenarioDefinition("LEG-META-01", (attempt,))
        result = self.driver.run_scenario(
            RecordingAdapter("G3"),
            scenario,
            context("G3"),
            {attempt.attempt_id: observation()},
        )[0]
        self.assertTrue(
            all(metric["availability"] == "not-collected" for metric in result["cost"].values())
        )
        self.assertTrue(all(metric["value"] is None for metric in result["cost"].values()))

        with self.assertRaises(ContractValidationError):
            self.driver.run_scenario(
                RecordingAdapter("G3"),
                scenario,
                context("G3"),
                {
                    attempt.attempt_id: observation(
                        common_cost={
                            "latency_ms": MetricObservation("N/A", None, "ms")
                        }
                    )
                },
            )
        with self.assertRaises(ContractValidationError):
            MetricObservation("N/A", "0", "count")
        with self.assertRaises(ContractValidationError):
            MetricObservation("measured", "1.5", "count")

    def test_unknown_or_mismatched_inputs_block_without_result_fabrication(self) -> None:
        attempt = all_phase_scenario().attempts[-1]
        scenario = ScenarioDefinition("LEG-META-01", (attempt,))
        adapter = RecordingAdapter("B1")
        with self.assertRaises(ContractValidationError):
            self.driver.run_scenario(adapter, scenario, context("B1"), {})

        bad_context = RunContext(
            **{**context("B1").__dict__, "implementation_commit": "0" * 40}
        )
        with self.assertRaisesRegex(
            ContractValidationError, "run commit differs from baseline manifest"
        ):
            self.driver.materialize(scenario, bad_context)

        adapter.response_mutator = lambda response: response.update(baseline_id="B2")
        with self.assertRaises(ContractValidationError):
            self.driver.run_scenario(
                adapter,
                scenario,
                context("B1"),
                {attempt.attempt_id: observation()},
            )

        adapter.response_mutator = lambda response: response["post_snapshot"].update(
            authority_source="runtime"
        )
        with self.assertRaises(ContractValidationError):
            self.driver.run_scenario(
                adapter,
                scenario,
                context("B1"),
                {attempt.attempt_id: observation()},
            )

    def test_matrix_rejects_shared_workload_drift_and_validity_promotion(self) -> None:
        scenario = ScenarioDefinition(
            "LEG-META-01", (all_phase_scenario().attempts[-1],)
        )
        contexts = {baseline: context(baseline) for baseline in HEADS}
        contexts["B2"] = RunContext(
            **{**contexts["B2"].__dict__, "seed": 99}
        )
        adapters = {baseline: RecordingAdapter(baseline) for baseline in HEADS}
        observations = {
            baseline: {"query": observation()} for baseline in HEADS
        }
        with self.assertRaises(ContractValidationError):
            self.driver.run_matrix(scenario, adapters, contexts, observations)

        promoted = RunContext(
            **{**context("G3").__dict__, "validity_level": "paper-candidate"}
        )
        with self.assertRaises(ContractValidationError):
            self.driver.materialize(scenario, promoted)

    def test_applicable_architecture_metric_cannot_be_na(self) -> None:
        attempt = all_phase_scenario().attempts[-1]
        with self.assertRaises(ContractValidationError):
            self.driver.run_scenario(
                RecordingAdapter("B1"),
                ScenarioDefinition("LEG-META-01", (attempt,)),
                context("B1"),
                {
                    attempt.attempt_id: observation(
                        architecture_cost={
                            "contract_gas_or_ref_time": MetricObservation(
                                "N/A", None, "ref_time"
                            )
                        }
                    )
                },
            )

    def test_accepted_refund_requires_explicit_authoritative_observation(self) -> None:
        attempt = all_phase_scenario().attempts[6]
        scenario = ScenarioDefinition("LEG-REFUND-01", (attempt,))
        with self.assertRaises(ContractValidationError):
            self.driver.run_scenario(
                RecordingAdapter("B2"),
                scenario,
                context("B2"),
                {attempt.attempt_id: observation()},
            )
        result = self.driver.run_scenario(
            RecordingAdapter("B2"),
            scenario,
            context("B2"),
            {
                attempt.attempt_id: observation(
                    refund_recipient=HEX_B, refund_amount="3000000000000"
                )
            },
        )[0]
        self.assertEqual(result["settlement"]["refund_recipient"], HEX_B)
        self.assertEqual(result["settlement"]["refund_amount"], "3000000000000")

    def test_result_semantics_reject_timeout_as_expected_rejection(self) -> None:
        attempt = all_phase_scenario().attempts[-1]
        result = self.driver.run_scenario(
            RecordingAdapter("B1"),
            ScenarioDefinition("LEG-META-01", (attempt,)),
            context("B1"),
            {attempt.attempt_id: observation()},
        )[0]
        result = copy.deepcopy(result)
        result["request"]["expected_outcome"] = "reject"
        result["request"]["expected_error_code"] = "InsufficientBalance"
        result["outcome"]["dispatch_success"] = False
        result["outcome"]["protocol_outcome"] = "timeout"
        result["correctness"]["timeout_count"] = 1
        result["correctness"]["expected_outcome_match"] = False
        with self.assertRaises(ContractValidationError):
            validate_result_semantics(
                result, self.driver.common, self.driver.snapshot_schema
            )


if __name__ == "__main__":
    unittest.main()
