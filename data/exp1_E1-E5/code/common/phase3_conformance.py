"""Static S3.13 capability and semantic conformance gate.

The gate reads frozen planning sources and implementation declarations directly.
It creates no run, result, raw artifact, or experiment evidence.
"""

from __future__ import annotations

import importlib.util
import inspect
import re
from collections import Counter
from pathlib import Path
from types import ModuleType
from typing import Any, Mapping, Sequence

from phase3_contract import (
    ARCHITECTURE_APPLICABLE_METRICS,
    BASELINE_NAMES,
    BaselineAdapter,
    Phase,
    load_json,
    validate_contract_bundle,
    validate_instance,
)
from phase3_driver import (
    ARCHITECTURE_COST_UNITS,
    COMMON_COST_UNITS,
    Phase3ScenarioDriver,
)
from shared_fixture_codec import validate_bundle_semantics


GATE_VERSION = "phase3-capability-conformance-v1"
CONFIG_PATH = Path("configs/phase3/capability-conformance-v1.json")
CONFIG_SCHEMA_PATH = Path("schemas/phase3/capability-conformance.schema.json")
BASELINE_COLUMNS = ("B1", "B2", "G3", "L1", "L2")
SCENARIO_RE = re.compile(r"^(LEG|NEG|CLEAN|DUP|RACE)-[A-Z0-9-]+-[0-9]{2}$")
RUST_CATEGORY_PREFIX = {
    "Legal": "LEG",
    "Illegal": "NEG",
    "Clean": "CLEAN",
    "Duplicate": "DUP",
    "Race": "RACE",
}


class ConformanceError(ValueError):
    """Raised when a frozen declaration or implementation surface drifts."""


def _display(value: Any) -> Any:
    if isinstance(value, (set, frozenset)):
        return sorted(value)
    return value


def _expect_equal(actual: Any, expected: Any, label: str) -> None:
    if actual != expected:
        raise ConformanceError(
            f"{label} mismatch: expected {_display(expected)!r}, got {_display(actual)!r}"
        )


def _strip_code(value: str) -> str:
    value = value.strip()
    if value.startswith("`") and value.endswith("`"):
        return value[1:-1]
    return value


def _markdown_rows(text: str, id_pattern: str) -> dict[str, list[str]]:
    matcher = re.compile(id_pattern)
    rows: dict[str, list[str]] = {}
    for line in text.splitlines():
        if not line.lstrip().startswith("|"):
            continue
        parts = [item.strip() for item in line.strip().strip("|").split("|")]
        if not parts:
            continue
        row_id = _strip_code(parts[0])
        if matcher.fullmatch(row_id) is None:
            continue
        if row_id in rows:
            raise ConformanceError(f"duplicate frozen Markdown row {row_id}")
        rows[row_id] = parts
    return rows


def _matrix_labels(rows: Mapping[str, Sequence[str]]) -> dict[str, dict[str, str]]:
    result: dict[str, dict[str, str]] = {}
    for row_id, parts in rows.items():
        if len(parts) != 8:
            raise ConformanceError(
                f"{row_id} must have ID, description, five profiles, and boundary"
            )
        result[row_id] = {
            baseline: _strip_code(parts[index + 2])
            for index, baseline in enumerate(BASELINE_COLUMNS)
        }
    return result


def _expected_architecture_rows(
    groups: Mapping[str, Mapping[str, Sequence[str]]],
) -> dict[str, dict[str, str]]:
    rows = {f"A{index:02d}": {} for index in range(1, 16)}
    for baseline in BASELINE_COLUMNS:
        assigned: set[str] = set()
        for label, identifiers in groups[baseline].items():
            for identifier in identifiers:
                if identifier in assigned:
                    raise ConformanceError(
                        f"{baseline} assigns architecture capability {identifier} twice"
                    )
                if identifier not in rows:
                    raise ConformanceError(
                        f"{baseline} declares unknown architecture capability {identifier}"
                    )
                rows[identifier][baseline] = label
                assigned.add(identifier)
        _expect_equal(assigned, set(rows), f"{baseline} architecture capability IDs")
    return rows


