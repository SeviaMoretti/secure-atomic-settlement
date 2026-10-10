from __future__ import annotations

import copy
import json
import sys
import unittest
from pathlib import Path

COMMON_DIR = Path(__file__).resolve().parents[1]
if str(COMMON_DIR) not in sys.path:
    sys.path.insert(0, str(COMMON_DIR))

from phase3_contract import ContractValidationError, load_json, validate_instance
from validate_s314_smoke import (
    CONFIG_PATH,
    DEV_PATH,
    EVIDENCE_SCHEMA_PATH,
    validate_evidence_semantics,
)


def metric(availability: str, value: str | None, unit: str) -> dict[str, object]:
    return {"availability": availability, "value": value, "unit": unit}


def evidence(baseline: str) -> dict[str, object]:
    config = load_json(CONFIG_PATH)
    dev = load_json(DEV_PATH)
    accounts = {
        role: details["ss58_address"]
        for role, details in dev["shared_fixture"]["roles"].items()
    }
    scenarios = {
        "TransferMeta": "LEG-META-01",
        "IssueRight": "LEG-ISSUE-01",
        "TransferRight": "LEG-RESALE-01",
    }
    operations = {}
    for index, (operation, scenario) in enumerate(scenarios.items(), start=1):
        operations[operation] = {
            "scenario_id": scenario,
            "order_id": f"0x{index:02x}" + "00" * 31,
            "expiry": 30,
            "created_block": 10,
            "expiry_offset": 20,
            "state": "Settled",
            "settlement_record_count": 1,
            "evidence_field_count": 24,
            "audit": {"record": {"operation": operation, "grossPrice": config["prices_plancks"][operation]}},
        }
    applicable = {
        "B1": {"contract_gas_or_ref_time", "contract_storage_writes"},
        "B2": {"runtime_dispatch_count", "runtime_storage_writes"},
        "G3": {
            "contract_gas_or_ref_time", "contract_storage_writes", "runtime_dispatch_count",
            "runtime_storage_writes", "typed_ce_call_count", "projection_storage_writes",
        },
    }[baseline]
    costs = {}
    for name in (
        "contract_gas_or_ref_time", "contract_storage_writes", "runtime_dispatch_count",
        "runtime_storage_writes", "typed_ce_call_count", "projection_storage_writes",
    ):
        costs[name] = metric("not-collected", None, "count") if name in applicable else metric("N/A", None, "count")
    return {
        "schema_id": "assetxchain-exp2-s314-live-smoke",
        "schema_version": 1,
        "run": {
            "run_id": f"test-{baseline.lower()}", "baseline_id": baseline,
            "validity_level": "legacy-pilot", "started_at": "2026-07-29T00:00:00+08:00",
            "finished_at": "2026-07-29T00:01:00+08:00", "timezone": "Asia/Shanghai",
            "implementation_commit": "a" * 40, "git_dirty": baseline != "G3",
            "fixture_profile_id": config["fixture_profile_id"],
            "shared_input_hash": config["shared_input_hash"], "seed": config["seed"],
            "claim_boundary": config["claim_boundary"],
        },
        "environment": {
            "rpc": "ws://127.0.0.1:9944", "chain": "Development",
            "runtime": "assetxchain-1", "genesis_hash": "0x" + "01" * 32,
            "metadata_hash": "0x" + "02" * 32, "deterministic_reset": "--dev --tmp",
        },
        "shared_input": {
            "quantity": config["quantity"], "prices_plancks": config["prices_plancks"],
            "timing_offsets_blocks": config["timing_offsets_blocks"],
            "experiment_families": config["experiment_families"],
            "scenario_ids": config["scenario_ids"],
        },
        "transactions": [{}], "events": {}, "operations": operations,
        "rejection": {
            "scenario_id": "NEG-PAYER-01", "cleanliness_scenario_id": "CLEAN-FUND-01",
            "common_error_code": "PayerBuyerMismatch", "native_error": "PayerBuyerMismatch",
            "authoritative_state_unchanged": True, "success_event_count": 0,
        },
        "duplicate": {
            "scenario_id": "DUP-SETTLE-01", "common_error_code": "AlreadySettled",
            "accepted_attempt_count": 1, "settlement_record_count": 1,
            "second_success_event_count": 0, "authoritative_state_unchanged": True,
        },
        "audit": {
            "authoritative_source": "contract" if baseline == "B1" else "runtime",
            "contract_projection_used_for_authority": False,
            "settled_operation_count": 3, "settlement_record_count": 3,
            "all_record_field_counts": [24, 24, 24],
            "amount_reconciliation_passed": True,
            "owner_holder_reconciliation_passed": True,
            "issue_quota_reconciliation_passed": True,
        },
        "architecture_cost": costs, "accounts": accounts, "status": "PASS",
    }


