import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
REVIEW = ROOT / "reports" / "planning" / "l1-dt-sc-source-review.md"
PROGRESS = ROOT / "reports" / "planning" / "phase3-progress.md"


class L1DtScSourceReviewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = REVIEW.read_text(encoding="utf-8")
        cls.progress = PROGRESS.read_text(encoding="utf-8")

    def test_verified_identity_and_evidence_boundary_are_explicit(self):
        required = (
            "Smart Contract Based Data Trading Mode Using Blockchain and Machine Learning",
            "Wei Xiong and Li Xiong",
            "10.1109/ACCESS.2019.2928325",
            "8760479",
            "pages 102331-102344",
            "Section-level claims are not verified",
            "`DT-SC` is only the AssetXChain project label",
        )
        for marker in required:
            with self.subTest(marker=marker):
                self.assertIn(marker, self.text)

    def test_required_dispositions_and_external_validity_are_present(self):
        for heading in (
            "### Preserved",
            "### Adapted",
            "### Omitted",
            "### N/A",
            "## 7. External-Validity Boundary",
            "## 8. Conditions to Activate L1 Later",
        ):
            with self.subTest(heading=heading):
                self.assertEqual(self.text.count(heading), 1)

        for mechanism in (
            "Similarity-learning availability verification",
            "Purchaser-owner challenge-response",
            "Arbitration and controversy resolution",
            "Storage-provider download protocol",
        ):
            with self.subTest(mechanism=mechanism):
                self.assertIn(mechanism, self.text)

    def test_current_decision_is_exclusion_without_runnable_l1_config(self):
        self.assertIn("Decision: `EXCLUDED_FROM_CURRENT_AUXILIARY_RUN`", self.text)
        self.assertIn("Executable L1 profile/config created: `no`", self.text)
        self.assertIn("L1 run ID created: `no`", self.text)
        self.assertIn("Current validity level: `legacy-pilot`", self.text)
        self.assertIn("`B1 Contract-only` is not L1", self.text)

        config_files = [
            path
            for path in (ROOT / "configs").rglob("*.json")
            if "l1" in path.name.lower() or "dt-sc" in path.name.lower()
        ]
        self.assertEqual(config_files, [])

        b1 = json.loads((ROOT / "configs" / "baselines" / "b1.json").read_text())
        self.assertEqual(b1["baseline_id"], "B1")
        self.assertEqual(b1["baseline_name"], "Contract-only")

    def test_no_l1_outputs_and_phase3_gate_keeps_l1_excluded(self):
        candidate_paths = (
            ROOT / "results" / "raw",
            ROOT / "logs",
        )
        l1_artifacts = []
        for root in candidate_paths:
            if root.exists():
                l1_artifacts.extend(
                    path for path in root.iterdir() if "l1" in path.name.lower()
                )
        self.assertEqual(l1_artifacts, [])
        gate = ROOT / "reports" / "planning" / "phase3-gate-review.md"
        self.assertTrue(gate.exists())
        gate_text = gate.read_text(encoding="utf-8")
        self.assertIn("`L1 DT-SC-adapted`", gate_text)
        self.assertIn("excluded", gate_text)

    def test_phase3_progress_keeps_s311_completed(self):
        self.assertIn("| `S3.11` | `completed` |", self.progress)
        self.assertIn("## S3.11 L1 DT-SC literature adaptation profile decision", self.progress)


if __name__ == "__main__":
    unittest.main()
