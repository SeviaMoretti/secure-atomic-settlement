from __future__ import annotations

import copy
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
    _path_is_owned,
    load_json,
    validate_contract_bundle,
    validate_instance,
    validate_invocation_semantics,
    validate_manifest_semantics,
    validate_result_semantics,
)


HEX_A = "0x" + "11" * 32
HEX_B = "0x" + "22" * 32
HEX_C = "0x" + "33" * 32


def sample_snapshot(baseline_id: str = "B1") -> dict:
    return {
        "schema_id": "assetxchain-exp2-normalized-snapshot",
        "schema_version": 1,
        "contract_version": "phase3-adapter-contract-v1",
        "baseline_id": baseline_id,
        "authority_source": "contract" if baseline_id == "B1" else "runtime",
        "observed_at": {"block_number": 1, "attempt_sequence": 0},
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
            "seller_balance": "1267650600228229401496703205376",
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
            "availability": "N/A" if baseline_id != "G3" else "observed",
            "authoritative": False,
            "order_state": None,
            "settlement_record_count": None,
        },
    }


def metric(availability: str = "not-collected", value: str | None = None, unit: str = "count") -> dict:
    return {"availability": availability, "value": value, "unit": unit}


def sample_result() -> dict:
    snapshot = sample_snapshot("B1")
    return {
        "schema_id": "assetxchain-exp2-normalized-result",
        "schema_version": 1,
        "contract_version": "phase3-adapter-contract-v1",
        "run": {
            "run_id": "schema-test-only",
            "timestamp": "2026-07-28T12:00:00+08:00",
            "timezone": "Asia/Shanghai",
            "baseline_id": "B1",
            "baseline_name": "Contract-only",
            "baseline_profile_version": "1",
            "settlement_protocol_version": 1,
            "validity_level": "legacy-pilot",
            "implementation_commit": "cf3859eeadfc7286a1f0656a148a9fa245f82605",
            "git_dirty": False,
            "config_path": "configs/dev.json",
            "workload_id": "schema-test-only",
            "seed": 20260726,
            "repetition": 0,
        },
        "request": {
            "scenario_id": "LEG-META-01",
            "attempt_id": "query-0",
            "phase": "query",
            "operation": "TransferMeta",
            "expected_outcome": "accept",
            "expected_error_code": None,
        },
        "outcome": {
            "dispatch_success": True,
            "protocol_outcome": "accepted",
            "common_error_code": None,
            "native_error": None,
            "included_block": 1,
            "completion_block": 1,
            "completion_blocks": 0,
        },
        "snapshots": {"pre": snapshot, "post": copy.deepcopy(snapshot)},
        "events": {
            "event_names": [],
            "success_event_count": 0,
            "post_state_commitment": None,
        },
        "settlement": {
            "settlement_id": None,
            "settlement_record_count": 0,
            "refund_recipient": None,
            "refund_amount": None,
        },
        "correctness": {
            "expected_outcome_match": True,
            "invariant_violation_count": 0,
            "false_accept_count": 0,
            "false_reject_count": 0,
            "duplicate_settlement_count": 0,
            "dirty_order_count": 0,
            "dirty_authorization_count": 0,
            "event_mismatch_count": 0,
            "timeout_count": 0,
            "critical_state_diff_count": 0,
            "unexpected_balance_delta_count": 0,
            "unexpected_index_delta_count": 0,
        },
        "cost": {
            "latency_ms": metric(unit="ms"),
            "completion_blocks": metric(unit="blocks"),
            "partial_fee": metric(unit="plancks"),
            "weight_ref_time": metric(unit="ref_time"),
            "weight_proof_size": metric(unit="bytes"),
            "storage_writes": metric(),
            "events_count": metric(),
            "rpc_response_ms": metric(unit="ms"),
            "phase_call_count": metric(),
        },
        "architecture_cost": {
            "contract_gas_or_ref_time": metric(),
            "contract_storage_writes": metric(),
            "runtime_dispatch_count": metric("N/A"),
            "runtime_storage_writes": metric("N/A"),
            "typed_ce_call_count": metric("N/A"),
            "projection_storage_writes": metric("N/A"),
        },
        "claim_boundary": "schema validation only; not experiment evidence",
    }


class FakeAdapter(BaselineAdapter):
    @property
    def baseline_id(self) -> str:
        return "B1"

    def _result(self, invocation: PhaseInvocation) -> dict:
        return {"phase": invocation.phase.value}

    setup = _result
    create_order = _result
    fund_order = _result
    settle = _result
    cancel_open = _result
    expire_open = _result
    refund_funded = _result
    query = _result


