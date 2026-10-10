import json
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[3]
REVIEW = ROOT / "reports" / "planning" / "l2-datamarket-pallet-source-review.md"
PROGRESS = ROOT / "reports" / "planning" / "phase3-progress.md"


class L2DataMarketPalletSourceReviewTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.text = REVIEW.read_text(encoding="utf-8")
        cls.progress = PROGRESS.read_text(encoding="utf-8")

    def test_source_identity_and_provenance_limit_are_explicit(self):
        required = (
            "No unique paper, published version, section set, or upstream implementation",
            "`pallet-market-runtime`",
            "`AssetXChain Project`",
            "cf3859eeadfc7286a1f0656a148a9fa245f82605",
            "393bc52af495ccd72ff883059d7715cbb153f964",
            "https://github.com/SeviaMoretti/assetxchain",
            "An original mechanism summary cannot be stated",
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
            "## 6. External-Validity Boundary",
            "## 7. Conditions to Activate L2 Later",
        ):
            with self.subTest(heading=heading):
                self.assertEqual(self.text.count(heading), 1)

        for boundary in (
            "Runtime/pallet domain authority",
            "No market contract or typed CE",
            "G3 contract market and typed CE path",
            "`Blocked/Missing`",
        ):
            with self.subTest(boundary=boundary):
                self.assertIn(boundary, self.text)

    def test_current_decision_is_exclusion_without_runnable_l2_config(self):
        self.assertIn("Decision: `EXCLUDED_FROM_CURRENT_AUXILIARY_RUN`", self.text)
        self.assertIn("Executable L2 profile/config created: `no`", self.text)
        self.assertIn("L2 run ID created: `no`", self.text)
        self.assertIn("Current validity level: `legacy-pilot`", self.text)
        self.assertIn("`B2 Runtime-only` is not L2", self.text)

        config_files = [
            path
            for path in (ROOT / "configs").rglob("*.json")
            if "l2" in path.name.lower() or "datamarket" in path.name.lower()
        ]
        self.assertEqual(config_files, [])

        b2 = json.loads((ROOT / "configs" / "baselines" / "b2.json").read_text())
        self.assertEqual(b2["baseline_id"], "B2")
        self.assertEqual(b2["baseline_name"], "Runtime-only")

    def test_no_l2_outputs_and_phase3_gate_keeps_l2_excluded(self):
        l2_artifacts = []
        for root in (ROOT / "results" / "raw", ROOT / "logs"):
            if root.exists():
                l2_artifacts.extend(
                    path
                    for path in root.iterdir()
                    if "l2" in path.name.lower() or "datamarket" in path.name.lower()
                )
        self.assertEqual(l2_artifacts, [])
        gate = ROOT / "reports" / "planning" / "phase3-gate-review.md"
        self.assertTrue(gate.exists())
        gate_text = gate.read_text(encoding="utf-8")
        self.assertIn("`L2 DataMarket-Pallet-adapted`", gate_text)
        self.assertIn("excluded", gate_text)

    def test_phase3_progress_keeps_s312_completed(self):
        self.assertIn("| `S3.12` | `completed` |", self.progress)
        self.assertIn(
            "## S3.12 L2 DataMarket-Pallet literature adaptation profile decision",
            self.progress,
        )


if __name__ == "__main__":
    unittest.main()