def _expected_deferred_rows(
    groups: Mapping[str, Sequence[str]],
) -> dict[str, dict[str, str]]:
    rows: dict[str, dict[str, str]] = {}
    for label, identifiers in groups.items():
        for identifier in identifiers:
            if identifier in rows:
                raise ConformanceError(f"deferred capability {identifier} assigned twice")
            rows[identifier] = {baseline: label for baseline in BASELINE_COLUMNS}
    return rows


def _validate_capability_sources(
    exp_root: Path,
    config: Mapping[str, Any],
    common: Mapping[str, Any],
) -> dict[str, int]:
    sources = config["sources"]
    capability_text = (exp_root / sources["capability_matrix"]).read_text(
        encoding="utf-8"
    )
    traceability_text = (exp_root / sources["traceability_matrix"]).read_text(
        encoding="utf-8"
    )
    common_rows = _matrix_labels(_markdown_rows(capability_text, r"C[0-9]{2}"))
    architecture_rows = _matrix_labels(
        _markdown_rows(capability_text, r"A[0-9]{2}")
    )
    deferred_rows = _matrix_labels(_markdown_rows(capability_text, r"D[0-9]{2}"))

    common_ids = config["common_capabilities"]
    _expect_equal(common_ids, [f"C{index:02d}" for index in range(1, 25)], "C01-C24 order")
    _expect_equal(set(common_rows), set(common_ids), "frozen common capability rows")
    for capability_id, labels in common_rows.items():
        expected = {
            "B1": "Native",
            "B2": "Native",
            "G3": "Native",
            "L1": "Adapted",
            "L2": "Adapted",
        }
        _expect_equal(labels, expected, f"{capability_id} labels")

    expected_architecture = _expected_architecture_rows(
        config["architecture_capabilities"]
    )
    _expect_equal(
        architecture_rows,
        expected_architecture,
        "A01-A15 architecture labels",
    )
    expected_deferred = _expected_deferred_rows(config["deferred_capabilities"])
    _expect_equal(
        set(expected_deferred),
        {f"D{index:02d}" for index in range(1, 11)},
        "D01-D10 IDs",
    )
    _expect_equal(deferred_rows, expected_deferred, "D01-D10 deferred labels")

    observed_labels = {
        label
        for rows in (common_rows, architecture_rows, deferred_rows)
        for labels in rows.values()
        for label in labels.values()
    }
    _expect_equal(
        observed_labels,
        set(config["label_policy"]["matrix_labels"]),
        "frozen capability label vocabulary",
    )
    _expect_equal(config["label_policy"]["blocking_label"], "Blocked", "blocking label")
    _expect_equal(
        common["not_applicable_policy"]["missing_common_core_policy"],
        config["label_policy"]["missing_common_core"],
        "missing common-core gate label",
    )
    if not common["not_applicable_policy"]["common_core_may_not_be_not_applicable"]:
        raise ConformanceError("common core must forbid N/A")

    trace_c = set(_markdown_rows(traceability_text, r"C[0-9]{2}"))
    trace_a = set(_markdown_rows(traceability_text, r"A[0-9]{2}"))
    _expect_equal(trace_c, set(common_ids), "traceability C01-C24 rows")
    _expect_equal(trace_a, set(expected_architecture), "traceability A01-A15 rows")
    return {
        "common": len(common_rows),
        "architecture": len(architecture_rows),
        "deferred": len(deferred_rows),
    }


def _expected_scenarios(config: Mapping[str, Any]) -> tuple[list[str], dict[str, int]]:
    identifiers: list[str] = []
    counts: dict[str, int] = {}
    for category in ("LEG", "NEG", "CLEAN", "DUP", "RACE"):
        category_ids = list(config["scenarios"][category])
        if any(identifier.split("-", 1)[0] != category for identifier in category_ids):
            raise ConformanceError(f"{category} scenario list contains another category")
        identifiers.extend(category_ids)
        counts[category] = len(category_ids)
    if len(set(identifiers)) != len(identifiers):
        raise ConformanceError("configured scenario IDs are not unique")
    _expect_equal(len(identifiers), 42, "configured scenario count")
    _expect_equal(counts, {"LEG": 6, "NEG": 24, "CLEAN": 5, "DUP": 3, "RACE": 4}, "scenario category counts")
    return identifiers, counts


