"""Canonical SCALE codec and shared fixture vectors for Phase 3 baselines.

The module deliberately has no baseline business logic. B1, B2, and G3 use
the same bytes and identifiers while retaining their separate authority maps.
"""

from __future__ import annotations

import hashlib
import json
from copy import deepcopy
from pathlib import Path
from typing import Any, Mapping


FIXTURE_PROFILE_ID = "assetxchain-paper2-shared-fixture-v1"
VECTOR_BUNDLE_ID = "assetxchain-paper2-shared-vectors-v1"
PROTOCOL_LABEL = "assetxchain-paper2-shared-v1"
DOMAIN_ID = b"axc/p2/shared"
PROTOCOL_VERSION = 1
BASELINES = ("B1", "B2", "G3")
OPERATIONS = {"TransferMeta": 0, "IssueRight": 1, "TransferRight": 2}
OBJECT_TYPES = {"MetaToken": 0, "RightToken": 1}
ORDER_STATES = {"Open": 0, "Funded": 1, "Settled": 2, "Cancelled": 3, "Expired": 4, "Refunded": 5}
PAYMENT_STATES = {"Unfunded": 0, "Escrowed": 1, "Released": 2, "Refunded": 3}
CURRENCIES = {"Native": 0}
MESSAGE_TYPES = {"Order": 0, "Seller": 1, "Buyer": 2, "Settlement": 3, "Right": 4}

ORDER_FIELDS = [
    "settlement_protocol_version",
    "market_id",
    "operation",
    "object_type",
    "object_id",
    "parent_asset_id",
    "authorization_id",
    "seller",
    "allowed_buyer",
    "price",
    "currency",
    "policy_hash",
    "right_parameters_hash",
    "seller_state_version",
    "order_nonce",
    "expiry",
]
SELLER_FIELDS = [
    "order_id",
    "object_id",
    "operation",
    "seller",
    "state_version",
    "price_policy_hash",
    "right_parameters_hash",
    "expires_at",
    "seller_nonce",
]
BUYER_FIELDS = ["order_id", "buyer", "price", "currency", "expires_at", "buyer_nonce"]

COMMON_ERRORS = [
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
]


class FixtureCodecError(ValueError):
    """Raised when frozen fixture input or a canonical value is malformed."""


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def hex_bytes(value: str, length: int = 32) -> bytes:
    if not isinstance(value, str) or not value.startswith("0x"):
        raise FixtureCodecError("hex value must start with 0x")
    try:
        decoded = bytes.fromhex(value[2:])
    except ValueError as error:
        raise FixtureCodecError("invalid hex value") from error
    if len(decoded) != length:
        raise FixtureCodecError(f"expected {length} bytes, got {len(decoded)}")
    return decoded


def to_hex(value: bytes) -> str:
    return "0x" + value.hex()


