from __future__ import annotations

import copy
import re
import sys
import unittest
from pathlib import Path


EXP_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(EXP_ROOT / "scripts/common"))
sys.path.insert(0, str(EXP_ROOT / "scripts/baselines"))

from g3_adapter import (  # noqa: E402
    COMMON_ERROR_CODES,
    COMMON_ERROR_BY_STATUS,
    RUNTIME_QUERIES,
    G3Adapter,
    ContractValidationError,
    normalize_common_error,
)
from phase3_contract import Operation, Phase, PhaseInvocation, load_json  # noqa: E402
from scripts.common.tests.test_phase3_contract import sample_snapshot  # noqa: E402


class RecordingTransport:
    def __init__(
        self,
        *,
        runtime_snapshot: dict | None = None,
        projection: dict | None = None,
        native_error: str | None = None,
        common_error_code: str | None = None,
    ) -> None:
        snapshot = runtime_snapshot or sample_snapshot("G3")
        snapshot = copy.deepcopy(snapshot)
        snapshot.pop("projection", None)
        self.runtime_snapshot = snapshot
        self.projection = projection or {
            "availability": "observed",
            "authoritative": False,
            "order_state": "Open",
            "settlement_record_count": 0,
        }
        self.native_error = native_error
        self.common_error_code = common_error_code
        self.requests: list[dict] = []

    def request(self, request: dict) -> dict:
        self.requests.append(request)
        if request["kind"] == "runtime_query_bundle":
            return {"snapshot": copy.deepcopy(self.runtime_snapshot)}
        if request["kind"] == "contract_projection_query":
            return copy.deepcopy(self.projection)
        success = self.native_error is None and self.common_error_code is None
        response = {
            "dispatch_success": success,
            "native_error": self.native_error,
            "common_error_code": self.common_error_code,
            "events": ["MutationAccepted"] if success else [],
            "receipt": {"block_number": 1},
        }
        if not success:
            response["error_source"] = (
                "runtime-dispatch"
                if request["kind"] == "runtime_extrinsic"
                else "chain-extension-v2"
            )
        return response


def invocation(phase: Phase) -> PhaseInvocation:
    return PhaseInvocation(
        run_id="adapter-test-only",
        baseline_id="G3",
        scenario_id="LEG-META-01",
        attempt_id=phase.value,
        repetition=0,
        phase=phase,
        operation=None if phase is Phase.SETUP else Operation.TRANSFER_META,
        actor_role="RELAYER",
        expected_outcome="accept",
        expected_error_code=None,
        payload={
            "calls": [{"call": "register_market", "args": {}}],
            "args": {"order_id": "0x" + "11" * 32},
            "query": {"order_id": "0x" + "11" * 32},
        },
    )