def _rust_scenario_specs(source: str) -> dict[str, tuple[str, str | None]]:
    pattern = re.compile(
        r'id:\s*"(?P<id>(?:LEG|NEG|CLEAN|DUP|RACE)-[A-Z0-9-]+-[0-9]{2})"\s*,'
        r'\s*category:\s*(?:S38)?Category::(?P<category>[A-Za-z]+)'
        r'.*?expected_error:\s*(?P<error>None|Some\((?:crate::)?CommonErrorCode::[A-Za-z0-9_]+\))',
        re.DOTALL,
    )
    specs: dict[str, tuple[str, str | None]] = {}
    for match in pattern.finditer(source):
        identifier = match.group("id")
        if identifier in specs:
            raise ConformanceError(f"duplicate Rust scenario registry entry {identifier}")
        error_value = match.group("error")
        error = None
        if error_value != "None":
            error = error_value.rsplit("::", 1)[1].rstrip(")")
        specs[identifier] = (match.group("category"), error)
    return specs


def _scenario_matrix_errors(
    rows: Mapping[str, Sequence[str]], common_errors: Sequence[str]
) -> dict[str, str | None]:
    expected: dict[str, str | None] = {}
    for identifier, parts in rows.items():
        if len(parts) != 7:
            raise ConformanceError(f"scenario {identifier} must have seven matrix columns")
        line = parts[4]
        matches = [error for error in common_errors if re.search(rf"\b{re.escape(error)}\b", line)]
        if len(matches) > 1:
            raise ConformanceError(
                f"scenario {identifier} declares multiple common errors {matches}"
            )
        expected[identifier] = matches[0] if matches else None
    return expected


def _validate_scenarios(
    exp_root: Path,
    config: Mapping[str, Any],
    common_errors: Sequence[str],
    manifests: Mapping[str, Mapping[str, Any]],
) -> tuple[dict[str, int], dict[str, dict[str, str | None]]]:
    expected_ids, counts = _expected_scenarios(config)
    matrix_text = (exp_root / config["sources"]["scenario_matrix"]).read_text(
        encoding="utf-8"
    )
    matrix_rows = _markdown_rows(
        matrix_text,
        r"(?:LEG|NEG|CLEAN|DUP|RACE)-[A-Z0-9-]+-[0-9]{2}",
    )
    _expect_equal(set(matrix_rows), set(expected_ids), "frozen 42-scenario matrix")
    matrix_counts = Counter(identifier.split("-", 1)[0] for identifier in matrix_rows)
    _expect_equal(dict(matrix_counts), counts, "frozen scenario category counts")
    matrix_errors = _scenario_matrix_errors(matrix_rows, common_errors)

    baseline_errors: dict[str, dict[str, str | None]] = {}
    for baseline_id, baseline_config in config["baselines"].items():
        worktree = Path(manifests[baseline_id]["git"]["worktree"])
        source_path = worktree / baseline_config["scenario_source"]
        specs = _rust_scenario_specs(source_path.read_text(encoding="utf-8"))
        _expect_equal(set(specs), set(expected_ids), f"{baseline_id} Rust scenario IDs")
        observed_counts = Counter(
            RUST_CATEGORY_PREFIX[category] for category, _ in specs.values()
        )
        _expect_equal(
            dict(observed_counts), counts, f"{baseline_id} Rust scenario categories"
        )
        errors = {identifier: error for identifier, (_, error) in specs.items()}
        _expect_equal(errors, matrix_errors, f"{baseline_id} scenario error mapping")
        baseline_errors[baseline_id] = errors
    return counts, baseline_errors


