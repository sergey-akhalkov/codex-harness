from pathlib import Path
path = Path(r"D:/home/sergey-akhalkov/codex-harness/docs/evidence/subscription-usage.md")
text = path.read_text(encoding="utf-8")
header = (
    "# Subscription usage from existing local rollouts\n\n"
    "[Documentation map](../README.md) · "
    "[User decision](../project-decisions.md#расход-подписки-и-условная-автоматизация) · "
    "[Optimization plan](../../openspec/changes/reduce-subscription-waste/proposal.md)\n\n"
    "Verified on 2026-09-08 from existing host rollouts. No new model experiment was\n"
    "created. Reporter: [tools/delegation-usage.py](../../tools/delegation-usage.py).\n"
    "Checks: python -B tests/delegation-usage.py (27 tests). Runtime: Python 3.13.14\n"
    "from .venv, native CLI 0.153.3/0.153.4. Private hashes:\n"
    "~/.codex/harness/verification/subscription-usage/sources.json.\n\n"
)
if text.startswith("# Subscription usage from existing local rollouts"):
    rest = text.split("\n", 1)[1]
    path.write_text(header + rest.lstrip("\n"), encoding="utf-8")
    print("updated")
else:
    raise SystemExit("unexpected start")
