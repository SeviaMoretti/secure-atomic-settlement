#!/usr/bin/env python3
"""Run the S3.13 static capability and semantic conformance gate."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from phase3_conformance import ConformanceError, validate_phase3_conformance


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
        help="validate static conformance without reinspecting worktree identities",
    )
    args = parser.parse_args()
    try:
        result = validate_phase3_conformance(
            args.exp_root,
            check_git=not args.no_git,
        )
    except (ConformanceError, OSError, ValueError) as error:
        print(json.dumps({"status": "FAIL", "error": str(error)}, indent=2))
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
