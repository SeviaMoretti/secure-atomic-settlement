#!/usr/bin/env python3
"""Versioned R3 X1 launcher with the frozen driver output contract."""

from __future__ import annotations

from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from scripts.r3 import run_x1_formal as formal


_run = formal.run


def run(command: list[str], cwd: Path) -> None:
    """Adapt the driver's directory output contract without changing the driver."""
    adjusted = list(command)
    if "scripts/r2/x1_driver.py" in adjusted:
        try:
            out_index = adjusted.index("--out")
            output_path = Path(adjusted[out_index + 1])
        except (ValueError, IndexError):
            output_path = None
        if output_path is not None and output_path.name == "result.json":
            adjusted[out_index + 1] = str(output_path.parent)
    _run(adjusted, cwd)


formal.run = run


if __name__ == "__main__":
    raise SystemExit(formal.main())
