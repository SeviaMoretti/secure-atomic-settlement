"""Recompute formal expected-outcome, Q_ALL, I1-I10, duplicate, and race gates."""

from __future__ import annotations

import argparse
import copy
import json
import os
import re
from pathlib import Path
from typing import Any, Mapping, Sequence

from phase4_contract import (
    BLOCK_EVIDENCE_SCHEMA_PATH,
    EXP_ROOT,
    RESULT_SCHEMA_PATH,
    SHA256_RE,
    SETTLEMENT_RECORD_FIELDS,
    SNAPSHOT_SCHEMA_PATH,
    TOOLING_VERSION,
    Phase4ValidationError,
    load_catalog,
    load_json,
    scenario_map,
    schema_validate,
    validate_preflight,
    write_json_exclusive,
)


COMMON_COST_FIELDS = {
    "latency_ms", "completion_blocks", "partial_fee", "weight_ref_time",
    "weight_proof_size", "storage_writes", "events_count", "rpc_response_ms",
    "phase_call_count",
}
ARCHITECTURE_COST_FIELDS = {
    "contract_gas_or_ref_time", "contract_storage_writes", "runtime_dispatch_count",
    "runtime_storage_writes", "typed_ce_call_count", "projection_storage_writes",
}
ARCHITECTURE_APPLICABLE = {
    "B1": {"contract_gas_or_ref_time", "contract_storage_writes"},
    "B2": {"runtime_dispatch_count", "runtime_storage_writes"},
    "G3": ARCHITECTURE_COST_FIELDS,
}
SUCCESS_EVENTS = {
    "SettlementFinalized", "OrderSettled", "MetaTransferred", "RightIssued",
    "RightTransferred", "AssetTransferred", "TransferMetaSettled",
    "IssueRightSettled", "TransferRightSettled",
}


class CheckFailure:
    def __init__(self) -> None:
        self.rows: list[dict[str, Any]] = []

    def add(self, code: str, message: str, *, scenario_id: str, repetition: int, attempt_id: str) -> None:
        self.rows.append({
            "code": code,
            "scenario_id": scenario_id,
            "repetition": repetition,
            "attempt_id": attempt_id,
            "message": message,
        })


def _equal(actual: Any, expected: Any, code: str, message: str, context: tuple[str, int, str], failures: CheckFailure) -> None:
    if actual != expected:
        failures.add(code, f"{message}: expected={expected!r}, actual={actual!r}", scenario_id=context[0], repetition=context[1], attempt_id=context[2])


def _load_required(path: Path) -> Any:
    if not path.is_file():
        raise Phase4ValidationError(f"required attempt artifact is missing: {path}")
    return load_json(path)


def _phase_extrinsic_index(value: str) -> int | None:
    try:
        decoded = json.loads(value)
    except (json.JSONDecodeError, TypeError):
        decoded = None
    if isinstance(decoded, dict) and isinstance(decoded.get("applyExtrinsic"), int):
        return decoded["applyExtrinsic"]
    match = re.fullmatch(r"ApplyExtrinsic\(([0-9]+)\)", value)
    return int(match.group(1)) if match else None


