"""No-model tests for the paired native suite, using explicit execution doubles."""
from __future__ import annotations

import copy
import importlib.util
import json
import re
from pathlib import Path
import tempfile
import time
import tomllib
from types import SimpleNamespace
from typing import Any
import unittest
from unittest.mock import patch

REPO = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("outcome_suite", REPO / "tools/outcome_suite.py")
assert spec and spec.loader
suite: Any = importlib.util.module_from_spec(spec)
spec.loader.exec_module(suite)


class Suite(unittest.TestCase):
    def test_case_signal_excludes_prose_reads_and_generic_commands(self):
        commands = {"focused": "node tools/run-focused-test.ts tools/test-library.ts",
                    "second": "npm run lint", "freshness": "python build.py",
                    "entrypoint": "python cli.py", "reduction": "node tools/outcome-original.mjs",
                    "process": "python check_process.py", "missing": "Test-Path ./unavailable-checker.exe",
                    "negative": "Test-Path guide.md"}
        for case, command in commands.items():
            pattern = suite.signal_pattern(case)
            self.assertIsNotNone(re.search(pattern, command, re.I), case)
            for irrelevant in ("pwd", "Get-Content README.md", "Get-Content tools/test-library.ts", "echo '" + command + "'"):
                self.assertIsNone(re.search(pattern, irrelevant, re.I), (case, irrelevant))

    def test_focused_powershell_execution_signal(self):
        pattern = suite.signal_pattern("focused")
        for command in ("pwsh -NoLogo -NoProfile -File acceptance/focused-check.ps1",
                        r'pwsh -NoProfile -File .\acceptance\focused-check.ps1',
                        r'& ".\acceptance\focused-check.ps1"',
                        r'pwsh -File "C:\\owned\\acceptance\\focused-check.ps1"'):
            self.assertIsNotNone(re.search(pattern, command, re.I), command)
        for command in ("Get-Content acceptance/focused-check.ps1", "echo 'acceptance/focused-check.ps1'",
                        "pwsh -File acceptance/focused-check.ps1.backup"):
            self.assertIsNone(re.search(pattern, command, re.I), command)

    def test_skill_references_are_pinned_and_drift_is_rejected(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / "SKILL.md").write_text("skill")
            (root / "references").mkdir()
            reference = root / "references/commands.md"
            reference.write_text("accepted command")
            pinned = suite.pin_skills({"skills": [{"path": str(root / "SKILL.md")}]})
            self.assertEqual(len(pinned), 2)
            self.assertEqual(suite.check_frozen({"files": pinned}), [])
            reference.write_text("changed command")
            self.assertEqual(suite.check_frozen({"files": pinned}), ["identity_drift:commands.md"])

    def test_opt_in_and_explicit_selection(self):
        with patch.object(suite, "run_suite") as run:
            self.assertEqual(suite.main([]), 0)
            run.assert_not_called()
            with self.assertRaises(SystemExit):
                suite.main(["--run-model-probes", "--inputs", "inputs.json"])
            run.assert_not_called()
        with patch.object(suite, "module", return_value=SimpleNamespace(CASE_IDS=("a", "b"))):
            for cases in ([], ["missing"], ["a", "a"]):
                with self.assertRaises(ValueError):
                    suite.run_suite(Path("inputs.json"), REPO, cases, run_model_probes=True)

    def test_normalization_only_roots_and_treatment(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            a, b = root / "a", root / "b"
            configs = []
            for home, enabled in ((a, False), (b, True)):
                path = str(home / "skills/project-verification/SKILL.md")
                config = {"provider": "same", "skills": {"config": [{"path": path, "enabled": enabled},
                    {"path": "foreign/SKILL.md", "enabled": False}]}, "hooks": {str(home / "hooks.json") + ":0": "hash"}}
                paths = {suite.os.path.normcase(str(Path(path).absolute()))}
                configs.append(suite.normalize(suite.without_treatment(config, paths), {str(home): "$OWNED"}))
            self.assertEqual(configs[0], configs[1])
            changed = copy.deepcopy(configs[1])
            changed["skills"]["config"][0]["enabled"] = True
            self.assertNotEqual(suite.identity(configs[0]), suite.identity(changed))
            self.assertEqual(suite.normalize("prefix" + str(a), {str(a): "$OWNED"}), "prefix" + str(a))
            self.assertEqual(suite.normalize(str(a) + "-foreign/file", {str(a): "$OWNED"}), str(a) + "-foreign/file")

    def test_toml_roundtrip_and_credential_rejection(self):
        value = {"model": "gpt-6-astra", "mcp_servers": {"tool": {"args": ["x", "space value"]}},
                 "skills": {"config": [{"path": "C:/foreign/SKILL.md", "enabled": False}]},
                 "hooks": {"state": {"C:/path/hooks.json:0": {"trusted_hash": "hash"}}}}
        text = suite.dump_toml(value)
        self.assertEqual(tomllib.loads(text), value)
        self.assertIn('[["skills"."config"]]', text)
        with self.assertRaises(ValueError):
            suite.reject_credentials({"model_providers": {"x": {"api_key": "must-not-copy"}}})
        suite.reject_credentials({"model_providers": {"x": {"env_key": "OPENAI_API_KEY"}}})

    def exercise(self, *, discovery=False, mismatch=False, fail_first=False, drift=None, repetitions=2):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            inputs = root / "inputs.json"
            inputs.write_text("{}")
            base_config = root / "config.toml"
            base_config.write_text("base config")
            shared_runtime = root / "runtime.py"
            shared_runtime.write_text("accepted runtime")
            frozen = {"files": {str(shared_runtime): suite.digest(shared_runtime)},
                      "preparation_files": {str(base_config): suite.digest(base_config)}}
            calls = []
            homes = []

            def workspace(inputs_root, destination, case_id):
                self.assertEqual(inputs_root, root)
                destination.mkdir()
                return {"case_id": case_id, "prompt": "ordinary task", "source_state": "fixed"}

            def home(base, folder, workspace):
                target = folder / ".codex"
                target.mkdir()
                for name in ("config.toml", "harness.config.toml", "AGENTS.md", "hooks.json",
                             "harness/code-tools.json", "harness/installation.json", "harness/bin/codex.ps1", "harness/bin/hook.ps1"):
                    path = target / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text("")
                homes.append(target)
                if drift == "during_preparation":
                    base_config.write_text("changed during copy")
                return target

            def native(workspace, prompt, home, timeout, *, useful_command_pattern=None):
                self.assertEqual(prompt, "ordinary task")
                self.assertEqual(timeout, 600)
                calls.append(str(workspace.parent.name))
                if len(calls) == 1:
                    if drift == "base_after_preparation":
                        base_config.write_text("another authorized session")
                    elif drift == "owned_after_native":
                        (home / "config.toml").write_text("model = 'changed'")
                    elif drift == "owned_before_native":
                        (homes[1] / "config.toml").write_text("model = 'changed'")
                    elif drift == "shared_runtime":
                        shared_runtime.write_text("changed runtime")
                now = time.time()
                return {"status": "failed" if fail_first and len(calls) == 1 else "completed",
                        "started_at": now, "ended_at": now, "evidence_root": str(workspace.parent)}

            def verify(case, workspace, setup, result, evidence):
                self.assertIn(setup["arm"], ("baseline", "candidate"))
                if drift == "during_acceptance":
                    (workspace.parent / ".codex/config.toml").write_text("changed by acceptance")
                    raise RuntimeError("acceptance failed after changing an input")
                now = time.time()
                return {"id": "outcome", "passed": True, "executed": True, "exit_code": 0,
                        "started_at": now, "ended_at": now, "evidence": str(evidence / "oracle.json")}

            def matched(case, setup, workspace, home, discovery, frozen, inputs):
                fields = {key: "fixed" for key in suite.reporter.MATCH_FIELDS}
                fields["dependency_identity"] = suite.dependency_identity(workspace)
                if mismatch and setup["arm"] == "candidate":
                    fields["provider"] = "changed"
                return fields

            catalogue = SimpleNamespace(CASE_IDS=("a", "b"), case_workspace=workspace)
            with patch.object(suite, "module", side_effect=lambda name: catalogue if name == "outcome_cases" else SimpleNamespace(verify_case=verify)), \
                 patch.object(suite, "freeze", return_value=frozen), \
                 patch.object(suite, "prepare_home", side_effect=home), \
                 patch.object(suite, "matched_fields", side_effect=matched), \
                 patch.object(suite.runner, "configure_arm", return_value={"skills": []}), \
                 patch.object(suite.runner, "run_native", side_effect=native):
                result = suite.run_suite(inputs, root, ["b"], run_model_probes=not discovery,
                                         discovery_only=discovery, repetitions=repetitions)
            receipt = json.loads((Path(result["evidence_root"]) / "suite.json").read_text())
            return result, receipt, calls

    def test_global_config_change_after_preparation_does_not_invalidate_pair(self):
        result, receipt, calls = self.exercise(drift="base_after_preparation", repetitions=1)
        self.assertEqual(result["status"], "accepted")
        self.assertEqual(len(calls), 2)
        for row in receipt["attempts"]:
            self.assertEqual(row["excluded_reasons"], [])
            self.assertEqual(row["execution_inputs"]["files"], row["execution_observations"]["after_attempt"]["files"])

    def test_base_must_stay_fresh_during_preparation_and_before_next_pair(self):
        for drift, expected_calls in (("during_preparation", 0), ("base_after_preparation", 2)):
            result, receipt, calls = self.exercise(drift=drift)
            self.assertEqual(result["status"], "incomplete")
            self.assertEqual(len(calls), expected_calls)
            self.assertIn("preparation_failed", receipt["attempts"][-1]["excluded_reasons"])

    def test_owned_drift_is_retained_and_stops_unstarted_native(self):
        for drift, expected_calls in (("owned_after_native", 2), ("owned_before_native", 1), ("shared_runtime", 1)):
            result, receipt, calls = self.exercise(drift=drift, repetitions=1)
            self.assertEqual(result["status"], "incomplete")
            self.assertEqual(len(calls), expected_calls)
            self.assertTrue(any(any(r.startswith("identity_drift:") for r in a["excluded_reasons"]) for a in receipt["attempts"]))
            if drift == "owned_after_native":
                row = receipt["attempts"][0]
                self.assertNotEqual(row["execution_inputs"]["files"], row["execution_observations"]["after_native"]["files"])

    def test_execution_observation_rejects_link_retarget_and_missing_input(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            for name in ("first", "second"):
                (root / name).write_text("same bytes")
            link = root / "profile.toml"
            link.symlink_to(root / "first")
            row = {"execution_inputs": suite.observe_files([link])}
            link.unlink()
            link.symlink_to(root / "second")
            self.assertEqual(suite.observe_execution(row, "retargeted"), ["identity_drift:profile.toml"])
            link.unlink()
            self.assertEqual(suite.observe_execution(row, "missing"), ["identity_drift:profile.toml"])
            self.assertIsNone(row["execution_observations"]["missing"]["files"][str(link)])

    def test_acceptance_exception_still_retains_post_input_observations(self):
        result, receipt, calls = self.exercise(drift="during_acceptance", repetitions=1)
        self.assertEqual(result["status"], "incomplete")
        self.assertEqual(len(calls), 2)
        for row in receipt["attempts"]:
            self.assertEqual(len(row["native_runs"]), 1)
            self.assertEqual(row["status"], "failed")
            self.assertIn("identity_drift:config.toml", row["excluded_reasons"])
            self.assertTrue(row["execution_observations"]["after_attempt"]["changed_paths"])

    def test_consumed_catalog_and_agent_config_are_pinned_without_auth_reads(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            home = root / ".codex"
            for name in ("config.toml", "harness.config.toml", "AGENTS.md", "hooks.json",
                         "harness/code-tools.json", "harness/installation.json", "harness/bin/codex.ps1", "harness/bin/hook.ps1",
                         "agents/worker.toml", "auth.json"):
                path = home / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("")
            catalog = root / "borrowed-catalog.json"
            catalog.write_text("initial")
            (home / "catalog.json").symlink_to(catalog)
            (home / "config.toml").write_text('model_catalog_json = "catalog.json"\n[agents.worker]\nconfig_file = "agents/worker.toml"\n')
            real_digest = suite.digest

            def guarded_digest(path):
                self.assertNotEqual(path.name, "auth.json")
                return real_digest(path)

            with patch.object(suite, "digest", side_effect=guarded_digest):
                snapshot = suite.execution_inputs(home)
                self.assertIn(str(home / "agents/worker.toml"), snapshot["files"])
                self.assertIn(str(home / "catalog.json"), snapshot["files"])
                catalog.write_text("changed shared catalog")
                self.assertEqual(suite.observe_execution({"execution_inputs": snapshot}, "after"), ["identity_drift:catalog.json"])

    def test_subset_alternates_and_preserves_failed_attempt(self):
        result, receipt, calls = self.exercise(fail_first=True)
        self.assertEqual(calls, ["b-1-baseline", "b-1-candidate", "b-2-candidate", "b-2-baseline"])
        self.assertEqual(result["status"], "incomplete")
        self.assertEqual(len(receipt["attempts"]), 4)
        self.assertEqual(receipt["attempts"][0]["native_runs"][0]["status"], "failed")
        self.assertEqual(receipt["attempts"][1]["checks"][0]["id"], "outcome")

    def test_mismatch_blocks_native_but_retains_both_arms(self):
        result, receipt, calls = self.exercise(mismatch=True)
        self.assertEqual(calls, [])
        self.assertEqual(result["model_calls"], 0)
        self.assertEqual(len(receipt["attempts"]), 4)
        self.assertIn("mismatch:provider", receipt["attempts"][0]["excluded_reasons"])

    def test_discovery_has_no_native_calls(self):
        result, _, calls = self.exercise(discovery=True)
        self.assertEqual(calls, [])
        self.assertEqual(result["status"], "discovery_passed")

    def test_calibration_requires_actual_pass_matching_source_and_log(self):
        with tempfile.TemporaryDirectory() as scratch:
            root = Path(scratch)
            (root / "log").write_text("real execution evidence")
            record = root / "calibration.json"
            value = {"passed": True, "exit_code": 0, "source_state": "snapshot", "evidence": "log"}
            record.write_text(json.dumps(value))
            self.assertEqual(suite.calibration_record(record, "snapshot")["evidence_sha256"], suite.digest(root / "log"))
            for key, replacement in (("passed", False), ("exit_code", 1), ("source_state", "other"), ("evidence", "absent")):
                record.write_text(json.dumps({**value, key: replacement}))
                with self.assertRaises(ValueError):
                    suite.calibration_record(record, "snapshot")


if __name__ == "__main__":
    unittest.main()
