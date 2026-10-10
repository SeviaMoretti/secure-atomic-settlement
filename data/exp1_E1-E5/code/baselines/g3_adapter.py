"""G3 adapter for the frozen Phase 3 phase contract.

The adapter owns no business state. Mutations preserve the Phase 2 split:
market actions use the contract relay, expiry uses the Runtime extrinsic, and
all normalized authority is read directly from Runtime. The recoverable
contract projection is queried separately and can only populate the
non-authoritative projection field.
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


_COMMON_ERROR_ORDER = (
    "UnsupportedProtocolVersion",
    "MalformedCommitment",
    "DomainMismatch",
    "MarketNotRegistered",
    "MarketRestricted",
    "CapabilityDenied",
    "CapacityExceeded",
    "AuthorizationMissing",
    "AuthorizationExpired",
    "AuthorizationRevoked",
    "QuotaExceeded",
    "InvalidSignature",
    "NonceUsed",
    "StateVersionMismatch",
    "OrderNotFound",
    "OrderNotOpen",
    "OrderNotFunded",
    "OrderExpired",
    "AlreadySettled",
    "TerminalOrder",
    "AssetNotTradable",
    "RightInvalid",
    "OwnershipMismatch",
    "ParentMismatch",
    "MutualExclusionConflict",
    "BelowPriceFloor",
    "FeeOutOfBounds",
    "InsufficientBalance",
    "EscrowMismatch",
    "PayerBuyerMismatch",
    "RefundNotAllowed",
    "GracePeriodActive",
    "RefundBeneficiaryMismatch",
)
COMMON_ERROR_CODES = frozenset(_COMMON_ERROR_ORDER)
COMMON_ERROR_BY_STATUS = dict(enumerate(_COMMON_ERROR_ORDER, start=1))

RUNTIME_QUERIES = (
    "q_meta",
    "q_right",
    "q_order",
    "q_authorization",
    "q_payment",
    "q_evidence",
    "q_event",
    "q_market",
    "q_identity",
    "q_all",
)

_PROJECTION_FIELDS = {
    "availability",
    "authoritative",
    "order_state",
    "settlement_record_count",
}


class G3Transport(Protocol):
    def request(self, request: Mapping[str, Any]) -> Mapping[str, Any]:
        """Execute one request against the G3 contract/Runtime bridge."""


class SubprocessG3Transport:
    """JSON stdin/stdout transport for a real G3 contract/Runtime bridge."""

    def __init__(self, command: Sequence[str], *, cwd: Path | None = None) -> None:
        if not command:
            raise ValueError("G3 bridge command must not be empty")
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
                f"G3 bridge exited {completed.returncode}: {completed.stderr.strip()}"
            )
        try:
            response = json.loads(completed.stdout)
        except json.JSONDecodeError as error:
            raise ContractValidationError("G3 bridge returned invalid JSON") from error
        if not isinstance(response, dict):
            raise ContractValidationError("G3 bridge response must be an object")
        return response


def _validated_projection(response: Mapping[str, Any]) -> dict[str, Any]:
    if set(response) != _PROJECTION_FIELDS:
        raise ContractValidationError(
            "G3 projection response must contain only architecture projection fields"
        )
    availability = response.get("availability")
    if availability not in {"observed", "unavailable"}:
        raise ContractValidationError("G3 projection availability is invalid")
    if response.get("authoritative") is not False:
        raise ContractValidationError("G3 contract projection cannot claim authority")
    order_state = response.get("order_state")
    settlement_count = response.get("settlement_record_count")
    if order_state is not None and not isinstance(order_state, str):
        raise ContractValidationError("G3 projection order_state must be a string or null")
    if settlement_count is not None and (
        not isinstance(settlement_count, int)
        or isinstance(settlement_count, bool)
        or settlement_count < 0
    ):
        raise ContractValidationError(
            "G3 projection settlement_record_count must be a non-negative integer or null"
        )
    if availability == "unavailable" and (
        order_state is not None or settlement_count is not None
    ):
        raise ContractValidationError(
            "unavailable G3 projection must not contain projected state"
        )
    return dict(response)


def normalize_common_error(value: Any) -> str | None:
    if value is None:
        return None
    if isinstance(value, bool):
        raise ContractValidationError("G3 common error must not be boolean")
    if isinstance(value, int):
        try:
            return COMMON_ERROR_BY_STATUS[value]
        except KeyError as error:
            raise ContractValidationError(
                f"G3 returned unknown CE v2 status {value}"
            ) from error
    if isinstance(value, str) and value in COMMON_ERROR_CODES:
        return value
    raise ContractValidationError(f"G3 returned unfrozen common error {value!r}")


class G3Adapter(BaselineAdapter):
    """Eight-phase adapter preserving G3 contract/Runtime ownership."""

    _CONTRACT_RELAYS = {
        Phase.CREATE_ORDER: "relay_create_order",
        Phase.FUND_ORDER: "relay_fund_order",
        Phase.SETTLE: "relay_settle",
        Phase.CANCEL_OPEN: "relay_cancel_open",
        Phase.REFUND_FUNDED: "relay_refund_funded",
    }

    def __init__(
        self,
        transport: G3Transport,
        *,
        snapshot_schema: Mapping[str, Any] | None = None,
    ) -> None:
        self._transport = transport
        self._snapshot_schema = snapshot_schema

    @property
    def baseline_id(self) -> str:
        return "G3"

    def _snapshot(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        query = dict(invocation.payload.get("query", {}))
        runtime_response = self._transport.request(
            {
                "kind": "runtime_query_bundle",
                "baseline_id": "G3",
                "scenario_id": invocation.scenario_id,
                "attempt_id": invocation.attempt_id,
                "query": query,
                "queries": list(RUNTIME_QUERIES),
                "event_source": "system-events",
            }
        )
        runtime_snapshot = runtime_response.get("snapshot")
        if not isinstance(runtime_snapshot, dict):
            raise ContractValidationError(
                "G3 Runtime query bundle omitted normalized snapshot"
            )
        if (
            runtime_snapshot.get("baseline_id") != "G3"
            or runtime_snapshot.get("authority_source") != "runtime"
        ):
            raise ContractValidationError(
                "G3 normalized snapshot did not read Runtime authority"
            )
        if "projection" in runtime_snapshot:
            raise ContractValidationError(
                "G3 Runtime query bundle must not supply contract projection"
            )

        projection_response = self._transport.request(
            {
                "kind": "contract_projection_query",
                "baseline_id": "G3",
                "scenario_id": invocation.scenario_id,
                "attempt_id": invocation.attempt_id,
                "query": query,
                "message": "get_projection",
            }
        )
        snapshot = dict(runtime_snapshot)
        snapshot["projection"] = _validated_projection(projection_response)
        if self._snapshot_schema is not None:
            validate_instance(snapshot, self._snapshot_schema)
        return snapshot

    def _receipt_result(
        self,
        invocation: PhaseInvocation,
        *,
        pre: Mapping[str, Any],
        response: Mapping[str, Any],
        expected_error_source: str,
    ) -> Mapping[str, Any]:
        success = response.get("dispatch_success")
        if not isinstance(success, bool):
            raise ContractValidationError("G3 receipt omitted dispatch_success")
        native_error = response.get("native_error")
        if native_error is not None and not isinstance(native_error, str):
            raise ContractValidationError("G3 native_error must be a string or null")
        common_error = normalize_common_error(response.get("common_error_code"))
        if success and (native_error is not None or common_error is not None):
            raise ContractValidationError("successful G3 receipt contains an error")
        if not success:
            if not native_error or common_error is None:
                raise ContractValidationError(
                    "rejected G3 receipt requires native and frozen common errors"
                )
            if response.get("error_source") != expected_error_source:
                raise ContractValidationError(
                    "rejected G3 receipt has an invalid error source"
                )
        post = self._snapshot(invocation)
        events = response.get("events", [])
        if not isinstance(events, list):
            raise ContractValidationError("G3 receipt events must be an array")
        return {
            "baseline_id": "G3",
            "phase": invocation.phase.value,
            "dispatch_success": success,
            "native_error": native_error,
            "common_error_code": common_error,
            "pre_snapshot": pre,
            "post_snapshot": post,
            "events": events,
            "receipt": response.get("receipt"),
        }

    def _contract_mutation(
        self,
        invocation: PhaseInvocation,
        *,
        message: str,
    ) -> Mapping[str, Any]:
        pre = self._snapshot(invocation)
        response = self._transport.request(
            {
                "kind": "contract_relay",
                "baseline_id": "G3",
                "scenario_id": invocation.scenario_id,
                "attempt_id": invocation.attempt_id,
                "message": message,
                "args": dict(invocation.payload.get("args", {})),
                "actor_role": invocation.actor_role,
            }
        )
        return self._receipt_result(
            invocation,
            pre=pre,
            response=response,
            expected_error_source="chain-extension-v2",
        )

    def setup(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        calls = invocation.payload.get("calls")
        if not isinstance(calls, list):
            raise ContractValidationError("G3 setup requires an ordered calls array")
        response = self._transport.request(
            {
                "kind": "setup",
                "baseline_id": "G3",
                "scenario_id": invocation.scenario_id,
                "attempt_id": invocation.attempt_id,
                "calls": calls,
                "market_logic_path": "contract",
                "authority_path": "runtime",
            }
        )
        if response.get("dispatch_success") is not True:
            raise ContractValidationError(
                f"G3 setup failed: {response.get('native_error')}"
            )
        if response.get("native_error") is not None or response.get("common_error_code") is not None:
            raise ContractValidationError("successful G3 setup contains an error")
        events = response.get("events", [])
        if not isinstance(events, list):
            raise ContractValidationError("G3 setup events must be an array")
        return {
            "baseline_id": "G3",
            "phase": Phase.SETUP.value,
            "dispatch_success": True,
            "native_error": None,
            "common_error_code": None,
            "pre_snapshot": None,
            "post_snapshot": self._snapshot(invocation),
            "events": events,
            "receipt": response.get("receipt"),
        }

    def create_order(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._contract_mutation(
            invocation, message=self._CONTRACT_RELAYS[Phase.CREATE_ORDER]
        )

    def fund_order(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._contract_mutation(
            invocation, message=self._CONTRACT_RELAYS[Phase.FUND_ORDER]
        )

    def settle(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._contract_mutation(
            invocation, message=self._CONTRACT_RELAYS[Phase.SETTLE]
        )

    def cancel_open(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._contract_mutation(
            invocation, message=self._CONTRACT_RELAYS[Phase.CANCEL_OPEN]
        )

    def expire_open(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        pre = self._snapshot(invocation)
        response = self._transport.request(
            {
                "kind": "runtime_extrinsic",
                "baseline_id": "G3",
                "scenario_id": invocation.scenario_id,
                "attempt_id": invocation.attempt_id,
                "call": "expire_open",
                "args": dict(invocation.payload.get("args", {})),
                "actor_role": invocation.actor_role,
            }
        )
        return self._receipt_result(
            invocation,
            pre=pre,
            response=response,
            expected_error_source="runtime-dispatch",
        )

    def refund_funded(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        return self._contract_mutation(
            invocation, message=self._CONTRACT_RELAYS[Phase.REFUND_FUNDED]
        )

    def query(self, invocation: PhaseInvocation) -> Mapping[str, Any]:
        snapshot = self._snapshot(invocation)
        return {
            "baseline_id": "G3",
            "phase": Phase.QUERY.value,
            "dispatch_success": True,
            "native_error": None,
            "common_error_code": None,
            "pre_snapshot": snapshot,
            "post_snapshot": snapshot,
            "events": [],
            "receipt": None,
        }
