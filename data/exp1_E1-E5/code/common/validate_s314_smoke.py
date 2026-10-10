#!/usr/bin/env python3
"""Validate immutable S3.14 legacy-pilot live-smoke evidence and manifests."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any, Mapping

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from phase3_contract import (  # noqa: E402
    ARCHITECTURE_APPLICABLE_METRICS,
    ContractValidationError,
    load_json,
    validate_instance,
)

EXP_ROOT = SCRIPT_DIR.parents[1]
CONFIG_PATH = EXP_ROOT / "configs/phase3/s314-smoke-v1.json"
DEV_PATH = EXP_ROOT / "configs/dev.json"
EVIDENCE_SCHEMA_PATH = EXP_ROOT / "schemas/phase3/s314-live-smoke.schema.json"
MANIFEST_SCHEMA_PATH = EXP_ROOT / "schemas/phase3/s314-run-manifest.schema.json"

SCENARIO_BY_OPERATION = {
    "TransferMeta": "LEG-META-01",
    "IssueRight": "LEG-ISSUE-01",
    "TransferRight": "LEG-RESALE-01",
}
AUTHORITY_BY_BASELINE = {"B1": "contract", "B2": "runtime", "G3": "runtime"}


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractValidationError(message)


def _field(value: Mapping[str, Any], camel: str, snake: str) -> Any:
    if camel in value:
        return value[camel]
    return value.get(snake)


def validate_evidence_semantics(
    evidence: Mapping[str, Any],
    config: Mapping[str, Any],
    dev: Mapping[str, Any],
) -> None:
    baseline = evidence["run"]["baseline_id"]
    _require(baseline in AUTHORITY_BY_BASELINE, "unknown S3.14 baseline")
    _require(evidence["status"] == "PASS", "live smoke did not pass")
    _require(evidence["run"]["validity_level"] == "legacy-pilot", "validity promotion is forbidden")
    _require(evidence["run"]["shared_input_hash"] == config["shared_input_hash"], "shared input hash drift")
    _require(evidence["run"]["seed"] == config["seed"], "seed drift")
    _require(evidence["shared_input"]["quantity"] == config["quantity"], "quantity drift")
    _require(evidence["shared_input"]["prices_plancks"] == config["prices_plancks"], "price drift")
    _require(
        evidence["shared_input"]["timing_offsets_blocks"] == config["timing_offsets_blocks"],
        "timing drift",
    )
    _require(
        evidence["shared_input"]["experiment_families"] == config["experiment_families"],
        "experiment-family drift",
    )
    _require(evidence["shared_input"]["scenario_ids"] == config["scenario_ids"], "scenario selection drift")

    roles = dev["shared_fixture"]["roles"]
    expected_accounts = {role: details["ss58_address"] for role, details in roles.items()}
    _require(evidence["accounts"] == expected_accounts, "shared role accounts drift")

    operations = evidence["operations"]
    _require(set(operations) == set(SCENARIO_BY_OPERATION), "three-operation smoke surface is not closed")
    for operation, scenario_id in SCENARIO_BY_OPERATION.items():
        result = operations[operation]
        _require(result["scenario_id"] == scenario_id, f"{operation} scenario mismatch")
        _require(result["expiry_offset"] == 20, f"{operation} expiry offset mismatch")
        _require(result["state"] == "Settled", f"{operation} did not settle")
        _require(result["settlement_record_count"] == 1, f"{operation} evidence count mismatch")
        _require(result["evidence_field_count"] == 24, f"{operation} frozen evidence fields mismatch")
        record = result["audit"]["record"]
        _require(_field(record, "operation", "operation") == operation, f"{operation} record operation mismatch")
        _require(
            str(_field(record, "grossPrice", "gross_price")) == config["prices_plancks"][operation],
            f"{operation} record price mismatch",
        )

    rejection = evidence["rejection"]
    _require(rejection["common_error_code"] == "PayerBuyerMismatch", "rejection vocabulary drift")
    _require(rejection["authoritative_state_unchanged"] is True, "rejection polluted authority")
    _require(rejection["success_event_count"] == 0, "rejection emitted success event")

    duplicate = evidence["duplicate"]
    _require(duplicate["common_error_code"] == "AlreadySettled", "duplicate vocabulary drift")
    _require(duplicate["accepted_attempt_count"] == 1, "duplicate accepted count mismatch")
    _require(duplicate["settlement_record_count"] == 1, "duplicate evidence count mismatch")
    _require(duplicate["second_success_event_count"] == 0, "duplicate emitted a second success")
    _require(duplicate["authoritative_state_unchanged"] is True, "duplicate polluted authority")

    audit = evidence["audit"]
    _require(audit["authoritative_source"] == AUTHORITY_BY_BASELINE[baseline], "authority source drift")
    _require(audit["contract_projection_used_for_authority"] is False, "contract projection used as authority")
    _require(audit["settled_operation_count"] == 3, "settled operation count mismatch")
    _require(audit["settlement_record_count"] == 3, "target settlement count mismatch")
    _require(audit["all_record_field_counts"] == [24, 24, 24], "evidence field-count mismatch")

    applicable = ARCHITECTURE_APPLICABLE_METRICS[baseline]
    metrics = evidence["architecture_cost"]
    for name, metric in metrics.items():
        availability = metric["availability"]
        value = metric["value"]
        if name in applicable:
            _require(availability != "N/A", f"{baseline}.{name} is applicable and cannot be N/A")
        else:
            _require(availability == "N/A", f"{baseline}.{name} must be structural N/A")
        if availability == "measured":
            _require(isinstance(value, str) and value.isdigit(), f"{baseline}.{name} measured value invalid")
        else:
            _require(value is None, f"{baseline}.{name} unavailable value must be null")


def validate_evidence(path: Path) -> Mapping[str, Any]:
    evidence = load_json(path)
    validate_instance(evidence, load_json(EVIDENCE_SCHEMA_PATH))
    validate_evidence_semantics(evidence, load_json(CONFIG_PATH), load_json(DEV_PATH))
    return evidence


def validate_manifest(path: Path, evidence: Mapping[str, Any]) -> Mapping[str, Any]:
    manifest = load_json(path)
    validate_instance(manifest, load_json(MANIFEST_SCHEMA_PATH))
    _require(manifest["status"] == "PASS", "manifest did not pass")
    _require(manifest["run_id"] == evidence["run"]["run_id"], "manifest/evidence run_id mismatch")
    _require(manifest["baseline_id"] == evidence["run"]["baseline_id"], "manifest/evidence baseline mismatch")
    _require(
        manifest["implementation"]["head"] == evidence["run"]["implementation_commit"],
        "manifest/evidence commit mismatch",
    )
    _require(manifest["implementation"]["dirty"] == evidence["run"]["git_dirty"], "dirty-state mismatch")
    _require(manifest["execution"]["rpc_ready"] is True, "RPC readiness was not confirmed")
    _require(manifest["formal_experiment_executed"] is False, "formal experiment is forbidden")
    _require(manifest["e6_executed"] is False, "E6 is forbidden")
    for artifact in manifest["build"]["artifacts"]:
        _require(Path(artifact["path"]).is_file(), f"build artifact missing: {artifact['path']}")
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", action="append", type=Path, required=True)
    parser.add_argument("--manifest", action="append", type=Path, default=[])
    args = parser.parse_args()
    manifests = {path.parent.name: path for path in args.manifest}
    validated: list[str] = []
    for evidence_path in args.evidence:
        evidence = validate_evidence(evidence_path)
        run_id = evidence["run"]["run_id"]
        if manifests:
            manifest_path = manifests.get(run_id)
            _require(manifest_path is not None, f"manifest missing for {run_id}")
            validate_manifest(manifest_path, evidence)
        validated.append(f"{evidence['run']['baseline_id']}:{run_id}")
    print(f"S3.14 smoke validation PASS: {', '.join(validated)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
