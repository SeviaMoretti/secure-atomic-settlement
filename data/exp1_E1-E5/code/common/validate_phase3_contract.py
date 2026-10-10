#!/usr/bin/env python3
"""Validate the S3.1 manifests, schemas, frozen fixture, and Git isolation."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from phase3_contract import ContractValidationError, validate_contract_bundle


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--exp-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
    )
    parser.add_argument(
        "--no-git",
        action="store_true",
        help="validate static files without inspecting worktrees",
    )
    args = parser.parse_args()
    try:
        result = validate_contract_bundle(args.exp_root.resolve(), check_git=not args.no_git)
    except ContractValidationError as error:
        print(json.dumps({"status": "FAIL", "error": str(error)}, indent=2))
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
