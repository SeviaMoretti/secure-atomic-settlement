from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path


COMMON_DIR = Path(__file__).resolve().parents[1]
EXP_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(COMMON_DIR))

from phase3_contract import load_json, validate_instance  # noqa: E402
from shared_fixture_codec import (  # noqa: E402
    BASELINES,
    BUYER_FIELDS,
    COMMON_ERRORS,
    MESSAGE_TYPES,
    OPERATIONS,
    ORDER_FIELDS,
    SELLER_FIELDS,
    FixtureCodecError,
    baseline_consumer_view,
    blake2_256,
    build_bundle,
    domain_separator,
    encode_buyer,
    encode_order,
    encode_seller,
    hex_bytes,
    to_hex,
    validate_bundle_semantics,
)


BUNDLE_PATH = EXP_ROOT / "fixtures/phase3/assetxchain-paper2-shared-fixture-v1.json"
SCHEMA_PATH = EXP_ROOT / "schemas/phase3/shared-fixture-vectors.schema.json"
EXPECTED_SHARED_INPUT_HASH = "0xc427552f41308fe6e7345a4af448ef3670caba676f1da46e433a1b0aa88e9c13"
EXPECTED_VECTOR_ANCHORS = {
    "TransferMeta": {
        "length": 236,
        "order_id": "0x71b80d5405da2ca9a67e3e01eaf9797495e5b8c9522c2eb5c19f1781fe0035db",
        "seller_hash": "0x0cb6005d63428ab432c43df6ef52d256879437e0c8441f3a924568285aa3db81",
        "buyer_hash": "0x4a4a2a3cfddf46c785347833eeffc1d03b0474023e7bbc2154f71bef0f32799c",
    },
    "IssueRight": {
        "length": 268,
        "order_id": "0xe07f2543d446a2eed1e978fc96bac64526da709679bfa0d8cf183fea4cec4396",
        "seller_hash": "0x1cf57fb9b944956fd5ace0379f3f691ffca3a4a5b2dbbfc43a3ba1b630724150",
        "buyer_hash": "0x7454cfcdf1564874dbaf497e60bbfa5e343d8cb40b6c3cd22fec749830ac8bcc",
    },
    "TransferRight": {
        "length": 300,
        "order_id": "0x00130e594d82265db390967a14004e97d2fd361c4204998e983ddef30fc540dc",
        "seller_hash": "0xd058a33fe354b3558c7192b2b88e33e39ac241c4ff1ddf00168fc8886a7627b9",
        "buyer_hash": "0x46830f867a19685881322bc2133b7b977f4bfea71ddbd20ad522316bb0fc72c4",
    },
}


class SharedFixtureCodecTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.bundle = load_json(BUNDLE_PATH)
        cls.dev = load_json(EXP_ROOT / "configs/dev.json")
        cls.common = load_json(EXP_ROOT / "configs/baselines/phase3-common.json")

    def test_generated_bundle_is_current_and_schema_valid(self) -> None:
        self.assertEqual(self.bundle, build_bundle(EXP_ROOT))
        validate_instance(self.bundle, load_json(SCHEMA_PATH))
        validate_bundle_semantics(self.bundle, EXP_ROOT)

    def test_all_baselines_consume_identical_bytes_and_frozen_inputs(self) -> None:
        views = {baseline: baseline_consumer_view(self.bundle, baseline) for baseline in BASELINES}
        self.assertEqual(views["B1"], views["B2"])
        self.assertEqual(views["B2"], views["G3"])
        self.assertEqual(
            {item["shared_input_hash"] for item in self.bundle["baseline_consumers"].values()},
            {EXPECTED_SHARED_INPUT_HASH},
        )
        inputs = views["B1"]["fixture_inputs"]
        self.assertEqual(inputs["seed"], 20260726)
        self.assertEqual(inputs["quantity"], 1)
        self.assertEqual(inputs["unit_plancks"], "1000000000000")
        self.assertEqual(
            inputs["prices_plancks"],
            {"TransferMeta": "10000000000000", "IssueRight": "3000000000000", "TransferRight": "4000000000000"},
        )
        self.assertEqual(
            inputs["timing_offsets_blocks"],
            {"order_expiry": 20, "funded_grace": 10, "right_validity": 100},
        )
        self.assertEqual(inputs["vector_block_context"]["order_expiry_block"], 20)
        self.assertIn("live adapters apply offsets", inputs["vector_block_context"]["scope"])

    def test_accounts_are_exact_frozen_account_id32_values(self) -> None:
        source_roles = self.dev["shared_fixture"]["roles"]
        self.assertEqual(list(self.bundle["accounts"]), list(source_roles))
        for role, account in self.bundle["accounts"].items():
            self.assertEqual(account["account_id_hex"], source_roles[role]["account_id_hex"])
            self.assertEqual(len(hex_bytes(account["account_id_hex"])), 32)
            self.assertEqual(account["ss58_address"], source_roles[role]["ss58_address"])

    def test_enum_discriminants_field_order_and_errors_are_frozen(self) -> None:
        self.assertEqual(self.bundle["enums"]["MarketOperation"], OPERATIONS)
        self.assertEqual(self.bundle["enums"]["MessageType"], MESSAGE_TYPES)
        self.assertEqual(self.bundle["field_order"]["OrderCommitment"], ORDER_FIELDS)
        self.assertEqual(self.bundle["field_order"]["SellerCommitment"], SELLER_FIELDS)
        self.assertEqual(self.bundle["field_order"]["BuyerCommitment"], BUYER_FIELDS)
        self.assertEqual([item["name"] for item in self.bundle["common_errors"]], COMMON_ERRORS)
        self.assertEqual([item["status_code"] for item in self.bundle["common_errors"]], list(range(1, 34)))
        self.assertEqual([item["scale_hex"] for item in self.bundle["common_errors"]], [f"0x{i:02x}" for i in range(1, 34)])

    def test_three_operation_bytes_hashes_ids_and_signing_messages(self) -> None:
        genesis_hash = self.bundle["fixture_inputs"]["vector_genesis_hash"]
        self.assertEqual([item["operation"] for item in self.bundle["vectors"]], list(OPERATIONS))
        for vector in self.bundle["vectors"]:
            operation = vector["operation"]
            anchors = EXPECTED_VECTOR_ANCHORS[operation]
            order_bytes = encode_order(vector["order_commitment"])
            seller_bytes = encode_seller(vector["seller_commitment"])
            buyer_bytes = encode_buyer(vector["buyer_commitment"])
            self.assertEqual(to_hex(order_bytes), vector["scale"]["order_commitment_hex"])
            self.assertEqual(to_hex(seller_bytes), vector["scale"]["seller_commitment_hex"])
            self.assertEqual(to_hex(buyer_bytes), vector["scale"]["buyer_commitment_hex"])
            self.assertEqual(len(order_bytes), anchors["length"])
            self.assertEqual(vector["hashes"]["order_id"], anchors["order_id"])
            self.assertEqual(vector["hashes"]["seller_signing_payload_hash"], anchors["seller_hash"])
            self.assertEqual(vector["hashes"]["buyer_signing_payload_hash"], anchors["buyer_hash"])
            self.assertEqual(to_hex(blake2_256(order_bytes)), vector["hashes"]["order_commitment_hash"])
            seller_payload = domain_separator(genesis_hash, operation, "Seller") + seller_bytes
            buyer_payload = domain_separator(genesis_hash, operation, "Buyer") + buyer_bytes
            self.assertEqual(to_hex(seller_payload), vector["scale"]["seller_signing_payload_hex"])
            self.assertEqual(to_hex(buyer_payload), vector["scale"]["buyer_signing_payload_hex"])

    def test_g3_rust_reference_domain_matches(self) -> None:
        reference = self.bundle["g3_rust_reference"]
        self.assertEqual(
            to_hex(domain_separator(reference["genesis_hash"], reference["operation"], reference["message_type"])),
            "0x386902c5edd609861179d1ab6c6649622d74de3b79939656809ad20b03d115b7",
        )

    def test_reordered_commitment_and_source_drift_are_rejected(self) -> None:
        order = copy.deepcopy(self.bundle["vectors"][0]["order_commitment"])
        reordered = {key: order[key] for key in reversed(order)}
        with self.assertRaises(FixtureCodecError):
            encode_order(reordered)

        stale = copy.deepcopy(self.bundle)
        stale["fixture_inputs"]["quantity"] = 2
        with self.assertRaises(FixtureCodecError):
            validate_bundle_semantics(stale, EXP_ROOT)


if __name__ == "__main__":
    unittest.main()
