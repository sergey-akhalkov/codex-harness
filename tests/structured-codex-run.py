"""Exercise the real helper CLI against deterministic owned process fixtures.

Creates only temporary Git repositories and observer evidence. No models, network,
shared-service restart or foreign writes. Retains all receipts and inputs.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

REPO = Path(__file__).resolve().parents[1]
RUNNER = REPO / ".agents/skills/structured-codex-run/scripts/run.py"
FIXTURE = REPO / "tests/fixtures/structured-codex.py"
ROOT = Path(tempfile.mkdtemp(prefix="structured-checks-")).resolve()
PROMPT = "Inspect input.txt. Literal $() `quotes` 'single' \"double\" ; & Unicode: проверка\nSecond line."


class StructuredChecks(unittest.TestCase):
    def run_case(self, mode, expected, *, timeout=15, output_limit=1048576):
        case = ROOT / mode
        case.mkdir()
        subprocess.run(["git", "init", "-q", str(case)], check=True)
        subprocess.run(["git", "-C", str(case), "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                        "commit", "--allow-empty", "-qm", "fixture"], check=True)
        source = case / "input.txt"
        source.write_text("independent expected fact", encoding="utf-8")
        before = hashlib.sha256(source.read_bytes()).hexdigest()
        prompt = ROOT / (mode + "-prompt.txt")
        prompt.write_text(PROMPT, encoding="utf-8")
        argv = [sys.executable, str(RUNNER), "--cwd", str(case), "--prompt-file", str(prompt),
                "--command-json", json.dumps([sys.executable, str(FIXTURE), mode]),
                "--oracle-json", json.dumps([sys.executable, str(FIXTURE), "oracle"]),
                "--model", "fixture-model", "--provider", "fixture-provider", "--subscription", "deterministic-no-model",
                "--input", "input.txt", "--timeout", str(timeout), "--output-limit", str(output_limit)]
        completed = subprocess.run(argv, capture_output=True, text=True, encoding="utf-8", timeout=timeout + 90)
        (ROOT / (mode + "-cli.txt")).write_text(completed.stdout + completed.stderr, encoding="utf-8")
        self.assertIn(completed.returncode, (0, 1), completed.stderr)
        result = json.loads(completed.stdout)
        (ROOT / (mode + "-result.json")).write_text(json.dumps(result, indent=2), encoding="utf-8")
        self.assertEqual(result["status"], expected, result)
        self.assertEqual(completed.returncode, 0 if expected == "success" else 1)
        evidence = Path(result["evidence_root"])
        self.assertTrue((evidence / "process.json").is_file())
        self.assertTrue((evidence / "events.jsonl").is_file())
        self.assertTrue((evidence / "stderr.txt").is_file())
        self.assertNotEqual(evidence, case)
        capture = json.loads((evidence / "captured.json").read_text(encoding="utf-8"))
        self.assertTrue(capture["stdin"].startswith(PROMPT + "\n\n"))
        self.assertNotIn(PROMPT, capture["argv"])
        self.assertEqual(capture["argv"][-1], "-")
        self.assertEqual(capture["argv"][capture["argv"].index("--sandbox") + 1], "read-only")
        self.assertEqual(Path(capture["argv"][capture["argv"].index("--output-last-message") + 1]), evidence / "final.json")
        self.assertTrue(Path(capture["argv"][capture["argv"].index("--output-schema") + 1]).is_file())
        if mode != "changed":
            self.assertEqual(hashlib.sha256(source.read_bytes()).hexdigest(), before)
        if mode in ("timeout", "terminated", "stale", "wrong", "malformed", "final-limit"):
            self.assertTrue((evidence / "final.json").is_file(), "Partial output must remain")

    def test_success(self):
        self.run_case("success", "success")

    def test_negative_cases(self):
        for mode, status in (("auth", "auth-failure"), ("process", "process-failure"),
                ("terminated", "terminated"), ("missing", "missing-json"), ("malformed", "malformed-json"),
                ("stale", "stale-json"), ("schema", "schema-invalid"), ("wrong", "wrong-answer"),
                ("incomplete", "task-incomplete"), ("task-failure", "task-failure"),
                ("events", "malformed-events"), ("unresolved", "unresolved-issues"), ("changed", "inputs-changed")):
            with self.subTest(mode=mode):
                self.run_case(mode, status)

    def test_timeout_and_limits(self):
        self.run_case("timeout", "timeout", timeout=2)
        self.run_case("output", "output-limit", output_limit=1024)
        self.run_case("final-limit", "output-limit", output_limit=1024)


if __name__ == "__main__":
    print("Evidence: " + str(ROOT), flush=True)
    unittest.main()