class S314SmokeValidationTests(unittest.TestCase):
    def setUp(self) -> None:
        self.config = load_json(CONFIG_PATH)
        self.dev = load_json(DEV_PATH)
        self.schema = load_json(EVIDENCE_SCHEMA_PATH)

    def test_three_baselines_close_same_smoke_surface(self) -> None:
        for baseline in ("B1", "B2", "G3"):
            value = evidence(baseline)
            validate_instance(value, self.schema)
            validate_evidence_semantics(value, self.config, self.dev)

    def test_applicable_metric_cannot_be_na(self) -> None:
        value = evidence("B2")
        value["architecture_cost"]["runtime_dispatch_count"] = metric("N/A", None, "count")
        with self.assertRaisesRegex(ContractValidationError, "applicable"):
            validate_evidence_semantics(value, self.config, self.dev)

    def test_structural_na_cannot_be_numeric_zero(self) -> None:
        value = evidence("B1")
        value["architecture_cost"]["runtime_dispatch_count"] = metric("N/A", "0", "count")
        with self.assertRaisesRegex(ContractValidationError, "must be null"):
            validate_evidence_semantics(value, self.config, self.dev)

    def test_account_or_price_drift_blocks(self) -> None:
        value = evidence("G3")
        value["accounts"]["BUYER_1"] = value["accounts"]["BUYER_2"]
        with self.assertRaisesRegex(ContractValidationError, "accounts drift"):
            validate_evidence_semantics(value, self.config, self.dev)
        value = evidence("G3")
        value["operations"]["IssueRight"]["audit"]["record"]["grossPrice"] = "0"
        with self.assertRaisesRegex(ContractValidationError, "record price mismatch"):
            validate_evidence_semantics(value, self.config, self.dev)

    def test_b1_b2_expiry_scheduling_counts_pre_create_extrinsics(self) -> None:
        scenarios_dir = COMMON_DIR.parent / "scenarios"
        for baseline in ("b1", "b2"):
            source = (scenarios_dir / f"s314_{baseline}_live_smoke.js").read_text(encoding="utf-8")
            self.assertIn("nextExpiry(api, preCreateExtrinsics = 0)", source)
            self.assertIn("TIMING.orderExpiry + preCreateExtrinsics + 1", source)
            self.assertEqual(source.count("nextExpiry(api, 1)"), 2)
            self.assertEqual(source.count("nextExpiry(api);"), 1)

    def test_b1_uses_registry_weight_v2_for_contract_calls(self) -> None:
        source = (COMMON_DIR.parent / "scenarios" / "s314_b1_live_smoke.js").read_text(encoding="utf-8")
        self.assertNotIn("gasLimit: GAS_LIMIT", source)
        self.assertEqual(source.count("createType('WeightV2', GAS_LIMIT)"), 2)

    def test_b1_uses_contract_generated_market_id(self) -> None:
        source = (COMMON_DIR.parent / "scenarios" / "s314_b1_live_smoke.js").read_text(encoding="utf-8")
        self.assertNotIn("const marketId = fixedHash(0x51);", source)
        self.assertIn("const registeredMarket = await contractCall", source)
        self.assertIn("const marketId = hash32Hex(registeredMarket.dry.decoded.value", source)
        self.assertIn("bytes = Object.values(value);", source)
        self.assertIn("u8aToHex(Uint8Array.from(bytes || []))", source)
        self.assertIn(
            "'register_market', 'registerMarket', accounts.OWNER, [\n      accounts.MARKET_OK.address,",
            source,
        )

    def test_three_baselines_anchor_right_validity_at_current_block(self) -> None:
        scenarios_dir = COMMON_DIR.parent / "scenarios"
        for baseline in ("b1", "b2", "g3"):
            source = (scenarios_dir / f"s314_{baseline}_live_smoke.js").read_text(encoding="utf-8")
            self.assertIn("const fixtureAnchor =", source)
            anchor_line = next(
                line.strip() for line in source.splitlines() if "const fixtureAnchor =" in line
            )
            self.assertNotIn("+ 1", anchor_line)
            self.assertIn("number.toNumber()", anchor_line)

    def test_b1_decodes_language_and_business_result_layers(self) -> None:
        source = (COMMON_DIR.parent / "scenarios" / "s314_b1_live_smoke.js").read_text(encoding="utf-8")
        self.assertIn("for (let depth = 0; depth < 4; depth += 1)", source)
        self.assertIn("value = value.ok;", source)
        self.assertIn("return { ok: false, error: value.err };", source)

    def test_b1_decodes_native_revert_data_without_synthetic_errors(self) -> None:
        source = (COMMON_DIR.parent / "scenarios" / "s314_b1_live_smoke.js").read_text(encoding="utf-8")
        self.assertIn("contract.api.call.contractsApi.call(", source)
        self.assertIn("function returnFlagsBits(flags)", source)
        self.assertIn("flags.bits !== undefined", source)
        self.assertIn("value.includes('Revert')", source)
        self.assertIn("unsupported contract return flags", source)
        self.assertIn("returnFlagsBits(execution.flags)", source)
        self.assertIn("Uint8Array.from([0, ...raw])", source)
        self.assertIn("business error did not set the contract revert flag", source)
        self.assertNotIn("expectedErrorIndex", source)

    def test_b1_runtime_limit_can_deploy_the_release_contract(self) -> None:
        runtime_config = Path("/home/gz785/code/assetxchain-b1/runtime/src/configs/mod.rs").read_text(encoding="utf-8")
        wasm = Path(
            "/home/gz785/code/assetxchain-b1/contracts/market_pure_contract/target/ink/market_pure_contract.wasm"
        )
        self.assertGreaterEqual(runtime_config.count("128 * 1024"), 2)
        self.assertLessEqual(wasm.stat().st_size, 128 * 1024)

        manifest = json.loads((COMMON_DIR.parents[1] / "configs" / "baselines" / "b1.json").read_text(encoding="utf-8"))
        ownership = {item["path"]: item["role"] for item in manifest["ownership"]}
        self.assertIn("runtime/src/configs/mod.rs", ownership)
        self.assertIn("Contracts deploy limit", ownership["runtime/src/configs/mod.rs"])
        self.assertNotIn("runtime", ownership)
        self.assertNotIn("runtime/src", ownership)

    def test_g3_passes_genesis_hash_into_shared_execution(self) -> None:
        source = (COMMON_DIR.parent / "scenarios" / "s314_g3_live_smoke.js").read_text(encoding="utf-8")
        self.assertNotIn("evidence.genesisHash", source)
        self.assertIn("async function executeOrder(api, genesisHash, contractAddress", source)
        self.assertEqual(source.count("executeOrder(api, genesisHash, contractAddress"), 3)

    def test_g3_reads_meta_and_right_authority_from_runtime_api(self) -> None:
        source = (COMMON_DIR.parent / "scenarios" / "s314_g3_live_smoke.js").read_text(encoding="utf-8")
        self.assertNotIn("api.query.dataAssets.rightTokens", source)
        self.assertEqual(source.count("api.call.dataAssetsApi.getCertificate"), 3)
        self.assertEqual(source.count("api.call.dataAssetsApi.getAsset(metaAsset)"), 1)
        self.assertIn("const transferredMetaCore = field(transferredMeta, 'core', 'core');", source)
        self.assertIn("transferred meta owner mismatch", source)
        self.assertIn("issued right holder mismatch", source)
        self.assertIn("transferred right holder mismatch", source)
        self.assertEqual(source.count("assert(accountHex(field("), 4)
        self.assertIn("contract_projection_used_for_authority: false", source)

    def test_g3_schedules_each_order_expiry_from_its_own_create_path(self) -> None:
        source = (COMMON_DIR.parent / "scenarios" / "s314_g3_live_smoke.js").read_text(encoding="utf-8")
        common_source = (COMMON_DIR.parent / "scenarios" / "s314_live_common.js").read_text(encoding="utf-8")
        self.assertIn("const metaExpiry = (await api.rpc.chain.getHeader()).number.toNumber() + TIMING.orderExpiry + 2;", source)
        self.assertIn("const issueExpiry = (await api.rpc.chain.getHeader()).number.toNumber() + TIMING.orderExpiry + 2;", source)
        self.assertIn("const resaleExpiry = (await api.rpc.chain.getHeader()).number.toNumber() + TIMING.orderExpiry + 1;", source)
        self.assertGreater(source.index("const issueExpiry ="), source.index("meta_duplicate_settle_reject"))
        self.assertLess(source.index("const issueExpiry ="), source.index("const issueSigned ="))
        self.assertIn("api.rpc.chain.getHeader(blockHash)", common_source)
        self.assertIn("blockNumber:", common_source)

    def test_smoke_runner_serializes_clean_dirty_paths_as_array(self) -> None:
        source = (COMMON_DIR.parent / "scenarios" / "run_s314_smoke.ps1").read_text(encoding="utf-8")
        self.assertIn("[object[]]$dirtyPaths = @()", source)
        self.assertNotIn("$dirtyPaths = if ($dirtyText)", source)

    def test_right_validity_uses_block_height_in_b1_and_g3(self) -> None:
        b1_source = Path(
            "/home/gz785/code/assetxchain-b1/contracts/market_pure_contract/lib.rs"
        ).read_text(encoding="utf-8")
        b1_production = b1_source.rsplit("    #[cfg(test)]\n    mod tests {", maxsplit=1)[0]
        self.assertNotIn("block_timestamp()", b1_production)
        self.assertGreaterEqual(b1_production.count("self.env().block_number()"), 4)

        g3_source = Path(
            "/home/gz785/code/assetxchain-g3-v2/pallets/dataassets/src/lib.rs"
        ).read_text(encoding="utf-8")
        self.assertIn("fn current_validity_block() -> u64", g3_source)
        self.assertNotIn("right.is_valid(Self::current_timestamp())", g3_source)
        self.assertGreaterEqual(g3_source.count("Self::current_validity_block()"), 8)


if __name__ == "__main__":
    unittest.main()