class Phase3ContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.common = load_json(EXP_ROOT / "configs/baselines/phase3-common.json")
        cls.invocation_schema = load_json(EXP_ROOT / "schemas/phase3/phase-invocation.schema.json")
        cls.snapshot_schema = load_json(EXP_ROOT / "schemas/phase3/normalized-snapshot.schema.json")
        cls.result_schema = load_json(EXP_ROOT / "schemas/phase3/normalized-result.schema.json")

    def test_contract_bundle_and_git_isolation(self) -> None:
        result = validate_contract_bundle(EXP_ROOT, check_git=True)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["baselines"], ["B1", "B2", "G3"])
        self.assertEqual(result["common_errors"], 33)
        self.assertEqual(result["fixture_vectors"], 3)
        self.assertEqual(result["fixture_consumers"], 3)
        self.assertEqual(
            result["shared_input_hash"],
            "0xc427552f41308fe6e7345a4af448ef3670caba676f1da46e433a1b0aa88e9c13",
        )
        self.assertTrue(
            self.common["implementation_boundary"][
                "concrete_baseline_adapters_implemented"
            ]
        )
        self.assertTrue(
            self.common["implementation_boundary"][
                "b1_b2_business_logic_implemented"
            ]
        )
        self.assertTrue(
            self.common["implementation_boundary"]["unified_driver_implemented"]
        )
        self.assertEqual(
            self.common["implementation_boundary"]["unified_driver_entrypoint"],
            "scripts/common/phase3_driver.py#Phase3ScenarioDriver",
        )
        self.assertFalse(
            self.common["implementation_boundary"]["formal_experiment_authorized"]
        )

    def test_frozen_baseline_head_must_match_the_declared_worktree(self) -> None:
        manifest = load_json(EXP_ROOT / "configs/baselines/b1.json")
        manifest["git"]["head"] = manifest["git"]["formal_base_commit"]
        with self.assertRaisesRegex(ContractValidationError, "HEAD differs from manifest"):
            validate_manifest_semantics(manifest, check_git=True)

    def test_owned_path_accepts_exact_files_and_directory_descendants(self) -> None:
        owned_paths = ("Cargo.lock", "pallets/market_runtime")
        self.assertTrue(_path_is_owned("Cargo.lock", owned_paths))
        self.assertTrue(
            _path_is_owned("pallets/market_runtime/src/lib.rs", owned_paths)
        )
        self.assertFalse(_path_is_owned("Cargo.lock.backup", owned_paths))
        self.assertFalse(_path_is_owned("pallets/market_runtime-old/src/lib.rs", owned_paths))
        self.assertFalse(_path_is_owned("runtime/src/lib.rs", owned_paths))

    def test_phase_invocation_schema_and_semantics(self) -> None:
        invocation = {
            "schema_id": "assetxchain-exp2-phase-invocation",
            "schema_version": 1,
            "contract_version": "phase3-adapter-contract-v1",
            "run_id": "schema-test-only",
            "baseline_id": "B1",
            "scenario_id": "NEG-BALANCE-01",
            "attempt_id": "fund-0",
            "repetition": 0,
            "phase": "fund_order",
            "operation": "TransferMeta",
            "actor_role": "BUYER_1",
            "expected_outcome": "reject",
            "expected_error_code": "InsufficientBalance",
            "payload": {},
        }
        validate_instance(invocation, self.invocation_schema)
        validate_invocation_semantics(invocation, self.common)

    def test_normalized_snapshot_and_result(self) -> None:
        result = sample_result()
        validate_instance(result, self.result_schema)
        validate_result_semantics(result, self.common, self.snapshot_schema)

    def test_schema_rejects_missing_snapshot_field(self) -> None:
        snapshot = sample_snapshot()
        del snapshot["payment"]["escrow_amount"]
        with self.assertRaises(ContractValidationError):
            validate_instance(snapshot, self.snapshot_schema)

    def test_semantics_reject_wrong_authority_and_na_zero(self) -> None:
        result = sample_result()
        result["snapshots"]["post"]["authority_source"] = "runtime"
        with self.assertRaises(ContractValidationError):
            validate_result_semantics(result, self.common, self.snapshot_schema)

        result = sample_result()
        result["run"]["baseline_name"] = "Runtime-only"
        with self.assertRaises(ContractValidationError):
            validate_result_semantics(result, self.common, self.snapshot_schema)

        result = sample_result()
        result["run"]["validity_level"] = "paper-candidate"
        with self.assertRaises(ContractValidationError):
            validate_result_semantics(result, self.common, self.snapshot_schema)

        result = sample_result()
        result["architecture_cost"]["typed_ce_call_count"] = metric("N/A", "0")
        with self.assertRaises(ContractValidationError):
            validate_result_semantics(result, self.common, self.snapshot_schema)

    def test_projection_availability_follows_baseline_architecture(self) -> None:
        result = sample_result()
        result["snapshots"]["post"]["projection"].update(
            availability="observed",
            order_state="Open",
            settlement_record_count=0,
        )
        with self.assertRaises(ContractValidationError):
            validate_result_semantics(result, self.common, self.snapshot_schema)

        result = sample_result()
        result["snapshots"]["post"]["projection"]["authoritative"] = True
        with self.assertRaises(ContractValidationError):
            validate_result_semantics(result, self.common, self.snapshot_schema)

        result = sample_result()
        result["run"].update(baseline_id="G3", baseline_name="AssetXChain-CRS")
        for snapshot in result["snapshots"].values():
            snapshot.update(baseline_id="G3", authority_source="runtime")
        with self.assertRaises(ContractValidationError):
            validate_result_semantics(result, self.common, self.snapshot_schema)

    def test_adapter_routes_all_frozen_phases(self) -> None:
        adapter = FakeAdapter()
        for phase in Phase:
            invocation = PhaseInvocation(
                run_id="schema-test-only",
                baseline_id="B1",
                scenario_id="LEG-META-01",
                attempt_id=phase.value,
                repetition=0,
                phase=phase,
                operation=None if phase is Phase.SETUP else Operation.TRANSFER_META,
                actor_role="OWNER",
                expected_outcome="accept",
                expected_error_code=None,
                payload={},
            )
            self.assertEqual(adapter.invoke(invocation)["phase"], phase.value)


if __name__ == "__main__":
    unittest.main()
