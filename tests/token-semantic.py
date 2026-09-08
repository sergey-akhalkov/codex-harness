"""Prepare and verify the owned token-semantic Python fixture for task 3.2.

The helper writes one noisy pricing module under an owned TEMP root and runs a
deterministic oracle against discounted_total / invoice_total. It does not talk
to MCP servers or index source.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

PREFIX = "token-semantic-"
MODULE = "pricing.py"
BUGGY = "return round(subtotal * rate, 2)"
FIXED = "return round(subtotal * (1 - rate), 2)"
CASES = [
    {"expr": "discounted_total(100.0, 0.10)", "expected": 90.0},
    {"expr": "discounted_total(50.0, 0.0)", "expected": 50.0},
    {"expr": "invoice_total([10.0, 30.0], 0.25)", "expected": 30.0},
]


def fixture_source() -> str:
    padding = []
    for index in range(1, 21):
        padding.append(
            f'def padding_{index:02d}(value: int) -> int:\n'
            f'    """Unrelated helper kept only as retrieval noise."""\n'
            f"    total = 0\n"
            f"    for offset in range(3):\n"
            f"        total += (value + offset) * {index}\n"
            f"    return total\n"
        )
    return (
        '"""Owned pricing fixture with one defective discount helper."""\n\n'
        "def discounted_total(subtotal: float, rate: float) -> float:\n"
        '    """Apply a fractional discount rate to a subtotal."""\n'
        f"    {BUGGY}\n\n"
        "def invoice_total(lines: list[float], rate: float) -> float:\n"
        '    """Sum invoice lines, then apply discounted_total."""\n'
        "    return discounted_total(sum(lines), rate)\n\n"
        + "\n".join(padding)
    )


def write_project(root: Path) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    module = root / MODULE
    module.write_text(fixture_source(), encoding="utf-8", newline="\n")
    serena = root / ".serena"
    serena.mkdir(exist_ok=True)
    (serena / "project.yml").write_text(
        f"project_name: {root.name}\nlanguage_servers: [python]\nencoding: utf-8\n",
        encoding="utf-8",
        newline="\n",
    )
    (root / "evidence").mkdir(exist_ok=True)
    return module


def owned_temp_root() -> Path:
    return Path(tempfile.gettempdir()).resolve()


def require_new_owned_root(root: Path | None) -> Path:
    temp_root = owned_temp_root()
    if root is None:
        created = Path(tempfile.mkdtemp(prefix=PREFIX, dir=temp_root))
        return created.resolve()
    candidate = root.expanduser().resolve()
    if candidate.exists():
        raise ValueError(f"refusing existing root: {candidate}")
    try:
        candidate.relative_to(temp_root)
    except ValueError as exc:
        raise ValueError(f"root must be under {temp_root}: {candidate}") from exc
    if not candidate.name.startswith(PREFIX):
        raise ValueError(f"root name must start with {PREFIX}: {candidate.name}")
    if candidate.parent != temp_root:
        raise ValueError(f"root must be a direct child of {temp_root}: {candidate}")
    candidate.mkdir(parents=False, exist_ok=False)
    return candidate


def prepare(root: Path | None) -> dict:
    root = require_new_owned_root(root)
    module = write_project(root)
    payload = {
        "root": str(root),
        "module": str(module),
        "bytes": module.stat().st_size,
        "lines": module.read_text(encoding="utf-8").count("\n"),
        "sha256": hashlib.sha256(module.read_bytes()).hexdigest(),
        "python": sys.executable,
        "buggy": BUGGY,
        "fixed": FIXED,
    }
    (root / "evidence" / "fixture.json").write_text(
        json.dumps(payload, indent=2) + "\n", encoding="utf-8"
    )
    return payload


def oracle(root: Path) -> dict:
    module = root / MODULE
    source = module.read_text(encoding="utf-8")
    line = FIXED if FIXED in source else BUGGY if BUGGY in source else None
    script = (
        "from pricing import discounted_total, invoice_total\n"
        "print(json.dumps(["
        + ", ".join(case["expr"] for case in CASES)
        + "]))\n"
    )
    completed = subprocess.run(
        [sys.executable, "-B", "-c", "import json\n" + script],
        cwd=root,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    observed = []
    if completed.returncode == 0 and completed.stdout.strip():
        observed = json.loads(completed.stdout)
    results = []
    complete = (
        completed.returncode == 0
        and isinstance(observed, list)
        and len(observed) == len(CASES)
    )
    ok = complete
    values = observed if complete else [None] * len(CASES)
    for case, value in zip(CASES, values):
        match = value == case["expected"]
        ok = ok and match
        results.append({**case, "observed": value, "ok": match})
    return {
        "root": str(root),
        "exit": completed.returncode,
        "stderr": completed.stderr.strip(),
        "discount_line": line,
        "fixed": line == FIXED,
        "ok": ok,
        "observed_count": len(observed) if isinstance(observed, list) else 0,
        "cases": results,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--prepare", action="store_true")
    parser.add_argument("--oracle", action="store_true")
    parser.add_argument("--root")
    args = parser.parse_args(argv)
    if args.prepare:
        try:
            payload = prepare(Path(args.root) if args.root else None)
        except ValueError as exc:
            print(json.dumps({"ok": False, "error": str(exc)}, indent=2), file=sys.stderr)
            return 2
        print(json.dumps(payload, indent=2))
        return 0
    if args.oracle:
        if not args.root:
            parser.error("--oracle requires --root")
        payload = oracle(Path(args.root))
        print(json.dumps(payload, indent=2))
        return 0 if payload["ok"] else 1
    parser.error("pass --prepare or --oracle")
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
