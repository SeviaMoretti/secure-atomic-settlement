from __future__ import annotations

import copy
import sys
import unittest
from pathlib import Path


COMMON_DIR = Path(__file__).resolve().parents[1]
EXP_ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(COMMON_DIR))

from phase3_conformance import (  # noqa: E402
    CONFIG_PATH,
    ConformanceError,
    _validate_capability_sources,
    _validate_phase3_gate_state,
    _validate_scenarios,
    _validate_schema_contract,
    validate_phase3_conformance,
)
from phase3_contract import load_json  # noqa: E402


class Phase3ConformanceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.config = load_json(EXP_ROOT / CONFIG_PATH)
        cls.common = load_json(
            EXP_ROOT / cls.config["sources"]["common_contract"]
        )
        cls.manifests = {
            baseline_id: load_json(EXP_ROOT / baseline["manifest"])
            for baseline_id, baseline in cls.config["baselines"].items()
        }

    def test_real_static_gate_passes_and_keeps_literature_separate(self) -> None:
        result = validate_phase3_conformance(EXP_ROOT, check_git=False)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(result["phase3_gate_state"], "completed")
        main = result["main_experiment"]
        self.assertEqual(main["baselines"], ["B1", "B2", "G3"])
        self.assertEqual(main["capability_rows"]["common"], 24)
        self.assertEqual(main["scenario_count"], 42)
        self.assertEqual(main["common_error_count"], 33)
        self.assertEqual(main["unexplained_mismatch_count"], 0)
        for literature_id in ("L1", "L2"):
            auxiliary = result["literature_auxiliaries"][literature_id]
            self.assertEqual(auxiliary["status"], "excluded")
            self.assertEqual(auxiliary["declared_common_subset_count"], 0)
            self.assertFalse(auxiliary["aggregated_with_main_experiment"])

    def test_completed_gate_requires_all_frozen_boundary_markers(self) -> None:
        progress = "| `S3.15` | `completed` |"
        with self.assertRaisesRegex(ConformanceError, "missing marker"):
            _validate_phase3_gate_state(progress, "`PHASE_3_GATE = PASS`")
        with self.assertRaisesRegex(ConformanceError, "without a Phase 3 gate"):
            _validate_phase3_gate_state(progress, None)

    def test_architecture_label_drift_is_blocking(self) -> None:
        config = copy.deepcopy(self.config)
        config["architecture_capabilities"]["B1"]["Native"].remove("A01")
        config["architecture_capabilities"]["B1"]["N/A"].append("A01")
        with self.assertRaises(ConformanceError):
            _validate_capability_sources(EXP_ROOT, config, self.common)

    def test_missing_scenario_is_blocking_not_synthetic_failure(self) -> None:
        config = copy.deepcopy(self.config)
        config["scenarios"]["NEG"].remove("NEG-PROTO-01")
        with self.assertRaises(ConformanceError):
            _validate_scenarios(
                EXP_ROOT,
                config,
                self.common["common_error_codes"],
                self.manifests,
            )

    def test_common_and_architecture_field_drift_is_blocking(self) -> None:
        invocation_schema = load_json(
            EXP_ROOT / self.config["sources"]["invocation_schema"]
        )
        snapshot_schema = load_json(
            EXP_ROOT / self.config["sources"]["snapshot_schema"]
        )
        result_schema = load_json(
            EXP_ROOT / self.config["sources"]["result_schema"]
        )
        result_schema["properties"]["architecture_cost"]["required"].remove(
            "typed_ce_call_count"
        )
        with self.assertRaises(ConformanceError):
            _validate_schema_contract(
                self.config,
                self.common,
                invocation_schema,
                snapshot_schema,
                result_schema,
            )

    def test_na_metric_cannot_replace_an_applicable_metric(self) -> None:
        config = copy.deepcopy(self.config)
        config["baselines"]["G3"]["applicable_architecture_metrics"].remove(
            "typed_ce_call_count"
        )
        invocation_schema = load_json(
            EXP_ROOT / config["sources"]["invocation_schema"]
        )
        snapshot_schema = load_json(
            EXP_ROOT / config["sources"]["snapshot_schema"]
        )
        result_schema = load_json(EXP_ROOT / config["sources"]["result_schema"])
        with self.assertRaises(ConformanceError):
            _validate_schema_contract(
                config,
                self.common,
                invocation_schema,
                snapshot_schema,
                result_schema,
            )


if __name__ == "__main__":
    unittest.main()
