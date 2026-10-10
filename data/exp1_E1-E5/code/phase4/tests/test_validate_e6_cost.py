from __future__ import annotations

import copy
import json
import sys
import unittest
from pathlib import Path


PHASE4 = Path(__file__).resolve().parents[1]
if str(PHASE4) not in sys.path:
    sys.path.insert(0, str(PHASE4))

from validate_e6_cost import E6ValidationError, actual_chain_metrics, validate_metrics


OLD_ATTEMPT = Path(
    "/home/gz785/code/exp2/results/raw/"
    "p4-e6-g3-20260806T111142+0800-47ebd794-b140542f/"
    "attempts/LEG-ISSUE-01/r-001/attempt-001"
)


class ActualE6MetricTest(unittest.TestCase):
    def setUp(self) -> None:
        self.block = json.loads((OLD_ATTEMPT / "block-events.json").read_text(encoding="utf-8"))
        self.result = json.loads((OLD_ATTEMPT / "result.json").read_text(encoding="utf-8"))
        self.measurement = json.loads((OLD_ATTEMPT / "e6-measurement.json").read_text(encoding="utf-8"))

    def corrected(self) -> tuple[dict, dict]:
        result = copy.deepcopy(self.result)
        measurement = copy.deepcopy(self.measurement)
        measurement["schema_version"] = 2
        raw = measurement["raw_measurement"]
        raw.update({
            "actual_fee": "2921622787",
            "partial_fee": "2921622787",
            "estimated_partial_fee": "1001122225642",
            "weight_ref_time": "2921622145",
            "weight_proof_size": "85932",
            "post_dispatch_weight": {"refTime": "2921622145", "proofSize": "85932"},
            "declared_payment_weight": {
                "refTime": "1001014068000", "proofSize": "104873721",
            },
            "declared_contract_gas_limit": {
                "refTime": "1000000000000", "proofSize": "104857600",
            },
        })
        result["outcome"]["dispatch_fee_amount"] = "2921622787"
        result["cost"]["partial_fee"]["value"] = "2921622787"
        result["cost"]["weight_ref_time"]["value"] = "2921622145"
        result["cost"]["weight_proof_size"]["value"] = "85932"
        return result, measurement

    def test_reads_actual_metrics_from_current_extrinsic_events(self) -> None:
        self.assertEqual(actual_chain_metrics(self.block, 1), {
            "weight_ref_time": "2921622145",
            "weight_proof_size": "85932",
            "actual_fee": "2921622787",
        })
        result, measurement = self.corrected()
        validate_metrics(result, measurement, self.block, "G3")

    def test_rejects_fixed_gas_limit_as_actual_weight(self) -> None:
        result, measurement = self.corrected()
        raw = measurement["raw_measurement"]
        raw["weight_ref_time"] = "1000000000000"
        raw["weight_proof_size"] = "104857600"
        raw["post_dispatch_weight"] = raw["declared_contract_gas_limit"]
        result["cost"]["weight_ref_time"]["value"] = "1000000000000"
        result["cost"]["weight_proof_size"]["value"] = "104857600"
        with self.assertRaises(E6ValidationError):
            validate_metrics(result, measurement, self.block, "G3")


if __name__ == "__main__":
    unittest.main()
