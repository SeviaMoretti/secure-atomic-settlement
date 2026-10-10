from __future__ import annotations

import sys
import unittest
from pathlib import Path


EXP_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(EXP_ROOT / "scripts/common"))
sys.path.insert(0, str(EXP_ROOT / "scripts/baselines"))

from b2_adapter import (  # noqa: E402
    B2Adapter,
    ContractValidationError,
    NATIVE_ERROR_TO_COMMON,
    common_error_for_native,
)
from phase3_contract import Operation, Phase, PhaseInvocation  # noqa: E402


class RecordingTransport:
    def __init__(self, *, native_error: str | None = None) -> None:
        self.requests: list[dict] = []
        self.native_error = native_error

    def request(self, request: dict) -> dict:
        self.requests.append(request)
        if request["kind"] == "query_bundle":
            return {
                "snapshot": {
                    "baseline_id": "B2",
                    "authority_source": "runtime",
                    "sequence": len(self.requests),
                }
            }
        return {
            "dispatch_success": self.native_error is None,
            "native_error": self.native_error,
            "events": ["OrderCreated"] if self.native_error is None else [],
            "receipt": {"block_number": 1},
        }


def invocation(phase: Phase) -> PhaseInvocation:
    return PhaseInvocation(
        run_id="adapter-test-only",
        baseline_id="B2",
        scenario_id="LEG-META-01",
        attempt_id=phase.value,
        repetition=0,
        phase=phase,
        operation=None if phase is Phase.SETUP else Operation.TRANSFER_META,
        actor_role="OWNER",
        expected_outcome="accept",
        expected_error_code=None,
        payload={
            "calls": [{"call": "register_meta", "args": {}}],
            "args": {"order_id": "0x" + "11" * 32},
            "query": {"order_id": "0x" + "11" * 32},
        },
    )


class B2AdapterTests(unittest.TestCase):
    def test_routes_all_eight_phases_without_business_state(self) -> None:
        transport = RecordingTransport()
        adapter = B2Adapter(transport)
        for phase in Phase:
            result = adapter.invoke(invocation(phase))
            self.assertEqual(result["baseline_id"], "B2")
            self.assertEqual(result["phase"], phase.value)
            self.assertTrue(result["dispatch_success"])
        calls = [
            request["call"]
            for request in transport.requests
            if request["kind"] == "runtime_extrinsic"
        ]
        self.assertEqual(
            calls,
            [
                "create_order",
                "fund_order",
                "settle",
                "cancel_open",
                "expire_open",
                "refund_funded",
            ],
        )
        self.assertTrue(
            all(
                request.get("queries", [])[0:1] == ["q_meta"]
                and request.get("event_source") == "system-events"
                for request in transport.requests
                if request["kind"] == "query_bundle"
            )
        )

    def test_rejection_uses_frozen_mapping_and_preserves_native_error(self) -> None:
        transport = RecordingTransport(native_error="RightNotFound")
        result = B2Adapter(transport).create_order(invocation(Phase.CREATE_ORDER))
        self.assertFalse(result["dispatch_success"])
        self.assertEqual(result["native_error"], "RightNotFound")
        self.assertEqual(result["common_error_code"], "RightInvalid")

    def test_unknown_native_error_is_blocking(self) -> None:
        with self.assertRaises(ContractValidationError):
            common_error_for_native("SyntheticTimeout")

    def test_mapping_exactly_covers_rust_error_vocabulary(self) -> None:
        lib = EXP_ROOT.parent / "assetxchain-b2/pallets/market_runtime/src/lib.rs"
        if not lib.exists():
            lib = Path("/home/gz785/code/assetxchain-b2/pallets/market_runtime/src/lib.rs")
        source = lib.read_text(encoding="utf-8")
        error_body = source.split("pub enum Error<T> {", 1)[1].split("}", 1)[0]
        native_errors = {
            line.strip().rstrip(",")
            for line in error_body.splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        }
        self.assertEqual(native_errors, set(NATIVE_ERROR_TO_COMMON))
        for native_error in native_errors:
            self.assertIsNotNone(common_error_for_native(native_error), native_error)


if __name__ == "__main__":
    unittest.main()
