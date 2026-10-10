"""B1 Contract-only adapter for the frozen Phase 3 phase contract.

The adapter owns no business state. Every mutation and Q_* observation is
delegated to a bridge that talks to the actual B1 ink! contract.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import Any, Mapping, Protocol, Sequence

COMMON_DIR = Path(__file__).resolve().parents[1] / "common"
if str(COMMON_DIR) not in sys.path:
    sys.path.insert(0, str(COMMON_DIR))

from phase3_contract import (
    BaselineAdapter,
    ContractValidationError,
    Phase,
    PhaseInvocation,
    validate_instance,
)


class B1ContractTransport(Protocol):
    def request(self, request: Mapping[str, Any]) -> Mapping[str, Any]:
        """Execute one request against the authoritative B1 contract."""


class SubprocessB1ContractTransport:
    """JSON stdin/stdout transport for a real ink!/RPC bridge process."""

    def __init__(self, command: Sequence[str], *, cwd: Path | None = None) -> None:
        if not command:
            raise ValueError("B1 bridge command must not be empty")
        self._command = tuple(command)
        self._cwd = cwd

    def request(self, request: Mapping[str, Any]) -> Mapping[str, Any]:
        completed = subprocess.run(
            self._command,
            cwd=self._cwd,
            input=json.dumps(request, separators=(",", ":")),
            text=True,
            capture_output=True,
            check=False,
        )
        if completed.returncode != 0:
            raise ContractValidationError(
                f"B1 bridge exited {completed.returncode}: {completed.stderr.strip()}"
            )
        try:
            response = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise ContractValidationError("B1 bridge returned invalid JSON") from error
        if not isinstance(response, dict):
            raise ContractValidationError("B1 bridge response must be an object")
        return response


NATIVE_ERROR_TO_COMMON = {
    "UnsupportedProtocolVersion": "UnsupportedProtocolVersion",
    "MalformedCommitment": "MalformedCommitment",
    "MarketNotRegistered": "MarketNotRegistered",
    "MarketAlreadyRegistered": "MarketNotRegistered",
    "MarketRestricted": "MarketRestricted",
    "MarketHasActiveObligations": "MarketRestricted",
    "CapabilityDenied": "CapabilityDenied",
    "CapacityExceeded": "CapacityExceeded",
    "AuthorizationMissing": "AuthorizationMissing",
    "AuthorizationConsumed": "AuthorizationMissing",
    "AuthorizationAlreadyActive": "AuthorizationMissing",
    "AuthorizationNotActive": "AuthorizationMissing",
    "AuthorizationExpired": "AuthorizationExpired",
    "AuthorizationRevoked": "AuthorizationRevoked",
    "QuotaExceeded": "QuotaExceeded",
    "QuotaReservationMissing": "QuotaExceeded",
    "InvalidSignature": "InvalidSignature",
    "NonceUsed": "NonceUsed",
    "AuthorizationNonceMismatch": "NonceUsed",
    "StateVersionMismatch": "StateVersionMismatch",
    "OrderNotFound": "OrderNotFound",
    "OrderNotOpen": "OrderNotOpen",
    "OrderNotFunded": "OrderNotFunded",
    "OrderExpired": "OrderExpired",
    "AlreadySettled": "AlreadySettled",
    "TerminalOrder": "TerminalOrder",
    "AssetNotTradable": "AssetNotTradable",
    "AssetNotFound": "AssetNotTradable",
    "AssetAlreadyExists": "AssetNotTradable",
    "RightInvalid": "RightInvalid",
    "RightNotFound": "RightInvalid",
    "RightAlreadyExists": "RightInvalid",
    "RightIndexInconsistent": "RightInvalid",
    "NotAdmin": "OwnershipMismatch",
    "NotOperator": "OwnershipMismatch",
    "OwnershipMismatch": "OwnershipMismatch",
    "ParentMismatch": "ParentMismatch",
    "MutualExclusionConflict": "MutualExclusionConflict",
    "ActiveOrderConflict": "MutualExclusionConflict",
    "BelowPriceFloor": "BelowPriceFloor",
    "FeeOutOfBounds": "FeeOutOfBounds",
    "InsufficientBalance": "InsufficientBalance",
    "EscrowMismatch": "EscrowMismatch",
    "PaymentTransferFailed": "EscrowMismatch",
    "PayerBuyerMismatch": "PayerBuyerMismatch",
    "RefundNotAllowed": "RefundNotAllowed",
    "GracePeriodActive": "GracePeriodActive",
    "RefundBeneficiaryMismatch": "RefundBeneficiaryMismatch",
}

_MALFORMED_INTERNAL_ERRORS = {
    "InvalidCapabilities",
    "InvalidCapacityTier",
    "InvalidMarketFee",
    "InvalidMarketTransition",
    "StateVersionOverflow",
    "AuthorizationCounterInvariant",
    "AuthorizationNonceOverflow",
    "InvalidAuthorizationExpiry",
    "InvalidPricePolicy",
    "InvalidRightParameters",
    "InvalidIssueQuota",
    "QuotaInvariant",
    "QuotaReservationAlreadyExists",
    "QuotaReservationAuthorizationMismatch",
    "QuotaReservationStateInvalid",
    "ActiveOrderInvariant",
    "ArithmeticOverflow",
}
NATIVE_ERROR_TO_COMMON.update(
    {native: "MalformedCommitment" for native in _MALFORMED_INTERNAL_ERRORS}
)


def common_error_for_native(native_error: str | None) -> str | None:
    if native_error is None:
        return None
    try:
        return NATIVE_ERROR_TO_COMMON[native_error]
    except KeyError as error:
        raise ContractValidationError(
            f"B1 returned unmapped native error {native_error!r}"
        ) from error


class B1Adapter(BaselineAdapter):
    """Eight-phase adapter backed only by authoritative B1 contract calls."""

    _MESSAGES = {
        Phase.CREATE_ORDER: "create_order",
        Phase.FUND_ORDER: "fund_order",
        Phase.SETTLE: "settle",
        Phase.CANCEL_OPEN: "cancel_open",
        Phase.EXPIRE_OPEN: "expire_open",
        Phase.REFUND_FUNDED: "refund_funded",
    }

    def __init__(
        self,
        transport: B1ContractTransport,
        *,
        snapshot_schema: Mapping[str, Any] | None = None,
    ) -> None:
        self._transport = transport
        self._snapshot_schema = snapshot_schema

    @property
    def baseline_id(self) -> str:
        return "B1"

    def _snapshot(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        response = self._transport.request(
            {
                "kind": "query_bundle",
                "baseline_id": "B1",
                "scenario_id": invocation.scenario_id,
                "attempt_id": invocation.attempt_id,
                "query": dict(invocation.payload.get("query", {})),
                "messages": [
                    "q_meta",
                    "q_right",
                    "q_order",
                    "q_authorization",
                    "q_payment",
                    "q_evidence",
                    "q_market",
                    "q_identity",
                    "q_all",
                ],
                "event_source": "transaction-receipt",
            }
        )
        snapshot = response.get("snapshot")
        if not isinstance(snapshot, dict):
            raise ContractValidationError("B1 query bundle omitted normalized snapshot")
        if snapshot.get("baseline_id") != "B1" or snapshot.get("authority_source") != "contract":
            raise ContractValidationError("B1 snapshot did not read contract authority")
        if self._snapshot_schema is not None:
            validate_instance(snapshot, self._snapshot_schema)
        return snapshot

    def _mutate(
        self,
        invocation: PhaseInvocation,
        *,
        message: str,
    ) -> Mapping[str, Any]:
        pre = self._snapshot(invocation)
        response = self._transport.request(
            {
                "kind": "contract_message",
                "baseline_id": "B1",
                "scenario_id": invocation.scenario_id,
                "attempt_id": invocation.attempt_id,
                "message": message,
                "args": dict(invocation.payload.get("args", {})),
                "transferred_value": str(invocation.payload.get("transferred_value", "0")),
                "actor_role": invocation.actor_role,
            }
        )
        success = response.get("dispatch_success")
        if not isinstance(success, bool):
            raise ContractValidationError("B1 receipt omitted dispatch_success")
        native_error = response.get("native_error")
        if native_error is not None and not isinstance(native_error, str):
            raise ContractValidationError("B1 native_error must be a string or null")
        common_error = common_error_for_native(native_error)
        if success and common_error is not None:
            raise ContractValidationError("successful B1 receipt contains an error")
        if not success and common_error is None:
            raise ContractValidationError("rejected B1 receipt contains no mapped error")
        post = self._snapshot(invocation)
        events = response.get("events", [])
        if not isinstance(events, list):
            raise ContractValidationError("B1 receipt events must be an array")
        return {
            "baseline_id": "B1",
            "phase": invocation.phase.value,
            "dispatch_success": success,
            "native_error": native_error,
            "common_error_code": common_error,
            "pre_snapshot": pre,
            "post_snapshot": post,
            "events": events,
            "receipt": response.get("receipt"),
        }

    def setup(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        calls = invocation.payload.get("calls")
        if not isinstance(calls, list):
            raise ContractValidationError("B1 setup requires an ordered calls array")
        response = self._transport.request(
            {
                "kind": "setup",
                "baseline_id": "B1",
                "scenario_id": invocation.scenario_id,
                "attempt_id": invocation.attempt_id,
                "calls": calls,
            }
        )
        if response.get("dispatch_success") is not True:
            native_error = response.get("native_error")
            common_error_for_native(native_error)
            raise ContractValidationError(f"B1 setup failed: {native_error}")
        return {
            "baseline_id": "B1",
            "phase": Phase.SETUP.value,
            "dispatch_success": True,
            "native_error": None,
            "common_error_code": None,
            "pre_snapshot": None,
            "post_snapshot": self._snapshot(invocation),
            "events": response.get("events", []),
            "receipt": response.get("receipt"),
        }

    def create_order(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._mutate(invocation, message=self._MESSAGES[Phase.CREATE_ORDER])

    def fund_order(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._mutate(invocation, message=self._MESSAGES[Phase.FUND_ORDER])

    def settle(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._mutate(invocation, message=self._MESSAGES[Phase.SETTLE])

    def cancel_open(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._mutate(invocation, message=self._MESSAGES[Phase.CANCEL_OPEN])

    def expire_open(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._mutate(invocation, message=self._MESSAGES[Phase.EXPIRE_OPEN])

    def refund_funded(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._mutate(invocation, message=self._MESSAGES[Phase.REFUND_FUNDED])

    def query(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        snapshot = self._snapshot(invocation)
        return {
            "baseline_id": "B1",
            "phase": Phase.QUERY.value,
            "dispatch_success": True,
            "native_error": None,
            "common_error_code": None,
            "pre_snapshot": snapshot,
            "post_snapshot": snapshot,
            "events": [],
            "receipt": None,
        }