def _metric_contract(result: Mapping[str, Any], baseline_id: str, context: tuple[str, int, str], failures: CheckFailure) -> None:
    for group, expected in (("cost", COMMON_COST_FIELDS), ("architecture_cost", ARCHITECTURE_COST_FIELDS)):
        value = result[group]
        if set(value) != expected:
            failures.add("metric-contract", f"{group} keys differ", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
            continue
        for name, metric in value.items():
            if set(metric) != {"availability", "value", "unit"}:
                failures.add("metric-contract", f"{group}.{name} fields differ", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
                continue
            availability = metric["availability"]
            measured = availability == "measured"
            if measured != (metric["value"] is not None):
                failures.add("metric-contract", f"{group}.{name} availability/value mismatch", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
            if group == "cost" and availability == "N/A":
                failures.add("metric-contract", f"common metric {name} cannot be N/A", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
            if group == "architecture_cost":
                applicable = name in ARCHITECTURE_APPLICABLE[baseline_id]
                if applicable and availability == "N/A":
                    failures.add("metric-contract", f"applicable metric {name} is N/A", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
                if not applicable and (availability != "N/A" or metric["value"] is not None):
                    failures.add("metric-contract", f"inapplicable metric {name} must be N/A/null", scenario_id=context[0], repetition=context[1], attempt_id=context[2])


def _normalized_reject_snapshot(
    snapshot: Mapping[str, Any],
    payer_role: str | None,
    fee: str,
    environmental_adjustments: Sequence[Mapping[str, Any]],
    snapshot_seller_role: str,
    snapshot_buyer_role: str,
    *,
    post: bool,
) -> dict[str, Any]:
    value = copy.deepcopy(snapshot)
    value.pop("observed_at", None)
    value.pop("state_commitment", None)
    if post:
        reward_fields = {
            snapshot_seller_role: "seller_balance",
            snapshot_buyer_role: "buyer_balance",
            "MARKET_OK": "market_balance",
        }
        for adjustment in environmental_adjustments:
            field = reward_fields.get(adjustment["role"])
            if field and value["payment"][field] is not None:
                value["payment"][field] = str(
                    int(value["payment"][field]) - int(adjustment["amount"])
                )
        if int(fee):
            field = (
                "seller_balance" if payer_role == snapshot_seller_role
                else "buyer_balance" if payer_role == snapshot_buyer_role
                else "market_balance" if payer_role in {"MARKET_OK", "MARKET_DENIED"}
                else None
            )
            if field and value["payment"][field] is not None:
                value["payment"][field] = str(int(value["payment"][field]) + int(fee))
    return value


def _snapshot_balance_roles(result: Mapping[str, Any]) -> tuple[str, str]:
    operation = result["request"]["operation"]
    seller_role = "RIGHT_SELLER" if operation == "TransferRight" else "OWNER"
    buyer_role = "BUYER_2" if operation == "TransferRight" else "BUYER_1"
    scenario_id = result["request"]["scenario_id"]
    attempt_id = result["request"]["attempt_id"]
    if (scenario_id, attempt_id) in {
        ("RACE-BUYER-01", "buyer-2"),
        ("RACE-QUOTA-01", "fund-2"),
    }:
        buyer_role = "BUYER_2"
    return seller_role, buyer_role


def _check_snapshot_authority(snapshot: Mapping[str, Any], baseline_id: str, context: tuple[str, int, str], failures: CheckFailure) -> None:
    expected = "contract" if baseline_id == "B1" else "runtime"
    _equal(snapshot["baseline_id"], baseline_id, "authority", "snapshot baseline", context, failures)
    _equal(snapshot["authority_source"], expected, "authority", "authoritative source", context, failures)
    projection = snapshot["projection"]
    if projection["authoritative"] is not False:
        failures.add("authority", "contract projection claimed authority", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if baseline_id in {"B1", "B2"} and projection != {"availability": "N/A", "authoritative": False, "order_state": None, "settlement_record_count": None}:
        failures.add("authority", "B1/B2 projection must be structural N/A", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if baseline_id == "G3" and projection["availability"] == "N/A":
        failures.add("authority", "G3 projection applicability was hidden as N/A", scenario_id=context[0], repetition=context[1], attempt_id=context[2])


def _check_rejection(result: Mapping[str, Any], context: tuple[str, int, str], failures: CheckFailure) -> None:
    outcome = result["outcome"]
    pre = result["snapshots"]["pre"]
    post = result["snapshots"]["post"]
    adjustments = outcome["environmental_balance_adjustments"]
    seller_role, buyer_role = _snapshot_balance_roles(result)
    for adjustment in adjustments:
        if adjustment["block_number"] != outcome["included_block"]:
            failures.add("balance-adjustment", "environmental adjustment block differs from included block", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    normalized_pre = _normalized_reject_snapshot(
        pre, outcome["dispatch_fee_payer_role"], outcome["dispatch_fee_amount"],
        adjustments, seller_role, buyer_role, post=False
    )
    normalized_post = _normalized_reject_snapshot(
        post, outcome["dispatch_fee_payer_role"], outcome["dispatch_fee_amount"],
        adjustments, seller_role, buyer_role, post=True
    )
    if normalized_pre != normalized_post:
        failures.add("dirty-state", "rejected attempt changed Q_ALL after recorded dispatch fee normalization", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if pre["state_commitment"] != post["state_commitment"]:
        failures.add("dirty-state", "rejected attempt changed the normalized business-state commitment", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if result["events"]["success_event_count"] != 0 or SUCCESS_EVENTS.intersection(result["events"]["event_names"]):
        failures.add("event-mismatch", "rejected attempt emitted a success event", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if post["evidence"]["settlement_record_count"] != pre["evidence"]["settlement_record_count"]:
        failures.add("dirty-state", "rejected attempt changed settlement evidence", scenario_id=context[0], repetition=context[1], attempt_id=context[2])


def _integer(record: Mapping[str, Any], field: str, context: tuple[str, int, str], failures: CheckFailure) -> int:
    try:
        return int(record[field])
    except (KeyError, TypeError, ValueError):
        failures.add("I7", f"settlement field {field} is not an integer", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
        return 0


def _settlement_fixture(preflight: Mapping[str, Any] | None = None) -> Mapping[str, Any]:
    relative = (
        preflight["inputs"]["environment_config"]["path"]
        if preflight is not None else "configs/dev.json"
    )
    environment = load_json(EXP_ROOT / relative)
    return environment["shared_fixture"]


def _object_versions(
    pre: Mapping[str, Any], post: Mapping[str, Any], operation: str
) -> tuple[int, int]:
    if operation == "TransferMeta":
        return pre["meta"]["state_version"], post["meta"]["state_version"]
    if operation == "IssueRight":
        pre_version = pre["right"]["state_version"] if pre["right"]["exists"] else 0
        return pre_version, post["right"]["state_version"]
    return pre["right"]["state_version"], post["right"]["state_version"]


def _check_settlement_event(
    result: Mapping[str, Any],
    record: Mapping[str, Any],
    block_evidence: Mapping[str, Any] | None,
    context: tuple[str, int, str],
    failures: CheckFailure,
) -> None:
    events = result["events"]
    post = result["snapshots"]["post"]
    _equal(
        events["post_state_commitment"], post["state_commitment"],
        "event-mismatch", "event/post snapshot commitment", context, failures,
    )
    if events["event_names"].count("SettlementFinalized") != 1:
        failures.add(
            "event-mismatch", "accepted settlement lacks exactly one SettlementFinalized event",
            scenario_id=context[0], repetition=context[1], attempt_id=context[2],
        )
    if block_evidence is None:
        failures.add(
            "event-mismatch", "accepted settlement lacks block event evidence",
            scenario_id=context[0], repetition=context[1], attempt_id=context[2],
        )
        return
    event_index = result["outcome"]["event_index"]
    raw_events = block_evidence["events"]
    if not isinstance(event_index, int) or event_index < 0 or event_index >= len(raw_events):
        failures.add(
            "event-mismatch", "SettlementFinalized event index is absent or out of range",
            scenario_id=context[0], repetition=context[1], attempt_id=context[2],
        )
        return
    if event_index not in block_evidence["transaction_event_indexes"]:
        failures.add(
            "event-mismatch", "SettlementFinalized event index is outside the settlement extrinsic",
            scenario_id=context[0], repetition=context[1], attempt_id=context[2],
        )
    raw_event = raw_events[event_index]
    settlement_id = record["settlement_id"]
    order_id = record["order_id"]
    post_commitment = post["state_commitment"]
    baseline_id = post["baseline_id"]
    if baseline_id == "B1":
        _equal(raw_event["section"], "contracts", "event-mismatch", "B1 settlement event section", context, failures)
        _equal(raw_event["method"], "ContractEmitted", "event-mismatch", "B1 settlement event method", context, failures)
        expected_payload = "0x" + settlement_id[2:] + order_id[2:] + post_commitment[2:]
        payload = raw_event["data"][-1] if raw_event["data"] else None
        _equal(payload, expected_payload, "event-mismatch", "B1 SettlementFinalized payload", context, failures)
    else:
        expected_section = "marketRuntime" if baseline_id == "B2" else "dataAssets"
        _equal(raw_event["section"], expected_section, "event-mismatch", "Runtime settlement event section", context, failures)
        _equal(raw_event["method"], "SettlementFinalized", "event-mismatch", "Runtime settlement event method", context, failures)
        _equal(
            raw_event["data"][:3], [settlement_id, order_id, post_commitment],
            "event-mismatch", "Runtime SettlementFinalized payload", context, failures,
        )


def _check_settlement(
    result: Mapping[str, Any],
    operation: str,
    context: tuple[str, int, str],
    failures: CheckFailure,
    *,
    fixture: Mapping[str, Any] | None = None,
    block_evidence: Mapping[str, Any] | None = None,
) -> None:
    pre = result["snapshots"]["pre"]
    post = result["snapshots"]["post"]
    record = result["settlement"]["record"]
    if not isinstance(record, dict) or set(record) != set(SETTLEMENT_RECORD_FIELDS):
        failures.add("I7", "accepted settlement lacks the exact 24-field SettlementRecord", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
        return
    fixture = fixture or _settlement_fixture()
    transactions = fixture["transactions"]
    gross = int(pre["payment"]["escrow_amount"])
    royalty_bps = int({
        "TransferMeta": transactions["meta_sale_royalty_bps"],
        "IssueRight": transactions["right_issue_royalty_bps"],
        "TransferRight": transactions["right_resale_royalty_bps"],
    }[operation])
    royalty_amount = gross * royalty_bps // 10_000
    market_fee_amount = gross * int(transactions["market_fee_bps"]) // 10_000
    safety_fee_amount = gross * int(transactions["safety_fee_bps"]) // 10_000
    seller_amount = gross - royalty_amount - market_fee_amount - safety_fee_amount
    pre_version, post_version = _object_versions(pre, post, operation)
    right_id = None if operation == "TransferMeta" else post["right"]["right_id"]
    expected_pairs = {
        "settlement_id": result["settlement"]["settlement_id"],
        "order_id": post["order"]["order_id"],
        "operation": operation,
        "asset_id": post["meta"]["asset_id"],
        "right_id": right_id,
        "authorization_id": post["authorization"]["authorization_id"],
        "market_id": post["market"]["market_id"],
        "policy_hash": post["authorization"]["policy_hash"],
        "order_commitment_hash": post["identity"]["order_commitment_hash"],
        "seller": post["order"]["seller"],
        "buyer": post["order"]["buyer"],
        "currency": fixture["funding"]["currency"].capitalize(),
        "gross_price": str(gross),
        "seller_amount": str(seller_amount),
        "royalty_recipient": pre["meta"]["owner"] if royalty_amount else None,
        "royalty_amount": str(royalty_amount),
        "market_fee_recipient": fixture["roles"]["MARKET_OK"]["account_id_hex"],
        "market_fee_amount": str(market_fee_amount),
        "safety_fee_amount": str(safety_fee_amount),
        "pre_state_version": pre_version,
        "post_state_version": post_version,
        "pre_state_commitment": pre["state_commitment"],
        "block_number": post["observed_at"]["block_number"],
        "settlement_protocol_version": post["identity"]["settlement_protocol_version"],
    }
    if set(expected_pairs) != set(SETTLEMENT_RECORD_FIELDS):
        raise Phase4ValidationError("internal checker error: SettlementRecord expectations are incomplete")
    for field, expected in expected_pairs.items():
        _equal(record[field], expected, "I7", f"SettlementRecord.{field}", context, failures)
    record_gross = _integer(record, "gross_price", context, failures)
    components = sum(_integer(record, field, context, failures) for field in ("seller_amount", "royalty_amount", "market_fee_amount", "safety_fee_amount"))
    if record_gross != components:
        failures.add("I7", f"settlement amounts do not sum: gross={record_gross}, components={components}", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    _equal(post["evidence"]["settlement_id"], record["settlement_id"], "I7", "record/post evidence settlement ID", context, failures)
    _equal(post["evidence"]["pre_state_commitment"], record["pre_state_commitment"], "I7", "record/post evidence pre-state commitment", context, failures)
    _equal(result["settlement"]["settlement_record_count"], post["evidence"]["settlement_record_count"], "I7", "result/post evidence settlement count", context, failures)
    record_hash = post["evidence"]["record_fields_hash"]
    if not isinstance(record_hash, str) or re.fullmatch(r"0x[0-9a-f]{64}", record_hash) is None or int(record_hash[2:], 16) == 0:
        failures.add("I7", "authoritative record-fields hash is missing or zero", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    _check_settlement_event(result, record, block_evidence, context, failures)


def _check_accept(
    result: Mapping[str, Any],
    scenario: Mapping[str, Any],
    context: tuple[str, int, str],
    failures: CheckFailure,
    *,
    fixture: Mapping[str, Any] | None = None,
    block_evidence: Mapping[str, Any] | None = None,
) -> None:
    phase = result["request"]["phase"]
    operation = scenario["operation"]
    pre = result["snapshots"]["pre"]
    post = result["snapshots"]["post"]
    if phase != "settle" and (
        post["evidence"]["settlement_record_count"] != pre["evidence"]["settlement_record_count"]
        or result["settlement"]["record"] is not None
    ):
        failures.add("I3", "non-settlement success created settlement evidence", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if phase == "create_order":
        _equal(post["order"]["state"], "Open", "state-machine", "create post-state", context, failures)
    elif phase == "fund_order":
        _equal(post["order"]["state"], "Funded", "state-machine", "fund post-state", context, failures)
        _equal(post["payment"]["state"], "Escrowed", "state-machine", "fund payment state", context, failures)
    elif phase == "cancel_open":
        _equal(post["order"]["state"], "Cancelled", "state-machine", "cancel post-state", context, failures)
        _equal(post["evidence"]["settlement_record_count"], pre["evidence"]["settlement_record_count"], "I3", "cancel settlement count", context, failures)
    elif phase == "expire_open":
        _equal(post["order"]["state"], "Expired", "state-machine", "expiry post-state", context, failures)
        _equal(post["evidence"]["settlement_record_count"], pre["evidence"]["settlement_record_count"], "I3", "expiry settlement count", context, failures)
    elif phase == "refund_funded":
        _equal(post["order"]["state"], "Refunded", "state-machine", "refund order state", context, failures)
        _equal(post["payment"]["state"], "Refunded", "state-machine", "refund payment state", context, failures)
        _equal(post["evidence"]["settlement_record_count"], pre["evidence"]["settlement_record_count"], "I3", "refund settlement count", context, failures)
        if result["settlement"]["refund_recipient"] != post["order"]["buyer"] or result["settlement"]["refund_amount"] != pre["payment"]["escrow_amount"]:
            failures.add("refund", "refund recipient/amount differs from authoritative order/escrow", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    elif phase == "settle":
        _equal(post["order"]["state"], "Settled", "state-machine", "settlement order state", context, failures)
        _equal(post["payment"]["state"], "Released", "state-machine", "settlement payment state", context, failures)
        if post["evidence"]["settlement_record_count"] - pre["evidence"]["settlement_record_count"] != 1:
            failures.add("I3", "settlement did not add exactly one record", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
        if result["events"]["success_event_count"] != 1:
            failures.add("event-mismatch", "settlement did not emit exactly one normalized success event", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
        buyer = post["order"]["buyer"]
        holder = post["meta"]["owner"] if operation == "TransferMeta" else post["right"]["holder"]
        _equal(holder, buyer, "I1", "settled owner/holder", context, failures)
        _check_settlement(
            result, operation, context, failures,
            fixture=fixture, block_evidence=block_evidence,
        )
        if operation == "IssueRight":
            if not post["right"]["exists"] or post["right"]["holder"] != buyer:
                failures.add("I10", "successful issuance did not create the buyer right", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
            if post["authorization"]["issued_count"] != pre["authorization"]["issued_count"] + 1 or post["authorization"]["reserved_count"] + 1 != pre["authorization"]["reserved_count"]:
                failures.add("I10", "reserved quota was not atomically converted to issued quota", scenario_id=context[0], repetition=context[1], attempt_id=context[2])


def _actual_outcome(result: Mapping[str, Any]) -> str:
    return result["outcome"]["protocol_outcome"]


def _check_test_gate(path: Path, preflight: Mapping[str, Any], scenario: Mapping[str, Any], repetition: int, failures: CheckFailure) -> None:
    evidence = _load_required(path)
    expected_keys = {"schema_id", "schema_version", "run_id", "baseline_id", "scenario_id", "repetition", "attempt_id", "command", "exit_code", "source_commit", "q_all_equal", "production_failpoint_exposed", "test_output_sha256"}
    context = (scenario["scenario_id"], repetition, "atomic-rollback-test")
    if set(evidence) != expected_keys:
        failures.add("test-gate", "test-gate evidence fields differ", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
        return
    checks = (
        evidence["schema_id"] == "assetxchain-exp2-phase4-test-gate",
        evidence["schema_version"] == 1,
        evidence["run_id"] == preflight["run_id"],
        evidence["baseline_id"] == preflight["baseline"]["baseline_id"],
        evidence["scenario_id"] == scenario["scenario_id"],
        evidence["repetition"] == repetition,
        evidence["exit_code"] == 0,
        evidence["source_commit"] == preflight["baseline"]["head"],
        evidence["q_all_equal"] is True,
        evidence["production_failpoint_exposed"] is False,
        isinstance(evidence["test_output_sha256"], str) and SHA256_RE.fullmatch(evidence["test_output_sha256"]) is not None,
    )
    if not all(checks) or not isinstance(evidence["command"], list) or not evidence["command"]:
        failures.add("test-gate", "atomic rollback test evidence did not pass the frozen gate", scenario_id=context[0], repetition=context[1], attempt_id=context[2])


def _check_block_evidence(
    evidence: Mapping[str, Any],
    result: Mapping[str, Any],
    context: tuple[str, int, str],
    failures: CheckFailure,
) -> None:
    schema_validate(evidence, BLOCK_EVIDENCE_SCHEMA_PATH)
    outcome = result["outcome"]
    events = evidence["events"]
    _equal(evidence["included_block_number"], outcome["included_block"], "block-evidence", "included block", context, failures)
    _equal(evidence["extrinsic_index"], outcome["extrinsic_index"], "block-evidence", "extrinsic index", context, failures)
    _equal(evidence["complete_block_event_count"], len(events), "block-evidence", "complete event count", context, failures)
    _equal([row["event_index"] for row in events], list(range(len(events))), "block-evidence", "event indexes", context, failures)
    _equal(
        evidence["parent_block_number"] + 1,
        evidence["included_block_number"],
        "block-evidence", "parent/included adjacency", context, failures,
    )
    _equal(
        result["snapshots"]["pre"]["observed_at"]["block_number"],
        evidence["parent_block_number"],
        "block-evidence", "pre-state boundary", context, failures,
    )
    _equal(
        result["snapshots"]["post"]["observed_at"]["block_number"],
        evidence["included_block_number"],
        "block-evidence", "post-state boundary", context, failures,
    )
    transaction_indexes = evidence["transaction_event_indexes"]
    for event_index in transaction_indexes:
        if event_index >= len(events):
            failures.add("block-evidence", "transaction event index is outside complete block events", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
            continue
        if _phase_extrinsic_index(events[event_index]["phase"]) != evidence["extrinsic_index"]:
            failures.add("block-evidence", "transaction event phase/extrinsic index differs", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if outcome["event_index"] is not None and outcome["event_index"] not in transaction_indexes:
        failures.add("block-evidence", "normalized event index is absent from transaction events", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    for adjustment in outcome["environmental_balance_adjustments"]:
        event_index = adjustment["event_index"]
        if event_index >= len(events):
            failures.add("balance-adjustment", "environmental adjustment event is absent", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
            continue
        event = events[event_index]
        if (
            event["section"] != "rewards"
            or event["method"] != "RewardPaid"
            or event["phase"] != adjustment["event_phase"]
            or len(event["data"]) < 2
            or event["data"][1] != adjustment["amount"]
        ):
            failures.add("balance-adjustment", "environmental adjustment does not match its block event", scenario_id=context[0], repetition=context[1], attempt_id=context[2])


def _normalized_continuity_snapshot(
    snapshot: Mapping[str, Any], scenario_id: str
) -> dict[str, Any]:
    """Remove observation-only fields before comparing adjacent chain boundaries."""
    value = copy.deepcopy(snapshot)
    value.pop("observed_at", None)
    value.pop("state_commitment", None)
    if scenario_id == "RACE-BUYER-01":
        # Each attempt intentionally observes a different buyer account. The
        # authoritative order buyer and escrow still remain comparable.
        value["payment"].pop("buyer_balance", None)
    return value


def _check_attempt_continuity(
    unit_results: Sequence[Mapping[str, Any]],
    block_evidence: Sequence[Mapping[str, Any]],
    scenario: Mapping[str, Any],
    repetition: int,
    failures: CheckFailure,
) -> None:
    """Ensure inclusion-ordered attempts observe one continuous chain state."""
    for index in range(1, len(unit_results)):
        previous = unit_results[index - 1]
        current = unit_results[index]
        context = (
            scenario["scenario_id"],
            repetition,
            current["request"]["attempt_id"],
        )
        previous_block = block_evidence[index - 1]
        current_block = block_evidence[index]
        if previous_block["included_block_number"] == current_block["included_block_number"]:
            failures.add(
                "continuity",
                "same-block attempts lack an authoritative pre-extrinsic snapshot boundary",
                scenario_id=context[0],
                repetition=context[1],
                attempt_id=context[2],
            )
            continue
        _equal(
            previous_block["included_block_hash"],
            current_block["parent_block_hash"],
            "continuity",
            "adjacent attempts do not share a block boundary",
            context,
            failures,
        )
        if scenario["scenario_id"] in {"RACE-QUOTA-01", "RACE-OBJECT-01"}:
            # These attempts intentionally switch the normalized order/right
            # being observed, so only their chain block boundary is comparable.
            continue
        if _normalized_continuity_snapshot(
            previous["snapshots"]["post"], scenario["scenario_id"]
        ) != _normalized_continuity_snapshot(
            current["snapshots"]["pre"], scenario["scenario_id"]
        ):
            failures.add(
                "continuity",
                "adjacent attempts disagree on normalized business state",
                scenario_id=context[0],
                repetition=context[1],
                attempt_id=context[2],
            )


def _order_attempts_by_inclusion(
    unit_results: Sequence[Mapping[str, Any]],
    block_evidence: Sequence[Mapping[str, Any]],
    scenario: Mapping[str, Any],
    repetition: int,
    failures: CheckFailure,
) -> tuple[list[Mapping[str, Any]], list[Mapping[str, Any]]]:
    """Return attempts in authoritative chain inclusion order."""
    rows = sorted(
        zip(unit_results, block_evidence),
        key=lambda pair: (
            pair[0]["outcome"]["included_block"],
            pair[0]["outcome"]["extrinsic_index"],
        ),
    )
    keys = [
        (result["outcome"]["included_block"], result["outcome"]["extrinsic_index"])
        for result, _ in rows
    ]
    if len(keys) != len(set(keys)):
        failures.add(
            "block-order",
            "attempt inclusion positions are duplicated",
            scenario_id=scenario["scenario_id"],
            repetition=repetition,
            attempt_id="scenario",
        )
    return [result for result, _ in rows], [evidence for _, evidence in rows]


def _check_submission_sequences(
    unit_results: Sequence[Mapping[str, Any]],
    scenario: Mapping[str, Any],
    repetition: int,
    failures: CheckFailure,
) -> None:
    sequences = [item["outcome"]["submission_sequence"] for item in unit_results]
    expected = list(range(1, len(unit_results) + 1))
    if sorted(sequences) != expected:
        failures.add(
            "block-order",
            f"submission sequences must be exactly 1..N: expected={expected}, actual={sorted(sequences)}",
            scenario_id=scenario["scenario_id"],
            repetition=repetition,
            attempt_id="scenario",
        )


def _check_attempt(
    attempt_dir: Path,
    preflight: Mapping[str, Any],
    scenario: Mapping[str, Any],
    expected: Mapping[str, Any],
    repetition: int,
    failures: CheckFailure,
    fixture: Mapping[str, Any],
) -> tuple[Mapping[str, Any], Mapping[str, Any]]:
    result = _load_required(attempt_dir / "result.json")
    invocation = _load_required(attempt_dir / "invocation.json")
    dispatch = _load_required(attempt_dir / "dispatch.json")
    pre_state = _load_required(attempt_dir / "pre-state.json")
    post_state = _load_required(attempt_dir / "post-state.json")
    events = _load_required(attempt_dir / "events.json")
    block_evidence = _load_required(attempt_dir / "block-events.json")
    context = (scenario["scenario_id"], repetition, expected["attempt_id"])
    schema_validate(result, RESULT_SCHEMA_PATH)
    schema_validate(pre_state, SNAPSHOT_SCHEMA_PATH)
    schema_validate(post_state, SNAPSHOT_SCHEMA_PATH)
    _check_snapshot_authority(pre_state, preflight["baseline"]["baseline_id"], context, failures)
    _check_snapshot_authority(post_state, preflight["baseline"]["baseline_id"], context, failures)
    _check_block_evidence(block_evidence, result, context, failures)
    _equal(result["snapshots"]["pre"], pre_state, "cross-reference", "pre-state artifact", context, failures)
    _equal(result["snapshots"]["post"], post_state, "cross-reference", "post-state artifact", context, failures)
    _equal(result["events"], events, "cross-reference", "events artifact", context, failures)
    if (
        result["settlement"]["settlement_id"] is not None
        or (_actual_outcome(result) == "accepted" and result["request"]["phase"] == "settle")
    ):
        _equal(result["settlement"]["settlement_id"], post_state["evidence"]["settlement_id"], "cross-reference", "settlement ID/post evidence", context, failures)
    _equal(result["settlement"]["settlement_record_count"], post_state["evidence"]["settlement_record_count"], "cross-reference", "settlement count/post evidence", context, failures)
    if dispatch != result["outcome"]:
        failures.add("cross-reference", "dispatch artifact differs from normalized outcome", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    required_invocation = {
        "run_id": preflight["run_id"], "baseline_id": preflight["baseline"]["baseline_id"],
        "scenario_id": scenario["scenario_id"], "attempt_id": expected["attempt_id"],
        "repetition": repetition, "phase": expected["phase"], "operation": scenario["operation"],
    }
    for field, value in required_invocation.items():
        if invocation.get(field) != value:
            failures.add("cross-reference", f"invocation {field} mismatch", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    run = result["run"]
    expected_run = {
        "run_id": preflight["run_id"], "baseline_id": preflight["baseline"]["baseline_id"],
        "baseline_name": preflight["baseline"]["baseline_name"], "implementation_commit": preflight["baseline"]["head"],
        "git_dirty": False, "seed": preflight["inputs"]["seed"], "repetition": repetition,
        "validity_level": "provisional-candidate",
    }
    for field, value in expected_run.items():
        _equal(run[field], value, "cross-reference", f"run.{field}", context, failures)
    request = result["request"]
    _equal(request["scenario_id"], scenario["scenario_id"], "cross-reference", "request scenario", context, failures)
    _equal(request["attempt_id"], expected["attempt_id"], "cross-reference", "request attempt", context, failures)
    _equal(request["phase"], expected["phase"], "cross-reference", "request phase", context, failures)
    _equal(request["operation"], scenario["operation"], "cross-reference", "request operation", context, failures)
    _metric_contract(result, preflight["baseline"]["baseline_id"], context, failures)
    actual = _actual_outcome(result)
    if actual == "timeout":
        failures.add("timeout", "timeout cannot satisfy any formal expectation", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    expected_outcome = expected["expected_outcome"]
    matched = (
        (expected_outcome == "accept" and actual == "accepted")
        or (expected_outcome == "reject" and actual == "rejected" and result["outcome"]["common_error_code"] == expected["expected_error_code"])
        or (expected_outcome == "winner-or-loser" and actual in {"accepted", "rejected"})
    )
    if not matched:
        failures.add("expected-outcome", f"expected {expected_outcome}/{expected['expected_error_code']}, got {actual}/{result['outcome']['common_error_code']}", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if expected_outcome == "reject" and actual == "accepted":
        failures.add("false-accept", "invalid request was accepted", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if expected_outcome == "accept" and actual == "rejected":
        failures.add("false-reject", "valid request was rejected", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    if actual == "accepted":
        _check_accept(
            result, scenario, context, failures,
            fixture=fixture, block_evidence=block_evidence,
        )
    elif actual == "rejected":
        if result["outcome"]["common_error_code"] is None:
            failures.add("expected-outcome", "rejection lacks a common error", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
        _check_rejection(result, context, failures)
    reported = result["correctness"]
    if not reported["expected_outcome_match"] or any(
        reported[field] for field in reported if field != "expected_outcome_match"
    ):
        failures.add("reported-correctness", "executor-supplied correctness counters are not a clean match", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
    included = result["outcome"]["included_block"]
    completed = result["outcome"]["completion_block"]
    if (included is None) != (completed is None):
        failures.add("block-order", "included/completion block pairing differs", scenario_id=context[0], repetition=context[1], attempt_id=context[2])
        expected_delta = None
    else:
        expected_delta = None if included is None else completed - included
    _equal(result["outcome"]["completion_blocks"], expected_delta, "block-order", "completion block delta", context, failures)
    return result, block_evidence


def _summary(failures: CheckFailure, completed_attempts: int, completed_units: int) -> dict[str, Any]:
    def count(*codes: str) -> int:
        return sum(row["code"] in codes for row in failures.rows)
    return {
        "status": "PASS" if not failures.rows else "FAIL",
        "expected_outcome_mismatches": count("expected-outcome"),
        "invariant_violations": sum(row["code"].startswith("I") or row["code"] in {"state-machine", "authority", "refund"} for row in failures.rows),
        "false_accepts": count("false-accept"),
        "false_rejects": count("false-reject"),
        "duplicates": count("duplicate"),
        "dirty_state": count("dirty-state", "continuity"),
        "event_mismatches": count("event-mismatch"),
        "timeouts": count("timeout"),
        "completed_attempts": completed_attempts,
        "completed_units": completed_units,
    }


def _check_scenario_aggregation(
    scenario: Mapping[str, Any],
    unit_results: Sequence[Mapping[str, Any]],
    repetition: int,
    failures: CheckFailure,
) -> None:
    cardinality = scenario.get("cardinality")
    context = (scenario["scenario_id"], repetition, "scenario")
    if cardinality:
        accepted = [item for item in unit_results if _actual_outcome(item) == "accepted"]
        rejected = [item for item in unit_results if _actual_outcome(item) == "rejected"]
        _equal(len(accepted), cardinality["accepted"], "duplicate", "race accepted cardinality", context, failures)
        _equal(len(rejected), cardinality["rejected"], "duplicate", "race rejected cardinality", context, failures)
        for result in rejected:
            _equal(result["outcome"]["common_error_code"], cardinality["rejected_error_code"], "expected-outcome", "race loser error", context, failures)
    if scenario["category"] in {"duplicate", "competition"}:
        competing = [
            item for item in unit_results
            if not (scenario["scenario_id"] == "RACE-QUOTA-01" and item["request"]["phase"] == "settle")
        ]
        if not competing or _actual_outcome(competing[0]) != "accepted":
            failures.add("duplicate", "earliest included competing attempt was not the sole winner", scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id="scenario")
        if any(_actual_outcome(item) != "rejected" for item in competing[1:]):
            failures.add("duplicate", "a later included competing attempt was not rejected", scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id="scenario")

        settlements = max(
            item["snapshots"]["post"]["evidence"]["settlement_record_count"]
            for item in unit_results
        )
        success_events = sum(item["events"]["success_event_count"] for item in unit_results)
        settlement_ids = {
            item["snapshots"]["post"]["evidence"]["settlement_id"]
            for item in unit_results
            if item["snapshots"]["post"]["evidence"]["settlement_id"] is not None
        }
        settlement_scenarios = {"DUP-SETTLE-01", "RACE-RELAYER-01", "RACE-QUOTA-01"}
        if scenario["scenario_id"] in settlement_scenarios:
            accepted_settlements = [
                item for item in unit_results
                if item["request"]["phase"] == "settle" and _actual_outcome(item) == "accepted"
            ]
            if settlements != 1 or success_events != 1 or len(settlement_ids) != 1 or len(accepted_settlements) != 1:
                failures.add(
                    "duplicate",
                    f"exactly-once closure differs: records={settlements}, settlement_ids={len(settlement_ids)}, accepted_settles={len(accepted_settlements)}, success_events={success_events}",
                    scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id="scenario",
                )
            settled_order_ids = {
                item["snapshots"]["post"]["order"]["order_id"]
                for item in unit_results
                if item["snapshots"]["post"]["order"]["state"] == "Settled"
            }
            if len(settled_order_ids) != 1:
                failures.add("duplicate", "settlement race did not close exactly one order", scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id="scenario")
        elif settlements != 0 or success_events != 0 or settlement_ids:
            failures.add(
                "duplicate",
                f"non-settlement race created settlement closure: records={settlements}, settlement_ids={len(settlement_ids)}, success_events={success_events}",
                scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id="scenario",
            )

        if scenario["scenario_id"] == "RACE-BUYER-01":
            funded = [
                item for item in unit_results
                if item["snapshots"]["post"]["order"]["state"] == "Funded"
                and int(item["snapshots"]["post"]["payment"]["escrow_amount"]) > 0
            ]
            buyers = {item["snapshots"]["post"]["order"]["buyer"] for item in funded}
            if len(funded) != len(unit_results) or len(buyers) != 1:
                failures.add("duplicate", "buyer race did not leave exactly one funded buyer/escrow fact", scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id="scenario")
        elif scenario["scenario_id"] == "DUP-REFUND-01":
            if any(
                item["snapshots"]["post"]["order"]["state"] != "Refunded"
                or item["snapshots"]["post"]["payment"]["state"] != "Refunded"
                or item["snapshots"]["post"]["order"]["active_capacity_occupied"]
                or item["snapshots"]["post"]["order"]["funded_capacity_occupied"]
                for item in unit_results
            ):
                failures.add("duplicate", "duplicate refund left a non-terminal order or escrow", scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id="scenario")
        elif scenario["scenario_id"] == "RACE-QUOTA-01":
            accepted_funds = [
                item for item in unit_results
                if item["request"]["phase"] == "fund_order" and _actual_outcome(item) == "accepted"
            ]
            settle = [item for item in unit_results if item["request"]["phase"] == "settle"]
            counts = [
                (
                    item["snapshots"]["post"]["authorization"]["issued_count"],
                    item["snapshots"]["post"]["authorization"]["reserved_count"],
                )
                for item in unit_results
            ]
            if (
                len(accepted_funds) != 1
                or len(settle) != 1
                or _actual_outcome(settle[0]) != "accepted"
                or settle[0]["snapshots"]["post"]["order"]["state"] != "Settled"
                or settle[0]["snapshots"]["post"]["payment"]["state"] != "Released"
                or settle[0]["snapshots"]["post"]["order"]["active_capacity_occupied"]
                or settle[0]["snapshots"]["post"]["order"]["funded_capacity_occupied"]
                or any(issued > 1 or reserved > 1 or issued + reserved > 1 for issued, reserved in counts)
            ):
                failures.add("duplicate", "last-quota race exceeded one reservation/issuance or failed to settle its sole winner", scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id="scenario")


def _check_e0_reproducibility(
    raw_root: Path, preflight: Mapping[str, Any], failures: CheckFailure
) -> None:
    if preflight["experiment_family"] != "E0":
        return
    expected_shared_input = "0xc427552f41308fe6e7345a4af448ef3670caba676f1da46e433a1b0aa88e9c13"
    for scenario_id in preflight["inputs"]["selected_scenario_ids"]:
        rows = []
        for repetition in (1, 2):
            path = raw_root / "attempts" / scenario_id / f"r-{repetition:03d}" / "unit-manifest.json"
            try:
                rows.append(_load_required(path))
            except Phase4ValidationError as error:
                failures.add("reproducibility", str(error), scenario_id=scenario_id, repetition=repetition, attempt_id="unit-manifest")
        if len(rows) != 2:
            continue
        for field in ("genesis_hash", "metadata_hash", "fixture_state_hash", "scenario_result_hash"):
            if rows[0].get(field) != rows[1].get(field):
                failures.add("reproducibility", f"E0 repetitions differ in {field}", scenario_id=scenario_id, repetition=2, attempt_id="unit-manifest")
        for repetition, row in enumerate(rows, 1):
            if row.get("shared_input_hash") != expected_shared_input:
                failures.add("reproducibility", "E0 shared input hash differs from the frozen fixture", scenario_id=scenario_id, repetition=repetition, attempt_id="unit-manifest")


def check_run(raw_root: Path, *, write_report: bool = False) -> dict[str, Any]:
    preflight = _load_required(raw_root / "manifest.preflight.json")
    validate_preflight(preflight)
    catalog = load_catalog()
    scenarios = scenario_map(catalog)
    fixture = _settlement_fixture(preflight)
    failures = CheckFailure()
    completed_attempts = 0
    completed_units = 0
    seen_submission: dict[tuple[str, int], set[int]] = {}
    for unit in preflight["planned_execution"]:
        scenario = scenarios[unit["scenario_id"]]
        repetition = unit["repetition"]
        repetition_dir = raw_root / "attempts" / scenario["scenario_id"] / f"r-{repetition:03d}"
        unit_results: list[Mapping[str, Any]] = []
        unit_block_evidence: list[Mapping[str, Any]] = []
        if scenario["execution_mode"] == "test-gate":
            path = repetition_dir / "attempt-001" / "test-gate.json"
            try:
                _check_test_gate(path, preflight, scenario, repetition, failures)
                completed_attempts += 1
                completed_units += 1
            except Phase4ValidationError as error:
                failures.add("missing-artifact", str(error), scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id="atomic-rollback-test")
            continue
        for index, expected in enumerate(scenario["attempts"], 1):
            attempt_dir = repetition_dir / f"attempt-{index:03d}"
            try:
                result, block_evidence = _check_attempt(
                    attempt_dir, preflight, scenario, expected, repetition,
                    failures, fixture,
                )
                unit_results.append(result)
                unit_block_evidence.append(block_evidence)
                completed_attempts += 1
            except Phase4ValidationError as error:
                failures.add("missing-artifact", str(error), scenario_id=scenario["scenario_id"], repetition=repetition, attempt_id=expected["attempt_id"])
        if len(unit_results) != len(scenario["attempts"]):
            continue
        completed_units += 1
        inclusion_results, inclusion_evidence = _order_attempts_by_inclusion(
            unit_results, unit_block_evidence, scenario, repetition, failures
        )
        _check_attempt_continuity(
            inclusion_results, inclusion_evidence, scenario, repetition, failures
        )
        _check_submission_sequences(unit_results, scenario, repetition, failures)
        seen_submission[(scenario["scenario_id"], repetition)] = {
            item["outcome"]["submission_sequence"] for item in unit_results
        }
        _check_scenario_aggregation(scenario, inclusion_results, repetition, failures)
    _check_e0_reproducibility(raw_root, preflight, failures)
    summary = _summary(failures, completed_attempts, completed_units)
    report = {
        "schema_id": "assetxchain-exp2-phase4-checker-report",
        "schema_version": 1,
        "tooling_version": TOOLING_VERSION,
        "run_id": preflight["run_id"],
        "baseline_id": preflight["baseline"]["baseline_id"],
        "summary": summary,
        "failures": sorted(failures.rows, key=lambda row: (row["scenario_id"], row["repetition"], row["attempt_id"], row["code"], row["message"])),
    }
    if write_report:
        write_json_exclusive(raw_root / "checker-report.json", report)
    return report


def seal_summary(report: Mapping[str, Any]) -> dict[str, Any]:
    summary = report["summary"]
    return {field: summary[field] for field in (
        "status", "expected_outcome_mismatches", "invariant_violations",
        "false_accepts", "false_rejects", "duplicates", "dirty_state",
        "event_mismatches", "timeouts",
    )}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("raw_root", type=Path)
    parser.add_argument("--write-report", action="store_true")
    args = parser.parse_args()
    report = check_run(args.raw_root, write_report=args.write_report)
    print(json.dumps(report, sort_keys=True))
    return 0 if report["summary"]["status"] == "PASS" else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Phase4ValidationError as error:
        print(f"ERROR: {error}", file=os.sys.stderr)
        raise SystemExit(2)