def _load_module(path: Path, name: str) -> ModuleType:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ConformanceError(f"cannot import adapter module {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _rust_enum_body(source: str, enum_name: str) -> str:
    match = re.search(
        rf"pub\s+enum\s+{re.escape(enum_name)}(?:<[^>]+>)?[^{{]*\{{",
        source,
    )
    if match is None:
        raise ConformanceError(f"Rust enum {enum_name} was not found")
    start = match.end()
    depth = 1
    for index in range(start, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[start:index]
    raise ConformanceError(f"Rust enum {enum_name} is not closed")


def _rust_protocol_errors(source: str) -> list[tuple[int, str]]:
    body = _rust_enum_body(source, "CommonErrorCode")
    values = [
        (int(status), name)
        for name, status in re.findall(r"^\s*([A-Za-z0-9_]+)\s*=\s*([0-9]+),", body, re.MULTILINE)
    ]
    return sorted(values)


def _rust_native_errors(source: str) -> set[str]:
    body = _rust_enum_body(source, "Error")
    return set(re.findall(r"^\s*([A-Za-z0-9_]+),\s*$", body, re.MULTILINE))


def _validate_errors_and_adapters(
    exp_root: Path,
    config: Mapping[str, Any],
    common_errors: Sequence[str],
    manifests: Mapping[str, Mapping[str, Any]],
    projection_fields: set[str],
) -> dict[str, int]:
    expected_protocol = list(enumerate(common_errors, start=1))
    mapping_counts: dict[str, int] = {}
    phases = config["normalized_contract"]["phases"]
    for baseline_id, baseline_config in config["baselines"].items():
        manifest = manifests[baseline_id]
        _expect_equal(
            manifest["authority"]["normalized_authority_source"],
            baseline_config["authority_source"],
            f"{baseline_id} normalized authority",
        )
        expected_projection_role = (
            "not-applicable"
            if baseline_config["projection_mode"] == "N/A"
            else baseline_config["projection_mode"]
        )
        _expect_equal(
            manifest["authority"]["projection_role"],
            expected_projection_role,
            f"{baseline_id} projection role",
        )
        module_path = exp_root / baseline_config["adapter_module"]
        module = _load_module(module_path, f"_phase3_conformance_{baseline_id.lower()}")
        adapter_class = getattr(module, baseline_config["adapter_class"], None)
        if not inspect.isclass(adapter_class) or not issubclass(adapter_class, BaselineAdapter):
            raise ConformanceError(f"{baseline_id} adapter does not implement BaselineAdapter")
        if inspect.isabstract(adapter_class):
            raise ConformanceError(f"{baseline_id} adapter leaves a phase abstract")
        for phase in phases:
            if not callable(getattr(adapter_class, phase, None)):
                raise ConformanceError(f"{baseline_id} adapter omits phase {phase}")

        worktree = Path(manifest["git"]["worktree"])
        protocol_source = (
            worktree / baseline_config["protocol_error_source"]
        ).read_text(encoding="utf-8")
        _expect_equal(
            _rust_protocol_errors(protocol_source),
            expected_protocol,
            f"{baseline_id} Rust common-error status order",
        )

        native_source = baseline_config["native_error_source"]
        if native_source is not None:
            native_errors = _rust_native_errors(
                (worktree / native_source).read_text(encoding="utf-8")
            )
            mapping = getattr(module, "NATIVE_ERROR_TO_COMMON", None)
            if not isinstance(mapping, dict):
                raise ConformanceError(f"{baseline_id} has no native error map")
            _expect_equal(set(mapping), native_errors, f"{baseline_id} total native-error map")
            if not set(mapping.values()).issubset(set(common_errors)):
                raise ConformanceError(f"{baseline_id} maps to an unfrozen common error")
            mapping_counts[baseline_id] = len(mapping)
        else:
            _expect_equal(
                list(getattr(module, "_COMMON_ERROR_ORDER", ())),
                list(common_errors),
                "G3 common-error order",
            )
            _expect_equal(
                getattr(module, "COMMON_ERROR_BY_STATUS", {}),
                dict(expected_protocol),
                "G3 CE v2 status map",
            )
            _expect_equal(
                set(getattr(module, "COMMON_ERROR_CODES", ())),
                set(common_errors),
                "G3 common-error vocabulary",
            )
            _expect_equal(
                set(getattr(module, "_PROJECTION_FIELDS", ())),
                projection_fields,
                "G3 projection field boundary",
            )
            mapping_counts[baseline_id] = len(common_errors)
    return mapping_counts


def _validate_schema_contract(
    config: Mapping[str, Any],
    common: Mapping[str, Any],
    invocation_schema: Mapping[str, Any],
    snapshot_schema: Mapping[str, Any],
    result_schema: Mapping[str, Any],
) -> set[str]:
    normalized = config["normalized_contract"]
    phases = normalized["phases"]
    _expect_equal(common["phase_contract"]["allowed_phases"], phases, "common phase order")
    _expect_equal([phase.value for phase in Phase], phases, "driver phase enum")
    _expect_equal(
        invocation_schema["properties"]["phase"]["enum"], phases, "invocation phase enum"
    )
    _expect_equal(invocation_schema["required"], normalized["invocation_required"], "invocation fields")
    _expect_equal(snapshot_schema["required"], normalized["snapshot_required"], "snapshot fields")
    _expect_equal(result_schema["required"], normalized["result_required"], "result fields")
    for label, schema in (
        ("invocation", invocation_schema),
        ("snapshot", snapshot_schema),
        ("result", result_schema),
    ):
        if schema.get("additionalProperties") is not False:
            raise ConformanceError(f"{label} schema must reject unknown top-level fields")
        _validate_closed_schema_objects(schema, label)

    common_cost = normalized["common_cost_fields"]
    architecture_cost = normalized["architecture_cost_fields"]
    _expect_equal(list(COMMON_COST_UNITS), common_cost, "driver common-cost fields")
    _expect_equal(
        result_schema["properties"]["cost"]["required"], common_cost, "result common-cost fields"
    )
    _expect_equal(
        list(ARCHITECTURE_COST_UNITS), architecture_cost, "driver architecture-cost fields"
    )
    _expect_equal(
        result_schema["properties"]["architecture_cost"]["required"],
        architecture_cost,
        "result architecture-cost fields",
    )
    _expect_equal(
        result_schema["properties"]["run"]["properties"]["baseline_id"]["enum"],
        list(BASELINE_NAMES),
        "main result baseline IDs",
    )
    projection_schema = snapshot_schema["properties"]["projection"]
    projection_fields = set(projection_schema["required"])
    _expect_equal(
        projection_fields,
        {"availability", "authoritative", "order_state", "settlement_record_count"},
        "projection fields",
    )

    all_architecture_metrics = set(architecture_cost)
    for baseline_id, baseline_config in config["baselines"].items():
        applicable = set(baseline_config["applicable_architecture_metrics"])
        _expect_equal(
            set(ARCHITECTURE_APPLICABLE_METRICS[baseline_id]),
            applicable,
            f"{baseline_id} architecture metric applicability",
        )
        package = Phase3ScenarioDriver._cost_package(
            {},
            ARCHITECTURE_COST_UNITS,
            applicable=applicable,
            label=f"{baseline_id} conformance probe",
        )
        for name, metric in package.items():
            expected_availability = "not-collected" if name in applicable else "N/A"
            _expect_equal(
                metric,
                {
                    "availability": expected_availability,
                    "value": None,
                    "unit": ARCHITECTURE_COST_UNITS[name],
                },
                f"{baseline_id}.{name} unobserved availability",
            )
        _expect_equal(set(package), all_architecture_metrics, f"{baseline_id} architecture fields")
    return projection_fields


def _validate_closed_schema_objects(schema: Any, path: str) -> None:
    if isinstance(schema, list):
        for index, child in enumerate(schema):
            _validate_closed_schema_objects(child, f"{path}[{index}]")
        return
    if not isinstance(schema, Mapping):
        return
    properties = schema.get("properties")
    if schema.get("additionalProperties") is False and isinstance(properties, Mapping):
        _expect_equal(
            set(schema.get("required", ())),
            set(properties),
            f"{path} closed required fields",
        )
    for key, child in schema.items():
        _validate_closed_schema_objects(child, f"{path}.{key}")


def _validate_fixture(
    exp_root: Path,
    config: Mapping[str, Any],
    common: Mapping[str, Any],
) -> str:
    bundle = load_json(exp_root / config["sources"]["fixture_bundle"])
    validate_bundle_semantics(bundle, exp_root)
    fixture_config = config["fixture"]
    _expect_equal(len(bundle["vectors"]), fixture_config["expected_operation_count"], "fixture operation count")
    _expect_equal(
        [item["operation"] for item in bundle["vectors"]],
        common["phase_contract"]["allowed_operations"],
        "fixture operation order",
    )
    error_names = [item["name"] for item in bundle["common_errors"]]
    _expect_equal(error_names, common["common_error_codes"], "fixture common errors")
    _expect_equal(len(error_names), fixture_config["expected_error_count"], "fixture error count")
    _expect_equal(
        [item["status_code"] for item in bundle["common_errors"]],
        list(range(1, fixture_config["expected_error_count"] + 1)),
        "fixture error status sequence",
    )
    consumers = bundle["baseline_consumers"]
    _expect_equal(set(consumers), set(BASELINE_NAMES), "fixture baseline consumers")
    hashes = {item["shared_input_hash"] for item in consumers.values()}
    _expect_equal(hashes, {fixture_config["expected_consumer_hash"]}, "fixture consumer hash")
    return fixture_config["expected_consumer_hash"]


def _validate_literature_auxiliaries(
    exp_root: Path,
    config: Mapping[str, Any],
    manifests: Mapping[str, Mapping[str, Any]],
) -> dict[str, dict[str, Any]]:
    result: dict[str, dict[str, Any]] = {}
    all_common = set(config["common_capabilities"])
    for literature_id, profile in config["literature_auxiliaries"].items():
        _expect_equal(profile["status"], "excluded", f"{literature_id} status")
        subset = list(profile["declared_common_subset"])
        if not set(subset).issubset(all_common):
            raise ConformanceError(f"{literature_id} declares an unknown common capability")
        _expect_equal(subset, [], f"{literature_id} executable common subset")
        review_text = (exp_root / profile["review"]).read_text(encoding="utf-8")
        if profile["decision_marker"] not in review_text:
            raise ConformanceError(f"{literature_id} exclusion decision marker is missing")
        if "profile/config created: `no`" not in review_text or "run ID created: `no`" not in review_text:
            raise ConformanceError(f"{literature_id} review does not close executable identity")
        main_id = profile["must_not_alias"]
        if profile["name"] in {
            manifests[main_id]["baseline_name"],
            manifests[main_id]["baseline_id"],
        }:
            raise ConformanceError(f"{literature_id} aliases {main_id}")

        tokens = (
            ("l1", "dt-sc") if literature_id == "L1" else ("l2", "datamarket")
        )
        config_aliases = [
            str(path.relative_to(exp_root))
            for path in (exp_root / "configs").rglob("*.json")
            if any(token in path.name.lower() for token in tokens)
        ]
        _expect_equal(config_aliases, [], f"{literature_id} executable configs")
        output_aliases = []
        for output_root in (exp_root / "results" / "raw", exp_root / "logs"):
            if output_root.exists():
                output_aliases.extend(
                    str(path.relative_to(exp_root))
                    for path in output_root.rglob("*")
                    if any(token in path.name.lower() for token in tokens)
                )
        _expect_equal(output_aliases, [], f"{literature_id} run outputs")
        result[literature_id] = {
            "status": "excluded",
            "declared_common_subset_count": 0,
            "aggregated_with_main_experiment": False,
        }
    return result


def _validate_phase3_gate_state(
    progress_text: str,
    gate_text: str | None,
) -> str:
    completed_marker = "| `S3.15` | `completed` |"
    if gate_text is None:
        if completed_marker in progress_text:
            raise ConformanceError("S3.15 is completed without a Phase 3 gate review")
        return "pending"
    if completed_marker not in progress_text:
        raise ConformanceError("Phase 3 gate exists before S3.15 completion")
    for marker in (
        "`PHASE_3_GATE = PASS`",
        "`READY_FOR_PHASE_4 = true`",
        "`L1 DT-SC-adapted`",
        "`L2 DataMarket-Pallet-adapted`",
        "excluded",
        "`legacy-pilot`",
    ):
        if marker not in gate_text:
            raise ConformanceError(f"Phase 3 gate missing marker: {marker}")
    return "completed"


def validate_phase3_conformance(
    exp_root: Path,
    *,
    check_git: bool = True,
) -> dict[str, Any]:
    """Validate all S3.13 static gates and return a non-evidence summary."""

    exp_root = exp_root.resolve()
    config = load_json(exp_root / CONFIG_PATH)
    config_schema = load_json(exp_root / CONFIG_SCHEMA_PATH)
    validate_instance(config, config_schema)
    _expect_equal(config["gate_version"], GATE_VERSION, "conformance gate version")

    progress_path = exp_root / "reports/planning/phase3-progress.md"
    gate_path = exp_root / "reports/planning/phase3-gate-review.md"
    progress_text = progress_path.read_text(encoding="utf-8")
    gate_text = gate_path.read_text(encoding="utf-8") if gate_path.exists() else None
    phase3_gate_state = _validate_phase3_gate_state(progress_text, gate_text)

    validate_contract_bundle(exp_root, check_git=check_git)
    common = load_json(exp_root / config["sources"]["common_contract"])
    _expect_equal(
        config["validity_level"],
        common["implementation_boundary"]["allowed_validity_level"],
        "S3.13 validity boundary",
    )
    if common["implementation_boundary"]["formal_experiment_authorized"]:
        raise ConformanceError("S3.13 must not authorize formal experiments")

    manifests = {
        baseline_id: load_json(exp_root / baseline_config["manifest"])
        for baseline_id, baseline_config in config["baselines"].items()
    }
    invocation_schema = load_json(exp_root / config["sources"]["invocation_schema"])
    snapshot_schema = load_json(exp_root / config["sources"]["snapshot_schema"])
    result_schema = load_json(exp_root / config["sources"]["result_schema"])

    capability_counts = _validate_capability_sources(exp_root, config, common)
    projection_fields = _validate_schema_contract(
        config,
        common,
        invocation_schema,
        snapshot_schema,
        result_schema,
    )
    scenario_counts, _ = _validate_scenarios(
        exp_root,
        config,
        common["common_error_codes"],
        manifests,
    )
    mapping_counts = _validate_errors_and_adapters(
        exp_root,
        config,
        common["common_error_codes"],
        manifests,
        projection_fields,
    )
    fixture_hash = _validate_fixture(exp_root, config, common)
    literature = _validate_literature_auxiliaries(exp_root, config, manifests)

    return {
        "status": "PASS",
        "gate_version": GATE_VERSION,
        "phase3_gate_state": phase3_gate_state,
        "validity_level": config["validity_level"],
        "main_experiment": {
            "baselines": list(BASELINE_NAMES),
            "capability_rows": capability_counts,
            "scenario_count": sum(scenario_counts.values()),
            "scenario_category_counts": scenario_counts,
            "phase_count": len(config["normalized_contract"]["phases"]),
            "common_error_count": len(common["common_error_codes"]),
            "native_or_status_mapping_counts": mapping_counts,
            "fixture_consumer_hash": fixture_hash,
            "unexplained_mismatch_count": 0,
        },
        "literature_auxiliaries": literature,
        "claim_boundary": "static build/test conformance only; no smoke or experiment evidence",
    }
