#!/usr/bin/env python3
"""Generate or verify the canonical Phase 3 shared fixture/vector bundle."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from phase3_contract import load_json, validate_instance
from shared_fixture_codec import build_bundle, validate_bundle_semantics


DEFAULT_OUTPUT = Path("fixtures/phase3/assetxchain-paper2-shared-fixture-v1.json")
DEFAULT_SCHEMA = Path("schemas/phase3/shared-fixture-vectors.schema.json")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exp-root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--check", action="store_true", help="verify without writing")
    return parser.parse_args()


def resolve_under(root: Path, path: Path) -> Path:
    return path if path.is_absolute() else root / path


def main() -> int:
    args = parse_args()
    exp_root = args.exp_root.resolve()
    output = resolve_under(exp_root, args.output)
    schema = load_json(exp_root / DEFAULT_SCHEMA)
    generated = build_bundle(exp_root)

    if args.check:
        existing = load_json(output)
        if existing != generated:
            raise SystemExit(f"FAIL: {output} is stale; regenerate it")
        bundle = existing
        action = "checked"
    else:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(
            json.dumps(generated, indent=2, ensure_ascii=True) + "\n",
            encoding="utf-8",
        )
        bundle = load_json(output)
        action = "generated"

    validate_instance(bundle, schema)
    validate_bundle_semantics(bundle, exp_root)
    print(
        f"PASS: {action} {bundle['vector_bundle_id']} "
        f"({len(bundle['vectors'])} operations, "
        f"{len(bundle['common_errors'])} errors, "
        f"{len(bundle['baseline_consumers'])} consumers)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