class G3AdapterTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.snapshot_schema = load_json(
            EXP_ROOT / "schemas/phase3/normalized-snapshot.schema.json"
        )

    def test_routes_all_eight_phases_across_frozen_architecture(self) -> None:
        transport = RecordingTransport()
        adapter = G3Adapter(transport, snapshot_schema=self.snapshot_schema)
        for phase in Phase:
            result = adapter.invoke(invocation(phase))
            self.assertEqual(result["baseline_id"], "G3")
            self.assertEqual(result["phase"], phase.value)
            self.assertTrue(result["dispatch_success"])

        relays = [
            request["message"]
            for request in transport.requests
            if request["kind"] == "contract_relay"
        ]
        self.assertEqual(
            relays,
            [
                "relay_create_order",
                "relay_fund_order",
                "relay_settle",
                "relay_cancel_open",
                "relay_refund_funded",
            ],
        )
        runtime_calls = [
            request["call"]
            for request in transport.requests
            if request["kind"] == "runtime_extrinsic"
        ]
        self.assertEqual(runtime_calls, ["expire_open"])

    def test_every_snapshot_reads_all_runtime_queries_then_projection(self) -> None:
        transport = RecordingTransport()
        G3Adapter(transport).settle(invocation(Phase.SETTLE))
        kinds = [request["kind"] for request in transport.requests]
        self.assertEqual(
            kinds,
            [
                "runtime_query_bundle",
                "contract_projection_query",
                "contract_relay",
                "runtime_query_bundle",
                "contract_projection_query",
            ],
        )
        runtime_queries = [
            request
            for request in transport.requests
            if request["kind"] == "runtime_query_bundle"
        ]
        self.assertTrue(
            all(request["queries"] == list(RUNTIME_QUERIES) for request in runtime_queries)
        )
        self.assertTrue(
            all(request["event_source"] == "system-events" for request in runtime_queries)
        )

    def test_projection_cannot_override_runtime_authority(self) -> None:
        runtime_snapshot = sample_snapshot("G3")
        runtime_snapshot["order"]["state"] = "Funded"
        runtime_snapshot["evidence"]["settlement_record_count"] = 1
        projection = {
            "availability": "observed",
            "authoritative": False,
            "order_state": "Settled",
            "settlement_record_count": 99,
        }
        result = G3Adapter(
            RecordingTransport(
                runtime_snapshot=runtime_snapshot,
                projection=projection,
            ),
            snapshot_schema=self.snapshot_schema,
        ).query(invocation(Phase.QUERY))
        snapshot = result["post_snapshot"]
        self.assertEqual(snapshot["authority_source"], "runtime")
        self.assertEqual(snapshot["order"]["state"], "Funded")
        self.assertEqual(snapshot["evidence"]["settlement_record_count"], 1)
        self.assertEqual(snapshot["projection"], projection)
        self.assertFalse(snapshot["projection"]["authoritative"])

    def test_unavailable_projection_does_not_block_runtime_snapshot(self) -> None:
        projection = {
            "availability": "unavailable",
            "authoritative": False,
            "order_state": None,
            "settlement_record_count": None,
        }
        result = G3Adapter(
            RecordingTransport(projection=projection),
            snapshot_schema=self.snapshot_schema,
        ).query(invocation(Phase.QUERY))
        self.assertEqual(result["post_snapshot"]["projection"], projection)

    def test_runtime_authority_or_snapshot_omission_is_blocking(self) -> None:
        wrong_authority = sample_snapshot("G3")
        wrong_authority["authority_source"] = "contract"
        with self.assertRaises(ContractValidationError):
            G3Adapter(RecordingTransport(runtime_snapshot=wrong_authority)).query(
                invocation(Phase.QUERY)
            )

        transport = RecordingTransport()
        transport.runtime_snapshot = {"authority_source": "runtime"}
        with self.assertRaises(ContractValidationError):
            G3Adapter(transport, snapshot_schema=self.snapshot_schema).query(
                invocation(Phase.QUERY)
            )

    def test_runtime_bundle_cannot_smuggle_projection(self) -> None:
        transport = RecordingTransport()
        transport.runtime_snapshot["projection"] = {
            "availability": "observed",
            "authoritative": False,
            "order_state": "Settled",
            "settlement_record_count": 1,
        }
        with self.assertRaises(ContractValidationError):
            G3Adapter(transport).query(invocation(Phase.QUERY))

    def test_projection_authority_or_normalized_fields_are_blocking(self) -> None:
        claiming_authority = {
            "availability": "observed",
            "authoritative": True,
            "order_state": "Settled",
            "settlement_record_count": 1,
        }
        with self.assertRaises(ContractValidationError):
            G3Adapter(RecordingTransport(projection=claiming_authority)).query(
                invocation(Phase.QUERY)
            )

        carrying_owner = {
            "availability": "observed",
            "authoritative": False,
            "order_state": "Settled",
            "settlement_record_count": 1,
            "owner": "0x" + "44" * 32,
        }
        with self.assertRaises(ContractValidationError):
            G3Adapter(RecordingTransport(projection=carrying_owner)).query(
                invocation(Phase.QUERY)
            )

    def test_stable_common_error_is_preserved(self) -> None:
        transport = RecordingTransport(
            native_error="RightInvalid",
            common_error_code="RightInvalid",
        )
        result = G3Adapter(transport).create_order(invocation(Phase.CREATE_ORDER))
        self.assertFalse(result["dispatch_success"])
        self.assertEqual(result["native_error"], "RightInvalid")
        self.assertEqual(result["common_error_code"], "RightInvalid")
        self.assertEqual(normalize_common_error(22), "RightInvalid")

    def test_unknown_or_synthetic_error_is_blocking(self) -> None:
        self.assertEqual(len(COMMON_ERROR_CODES), 33)
        common = load_json(EXP_ROOT / "configs/baselines/phase3-common.json")
        self.assertEqual(
            list(COMMON_ERROR_BY_STATUS.values()),
            common["common_error_codes"],
        )
        with self.assertRaises(ContractValidationError):
            G3Adapter(
                RecordingTransport(
                    native_error="SyntheticTimeout",
                    common_error_code="SyntheticTimeout",
                )
            ).settle(invocation(Phase.SETTLE))

    def test_scenario_mapping_exactly_matches_frozen_42_ids_and_masks(self) -> None:
        source = Path(
            "/home/gz785/code/assetxchain-g3-v2/pallets/dataassets/src/tests/invariant_scenarios.rs"
        ).read_text(encoding="utf-8")
        catalog = source.split("const SCENARIOS:", 1)[1].split("fn run_scenario", 1)[0]
        source_ids = set(re.findall(r'id: "([A-Z0-9-]+)"', catalog))
        matrix = (EXP_ROOT / "reports/planning/invariant-scenario-matrix.md").read_text(
            encoding="utf-8"
        )
        marker = chr(96)
        frozen_ids = set(
            re.findall(
                marker + r"((?:LEG|NEG|CLEAN|DUP|RACE)-[A-Z0-9-]+-01)" + marker,
                matrix,
            )
        )
        self.assertEqual(len(source_ids), 42)
        self.assertEqual(source_ids, frozen_ids)
        for invariant in range(1, 11):
            self.assertIn(f"I{invariant}", source)
        for query in (
            "Q_META",
            "Q_RIGHT",
            "Q_ORDER",
            "Q_AUTH",
            "Q_PAY",
            "Q_EVID",
            "Q_EVENT",
            "Q_MARKET",
            "Q_ID",
            "Q_ALL",
        ):
            self.assertIn(query, source)
        for metric in (
            "M_ACCEPT",
            "M_REJECT_CLEAN",
            "M_AUDIT",
            "M_ONCE",
            "M_NO_SETTLEMENT",
            "M_ROLLBACK",
        ):
            self.assertIn(metric, source)


if __name__ == "__main__":
    unittest.main()