def uint_le(value: int, bits: int) -> bytes:
    if not isinstance(value, int) or isinstance(value, bool) or value < 0 or value >= 1 << bits:
        raise FixtureCodecError(f"value does not fit u{bits}: {value!r}")
    return value.to_bytes(bits // 8, "little")


def enum_byte(mapping: Mapping[str, int], value: str) -> bytes:
    try:
        return bytes([mapping[value]])
    except KeyError as error:
        raise FixtureCodecError(f"unknown enum value: {value}") from error


def option_fixed_hex(value: str | None, length: int = 32) -> bytes:
    return b"\x00" if value is None else b"\x01" + hex_bytes(value, length)


def blake2_256(value: bytes) -> bytes:
    return hashlib.blake2b(value, digest_size=32).digest()


def encode_order(order: Mapping[str, Any]) -> bytes:
    if list(order) != ORDER_FIELDS:
        raise FixtureCodecError("OrderCommitment field order drift")
    return b"".join(
        [
            uint_le(order["settlement_protocol_version"], 16),
            hex_bytes(order["market_id"]),
            enum_byte(OPERATIONS, order["operation"]),
            enum_byte(OBJECT_TYPES, order["object_type"]),
            hex_bytes(order["object_id"]),
            option_fixed_hex(order["parent_asset_id"]),
            hex_bytes(order["authorization_id"]),
            hex_bytes(order["seller"]),
            option_fixed_hex(order["allowed_buyer"]),
            uint_le(int(order["price"]), 128),
            enum_byte(CURRENCIES, order["currency"]),
            hex_bytes(order["policy_hash"]),
            option_fixed_hex(order["right_parameters_hash"]),
            uint_le(order["seller_state_version"], 64),
            uint_le(order["order_nonce"], 64),
            uint_le(order["expiry"], 32),
        ]
    )


def encode_seller(commitment: Mapping[str, Any]) -> bytes:
    if list(commitment) != SELLER_FIELDS:
        raise FixtureCodecError("SellerCommitment field order drift")
    return b"".join(
        [
            hex_bytes(commitment["order_id"]),
            hex_bytes(commitment["object_id"]),
            enum_byte(OPERATIONS, commitment["operation"]),
            hex_bytes(commitment["seller"]),
            uint_le(commitment["state_version"], 64),
            hex_bytes(commitment["price_policy_hash"]),
            option_fixed_hex(commitment["right_parameters_hash"]),
            uint_le(commitment["expires_at"], 32),
            uint_le(commitment["seller_nonce"], 64),
        ]
    )


def encode_buyer(commitment: Mapping[str, Any]) -> bytes:
    if list(commitment) != BUYER_FIELDS:
        raise FixtureCodecError("BuyerCommitment field order drift")
    return b"".join(
        [
            hex_bytes(commitment["order_id"]),
            hex_bytes(commitment["buyer"]),
            uint_le(int(commitment["price"]), 128),
            enum_byte(CURRENCIES, commitment["currency"]),
            uint_le(commitment["expires_at"], 32),
            uint_le(commitment["buyer_nonce"], 64),
        ]
    )


def domain_separator(genesis_hash: str, operation: str, message_type: str) -> bytes:
    encoded = b"".join(
        [
            hex_bytes(genesis_hash),
            DOMAIN_ID,
            uint_le(PROTOCOL_VERSION, 16),
            enum_byte(MESSAGE_TYPES, message_type),
            enum_byte(OPERATIONS, operation),
        ]
    )
    return blake2_256(encoded)


def hash_tuple(*parts: bytes) -> bytes:
    return blake2_256(b"".join(parts))


def fixture_id(seed: int, label: str) -> str:
    preimage = FIXTURE_PROFILE_ID.encode("ascii") + b"/" + uint_le(seed, 64) + b"/" + label.encode("ascii")
    return to_hex(blake2_256(preimage))


def _price_map(fixture: Mapping[str, Any]) -> dict[str, str]:
    tx = fixture["transactions"]
    return {
        "TransferMeta": tx["meta_sale_price_plancks"],
        "IssueRight": tx["right_issue_price_plancks"],
        "TransferRight": tx["right_resale_price_plancks"],
    }


def _operation_shape(operation: str, ids: Mapping[str, str]) -> tuple[str, str, str | None, str | None, str]:
    if operation == "TransferMeta":
        return "MetaToken", ids["meta_asset"], None, None, "OWNER"
    if operation == "IssueRight":
        return "MetaToken", ids["meta_asset"], None, ids["right_parameters"], "OWNER"
    return "RightToken", ids["existing_right"], ids["meta_asset"], ids["right_parameters"], "RIGHT_SELLER"


def _build_vector(
    operation: str,
    *,
    genesis_hash: str,
    fixture: Mapping[str, Any],
    ids: Mapping[str, str],
) -> dict[str, Any]:
    roles = fixture["roles"]
    state = fixture["state"]
    timing = fixture["timing"]
    object_type, object_id, parent_id, right_hash, seller_role = _operation_shape(operation, ids)
    buyer_role = "BUYER_2" if operation == "TransferRight" else "BUYER_1"
    order = {
        "settlement_protocol_version": PROTOCOL_VERSION,
        "market_id": ids["market"],
        "operation": operation,
        "object_type": object_type,
        "object_id": object_id,
        "parent_asset_id": parent_id,
        "authorization_id": ids[f"authorization_{operation}"],
        "seller": roles[seller_role]["account_id_hex"],
        "allowed_buyer": roles[buyer_role]["account_id_hex"],
        "price": _price_map(fixture)[operation],
        "currency": "Native",
        "policy_hash": ids[f"policy_{operation}"],
        "right_parameters_hash": right_hash,
        "seller_state_version": state["initial_state_version"],
        "order_nonce": state["initial_seller_nonce"],
        "expiry": timing["order_expiry_offset_blocks"],
    }
    order_bytes = encode_order(order)
    order_domain = domain_separator(genesis_hash, operation, "Order")
    order_id = to_hex(hash_tuple(order_domain, order_bytes))
    seller = {
        "order_id": order_id,
        "object_id": object_id,
        "operation": operation,
        "seller": order["seller"],
        "state_version": order["seller_state_version"],
        "price_policy_hash": order["policy_hash"],
        "right_parameters_hash": right_hash,
        "expires_at": order["expiry"],
        "seller_nonce": order["order_nonce"],
    }
    buyer = {
        "order_id": order_id,
        "buyer": order["allowed_buyer"],
        "price": order["price"],
        "currency": order["currency"],
        "expires_at": order["expiry"],
        "buyer_nonce": state["initial_buyer_nonce"],
    }
    seller_payload = domain_separator(genesis_hash, operation, "Seller") + encode_seller(seller)
    buyer_payload = domain_separator(genesis_hash, operation, "Buyer") + encode_buyer(buyer)
    settlement_domain = domain_separator(genesis_hash, operation, "Settlement")
    return {
        "operation": operation,
        "roles": {"seller": seller_role, "buyer": buyer_role},
        "order_commitment": order,
        "seller_commitment": seller,
        "buyer_commitment": buyer,
        "scale": {
            "order_commitment_hex": to_hex(order_bytes),
            "seller_commitment_hex": to_hex(encode_seller(seller)),
            "buyer_commitment_hex": to_hex(encode_buyer(buyer)),
            "seller_signing_payload_hex": to_hex(seller_payload),
            "buyer_signing_payload_hex": to_hex(buyer_payload),
        },
        "hashes": {
            "order_domain": to_hex(order_domain),
            "seller_domain": to_hex(domain_separator(genesis_hash, operation, "Seller")),
            "buyer_domain": to_hex(domain_separator(genesis_hash, operation, "Buyer")),
            "settlement_domain": to_hex(settlement_domain),
            "right_domain": to_hex(domain_separator(genesis_hash, operation, "Right")),
            "order_commitment_hash": to_hex(blake2_256(order_bytes)),
            "order_id": order_id,
            "settlement_id": to_hex(hash_tuple(settlement_domain, hex_bytes(order_id), b"settlement")),
            "right_id": to_hex(hash_tuple(domain_separator(genesis_hash, operation, "Right"), hex_bytes(order_id), b"right")),
            "escrow_id": to_hex(hash_tuple(settlement_domain, hex_bytes(order_id), b"escrow")),
            "seller_signing_payload_hash": to_hex(blake2_256(seller_payload)),
            "buyer_signing_payload_hash": to_hex(blake2_256(buyer_payload)),
        },
    }


def _canonical_json_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def baseline_consumer_view(bundle: Mapping[str, Any], baseline_id: str) -> dict[str, Any]:
    if baseline_id not in BASELINES:
        raise FixtureCodecError(f"unknown baseline: {baseline_id}")
    return deepcopy(
        {
            "fixture_profile_id": bundle["fixture_profile_id"],
            "protocol": bundle["protocol"],
            "accounts": bundle["accounts"],
            "fixture_inputs": bundle["fixture_inputs"],
            "enums": bundle["enums"],
            "field_order": bundle["field_order"],
            "common_errors": bundle["common_errors"],
            "vectors": bundle["vectors"],
        }
    )


def validate_source(dev: Mapping[str, Any], common: Mapping[str, Any]) -> None:
    protocol = dev["protocol"]
    fixture = dev["shared_fixture"]
    if protocol["protocol_label"] != PROTOCOL_LABEL:
        raise FixtureCodecError("protocol label drift")
    if protocol["settlement_protocol_version"] != PROTOCOL_VERSION:
        raise FixtureCodecError("protocol version drift")
    if protocol["settlement_domain_id"].encode("ascii") != DOMAIN_ID:
        raise FixtureCodecError("domain id drift")
    if protocol["encoding"] != "SCALE" or protocol["hash_scheme_id"] != "blake2-256":
        raise FixtureCodecError("codec or hash scheme drift")
    if fixture["fixture_profile_id"] != FIXTURE_PROFILE_ID:
        raise FixtureCodecError("fixture profile drift")
    if fixture["applies_to_baseline_ids"] != list(BASELINES):
        raise FixtureCodecError("fixture baseline order drift")
    if common["common_error_codes"] != COMMON_ERRORS:
        raise FixtureCodecError("common error vocabulary or order drift")


def build_bundle(exp_root: Path) -> dict[str, Any]:
    dev = load_json(exp_root / "configs/dev.json")
    common = load_json(exp_root / "configs/baselines/phase3-common.json")
    validate_source(dev, common)
    fixture = dev["shared_fixture"]
    seed = dev["execution"]["common_seed"]
    labels = [
        "test-genesis",
        "market",
        "meta-asset",
        "existing-right",
        "right-parameters",
        *[f"authorization-{name}" for name in OPERATIONS],
        *[f"policy-{name}" for name in OPERATIONS],
    ]
    raw_ids = {label: fixture_id(seed, label) for label in labels}
    ids = {
        "market": raw_ids["market"],
        "meta_asset": raw_ids["meta-asset"],
        "existing_right": raw_ids["existing-right"],
        "right_parameters": raw_ids["right-parameters"],
        **{f"authorization_{name}": raw_ids[f"authorization-{name}"] for name in OPERATIONS},
        **{f"policy_{name}": raw_ids[f"policy-{name}"] for name in OPERATIONS},
    }
    genesis_hash = raw_ids["test-genesis"]
    vectors = [
        _build_vector(name, genesis_hash=genesis_hash, fixture=fixture, ids=ids)
        for name in OPERATIONS
    ]
    accounts = {
        role: {
            "key_uri": value["key_uri"],
            "account_id_hex": value["account_id_hex"],
            "ss58_address": value["ss58_address"],
        }
        for role, value in fixture["roles"].items()
    }
    fixture_inputs = {
        "seed": seed,
        "quantity": fixture["transactions"]["quantity"],
        "unit_plancks": fixture["funding"]["unit_plancks"],
        "prices_plancks": _price_map(fixture),
        "timing_offsets_blocks": {
            "order_expiry": fixture["timing"]["order_expiry_offset_blocks"],
            "funded_grace": fixture["timing"]["funded_grace_offset_blocks"],
            "right_validity": fixture["timing"]["right_validity_offset_blocks"],
        },
        "vector_block_context": {
            "fixture_anchor_block": 0,
            "order_created_block": 0,
            "order_expiry_block": fixture["timing"]["order_expiry_offset_blocks"],
            "order_funded_block": 0,
            "grace_deadline_block": fixture["timing"]["funded_grace_offset_blocks"],
            "right_valid_until_block": fixture["timing"]["right_validity_offset_blocks"],
            "scope": "static-codec-vector-only; live adapters apply offsets to captured blocks",
        },
        "fees_bps": {
            "market": fixture["transactions"]["market_fee_bps"],
            "safety": fixture["transactions"]["safety_fee_bps"],
            "meta_sale_royalty": fixture["transactions"]["meta_sale_royalty_bps"],
            "right_issue_royalty": fixture["transactions"]["right_issue_royalty_bps"],
            "right_resale_royalty": fixture["transactions"]["right_resale_royalty_bps"],
        },
        "initial_state": fixture["state"],
        "vector_genesis_hash": genesis_hash,
        "derived_ids": ids,
    }
    bundle: dict[str, Any] = {
        "schema_id": "assetxchain-exp2-shared-fixture-vectors",
        "schema_version": 1,
        "vector_bundle_id": VECTOR_BUNDLE_ID,
        "fixture_profile_id": FIXTURE_PROFILE_ID,
        "source": "configs/dev.json#shared_fixture",
        "generator": "scripts/common/generate_shared_fixture.py",
        "protocol": {
            "protocol_label": PROTOCOL_LABEL,
            "settlement_protocol_version": PROTOCOL_VERSION,
            "settlement_domain_id": DOMAIN_ID.decode("ascii"),
            "encoding": "SCALE",
            "hash_scheme_id": "blake2-256",
            "signature_scheme_id": "sr25519-substrate-v1",
        },
        "accounts": accounts,
        "fixture_inputs": fixture_inputs,
        "enums": {
            "MarketOperation": OPERATIONS,
            "ObjectType": OBJECT_TYPES,
            "OrderState": ORDER_STATES,
            "PaymentState": PAYMENT_STATES,
            "CurrencyId": CURRENCIES,
            "MessageType": MESSAGE_TYPES,
        },
        "field_order": {
            "OrderCommitment": ORDER_FIELDS,
            "SellerCommitment": SELLER_FIELDS,
            "BuyerCommitment": BUYER_FIELDS,
        },
        "common_errors": [
            {"name": name, "status_code": index, "scale_hex": f"0x{index:02x}"}
            for index, name in enumerate(COMMON_ERRORS, start=1)
        ],
        "vectors": vectors,
        "baseline_consumers": {},
        "g3_rust_reference": {
            "source": "pallets/dataassets/src/tests.rs#protocol_enum_discriminants_and_commitment_vectors_are_stable",
            "genesis_hash": "0x" + "11" * 32,
            "operation": "TransferRight",
            "message_type": "Order",
            "domain_hex": "0x386902c5edd609861179d1ab6c6649622d74de3b79939656809ad20b03d115b7",
        },
    }
    consumer_hash = to_hex(blake2_256(_canonical_json_bytes(baseline_consumer_view(bundle, "B1"))))
    bundle["baseline_consumers"] = {
        baseline_id: {
            "consumer": "scripts/common/shared_fixture_codec.py#baseline_consumer_view",
            "shared_input_hash": consumer_hash,
        }
        for baseline_id in BASELINES
    }
    return bundle


def validate_bundle_semantics(bundle: Mapping[str, Any], exp_root: Path) -> None:
    expected = build_bundle(exp_root)
    if bundle != expected:
        raise FixtureCodecError("shared fixture bundle differs from canonical generator output")
    views = [baseline_consumer_view(bundle, baseline_id) for baseline_id in BASELINES]
    if not all(view == views[0] for view in views[1:]):
        raise FixtureCodecError("baseline consumer inputs differ")
    hashes = {item["shared_input_hash"] for item in bundle["baseline_consumers"].values()}
    if len(hashes) != 1:
        raise FixtureCodecError("baseline consumer hashes differ")
    reference = bundle["g3_rust_reference"]
    actual_domain = to_hex(
        domain_separator(reference["genesis_hash"], reference["operation"], reference["message_type"])
    )
    if actual_domain != reference["domain_hex"]:
        raise FixtureCodecError("Python codec disagrees with the frozen G3 Rust domain vector")
