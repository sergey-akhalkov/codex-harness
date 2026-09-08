"""Qualify installed Serena 1.7.0 diagnostics and edits through the shared broker.

Owned fixtures live outside this repository. The adopted dependency registry is
required. This check does not enable inline diagnostics, patch Serena, restore
hooks, or treat an empty diagnostic object as a clean result.
"""
from __future__ import annotations

import ast
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

import psutil

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools/code-tools"))
import serena_broker as broker

HOST_CODEX = Path(os.environ.get("CODEX_HOME") or (Path.home() / ".codex")).resolve()
REGISTRY = Path(os.environ.get("HARNESS_CODE_TOOLS_REGISTRY") or (HOST_CODEX / "harness/code-tools.json")).resolve()
HOST_SERENA = Path(os.environ.get("SERENA_HOME") or (Path.home() / ".serena")).resolve()


def load_shared():
    spec = importlib.util.spec_from_file_location("serena_shared", ROOT / "tests/serena-shared.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SHARED = load_shared()
ARGS = list(SHARED.ARGS)
INITIALIZE = {**SHARED.INITIALIZE, "clientInfo": {"name": "serena-efficiency", "version": "1"}}
SAMPLE = """def oracle_value() -> int:
    return 1

def helper() -> str:
    return "keep"
"""
BAD_BODY = """def oracle_value() -> int:
    return "wrong"
"""
GOOD_BODY = """def oracle_value() -> int:
    return 42
"""
EXCLUDED = {"create_text_file", "read_file", "execute_shell_command", "replace_content", "find_file", "list_dir"}
EXPOSED = {"get_diagnostics_for_file", "replace_symbol_body", "replace_in_files",
           "insert_before_symbol", "insert_after_symbol", "get_symbols_overview", "find_symbol"}


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tool_text(result) -> str:
    return "\n".join(item.get("text", "") for item in result.get("content") or [] if item.get("type") == "text")


def parse_json(text: str):
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return None


def python_language_record(inventory: dict) -> dict:
    """Reuse the still-installed basedpyright cache. Live code-tools.json currently has languages=[]."""
    node = next((Path(item.get("paths", {}).get("node", "")) for item in inventory.get("mcp", [])
                 if Path(item.get("paths", {}).get("node", "")).is_file()), None)
    entry = HOST_SERENA / "language_servers/static/BasedPyrightLanguageServer/shared/node_modules/basedpyright/langserver.index.js"
    package = entry.parent / "package.json"
    if node is None or not entry.is_file() or not package.is_file():
        raise FileNotFoundError("adopted basedpyright cache or Node runtime is missing")
    version = json.loads(package.read_text(encoding="utf-8")).get("version")
    return {
        "id": "python",
        "identity": "basedpyright",
        "status": "adopted",
        "version": version,
        "command": [str(node), str(entry)],
        "paths": {"node": str(node), "entrypoint": str(entry)},
    }


def replace_oracle_body(source: str, body: str) -> str:
    tree = ast.parse(source)
    target = next(node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name == "oracle_value")
    lines = source.splitlines(keepends=True)
    start = target.lineno - 1
    end = target.end_lineno
    prefix = "".join(lines[:start])
    suffix = "".join(lines[end:])
    text = body if body.endswith("\n") else body + "\n"
    return prefix + text + suffix


class QualClient:
    """stdio proxy client matching tests/serena-shared.ProxyClient with a longer first-use timeout."""

    def __init__(self, cwd, python):
        self.process = subprocess.Popen(
            [python, "-B", str(ROOT / "tools/code-tools/serena_proxy.py"), *ARGS],
            cwd=cwd, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        self.serial = 0
        self.output = queue.Queue()
        self.errors = []
        threading.Thread(target=self._stdout, daemon=True).start()
        threading.Thread(target=lambda: self.errors.append(self.process.stderr.read()), daemon=True).start()

    def _stdout(self):
        try:
            for raw in self.process.stdout:
                self.output.put(json.loads(raw))
        except Exception as error:
            self.output.put(error)

    def rpc(self, method, params=None, timeout=180):
        self.serial += 1
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.serial, "method": method,
                                           "params": params or {}}).encode() + b"\n")
        self.process.stdin.flush()
        deadline = time.monotonic() + timeout
        while True:
            message = self.output.get(timeout=max(0.01, deadline - time.monotonic()))
            if isinstance(message, Exception):
                raise message
            if message.get("id") == self.serial:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message["result"]

    def tool(self, name, arguments=None, timeout=180):
        result = self.rpc("tools/call", {"name": name, "arguments": arguments or {}}, timeout=timeout)
        return result, tool_text(result)

    def close(self):
        try:
            self.process.stdin.close()
        except OSError:
            pass
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        for stream in (self.process.stdout, self.process.stderr):
            stream.close()


class InstalledSerenaQualification(unittest.TestCase):
    def test_diagnostics_and_edit_surface(self):
        self.assertTrue(REGISTRY.is_file(), f"adopted registry missing: {REGISTRY}")
        inventory = json.loads(REGISTRY.read_text(encoding="utf-8-sig"))
        serena = next(item for item in inventory["mcp"] if item["id"] == "serena")
        python = serena["paths"]["python"]
        language = next((item for item in inventory.get("languages", []) if item.get("id") == "python"), None)
        self.assertIsNotNone(language, "Retained explicit Serena Python dependency is absent from the live registry")
        self.assertEqual(serena.get("version"), "1.7.0")
        module_root = Path(serena["paths"]["module_root"])
        tools_base = module_root / "serena/tools/tools_base.py"
        context = module_root / "serena/resources/config/contexts/codex.yml"
        ls_impl = module_root / "solidlsp/ls.py"
        self.assertIn("ENABLE_DIAGNOSTICS: bool = False", tools_base.read_text(encoding="utf-8"))
        self.assertIn("- create_text_file", context.read_text(encoding="utf-8"))
        self.assertIn("return bool(diagnostics)", ls_impl.read_text(encoding="utf-8"))

        preserved = {str(path): sha256(path) if path.is_file() else None for path in (
            HOST_SERENA / "serena_config.yml", HOST_CODEX / "harness/code-tools.json",
            HOST_CODEX / "harness/code-tools-registration.json", REGISTRY,
            ROOT / "tools/code-tools/serena_broker.py", ROOT / "tools/code-tools/serena_proxy.py")}

        scratch = Path(tempfile.mkdtemp(prefix="serena-efficiency-"))
        self.assertFalse(str(scratch).lower().startswith(str(ROOT).lower() + os.sep))
        report = {
            "root": str(scratch), "python": python, "serena": serena["version"],
            "declared_project_language_servers": ["python"],
            "discovery_python": None,
            "source": {str(path): sha256(path) for path in (
                ROOT / "tools/code-tools/serena_broker.py", ROOT / "tools/code-tools/serena_proxy.py",
                ROOT / "tools/code-tools/serena_entry.py", Path(__file__))},
            "installed": {"tools_base": sha256(tools_base), "codex_context": sha256(context),
                          "solidlsp_ls": sha256(ls_impl)},
            "enable_diagnostics": False, "findings": {}, "passed": False,
        }
        clients = []
        owner = None
        environment = {
            "CODEX_HOME": str(scratch / "codex"), "SERENA_HOME": str(scratch / "home"),
            "HARNESS_SERENA_BROKER_DIR": str(scratch / "broker"),
            "HARNESS_CODE_TOOLS_REGISTRY": str(REGISTRY), "PYTHONUTF8": "1",
        }
        try:
            with patch.dict(os.environ, environment):
                report["registry_languages_empty"] = False
                report["registry"] = str(REGISTRY)
                report["discovery_python"] = {"id": language.get("id"), "identity": language.get("identity"),
                                              "version": language.get("version"),
                                              "entrypoint": (language.get("paths") or {}).get("entrypoint")}
                projects = [scratch / "alpha", scratch / "beta"]
                for project, marker in zip(projects, (1, 2)):
                    (project / ".serena").mkdir(parents=True)
                    (project / ".serena/project.yml").write_text(
                        f"project_name: {project.name}\nlanguage_servers: [python]\nencoding: utf-8\n",
                        encoding="utf-8")
                    (project / "pyrightconfig.json").write_text(
                        '{"typeCheckingMode":"basic","reportReturnType":"error"}\n', encoding="utf-8")
                    (project / "sample.py").write_text(SAMPLE.replace("return 1", f"return {marker}"), encoding="utf-8")
                    (project / "caller.py").write_text('from sample import oracle_value\nanswer = oracle_value()\n', encoding="utf-8")
                cold_started = time.perf_counter()
                first = QualClient(projects[0], python)
                clients.append(first)
                first.rpc("initialize", INITIALIZE)
                listed = first.rpc("tools/list", {})
                names = {item["name"] for item in listed.get("tools", [])}
                report["tools"] = sorted(names)
                report["findings"]["exposed"] = sorted(EXPOSED & names)
                report["findings"]["excluded_absent"] = sorted(EXCLUDED - names)
                report["findings"]["excluded_present"] = sorted(EXCLUDED & names)
                self.assertTrue(EXPOSED <= names, sorted(names))
                self.assertFalse(EXCLUDED & names, sorted(names))
                created, created_text = first.tool("create_text_file", {"relative_path": "new.py", "content": "x = 1\n"})
                report["findings"]["creation_call"] = {"isError": bool(created.get("isError")), "text": created_text[:500]}
                self.assertTrue(created.get("isError") or created_text.startswith("Error:") or
                                "Unknown tool" in created_text or "not found" in created_text.lower(), created_text)
                self.assertFalse((projects[0] / "new.py").exists())
                overview, overview_text = first.tool("get_symbols_overview", {"relative_path": "sample.py"})
                self.assertFalse(overview.get("isError"), overview_text)
                self.assertIn("oracle_value", overview_text)
                report["timing"] = {"cold_connect_and_overview_s": round(time.perf_counter() - cold_started, 3)}

                endpoint = broker.ensure_endpoint(broker.source_identity())
                owner = psutil.Process(endpoint["pid"])
                status = broker.exchange(endpoint, "status", {}, 5)
                report["initial_workers"] = status.get("workers")
                self.assertEqual(len(status.get("workers") or []), 1)
                first_pid = status["workers"][0]["pid"]

                warm_started = time.perf_counter()
                second = QualClient(projects[0], python)
                clients.append(second)
                second.rpc("initialize", INITIALIZE)
                _, warm_text = second.tool("get_symbols_overview", {"relative_path": "sample.py"})
                self.assertIn("oracle_value", warm_text)
                report["timing"]["warm_second_client_overview_s"] = round(time.perf_counter() - warm_started, 3)
                status = broker.exchange(endpoint, "status", {}, 5)
                report["warm_workers"] = status.get("workers")
                self.assertEqual(len(status.get("workers") or []), 1)
                self.assertEqual(status["workers"][0]["pid"], first_pid)

                second.tool("activate_project", {"project": str(projects[1])})
                for client, present, absent in zip(clients, ("return 1", "return 2"), ("return 2", "return 1")):
                    result, text = client.tool("find_symbol", {
                        "relative_path": "sample.py", "name_path_pattern": "oracle_value", "include_body": True})
                    self.assertFalse(result.get("isError"), text)
                    self.assertIn(present, text)
                    self.assertNotIn(absent, text)
                status = broker.exchange(endpoint, "status", {}, 5)
                report["activated_workers"] = status.get("workers")
                self.assertEqual(len(status.get("workers") or []), 2)
                self.assertIn(first_pid, [item["pid"] for item in status["workers"]])

                _, clean_text = first.tool("get_diagnostics_for_file", {"relative_path": "sample.py"})
                report["findings"]["initial_diagnostics"] = clean_text[:1000]
                references, references_text = first.tool("find_referencing_symbols", {
                    "name_path": "oracle_value", "relative_path": "sample.py"})
                self.assertFalse(references.get("isError"), references_text)
                self.assertIn("caller.py", references_text)
                report["findings"]["cross_file_references"] = references_text[:1500]
                edit, edit_text = first.tool("replace_symbol_body", {
                    "name_path": "oracle_value", "relative_path": "sample.py", "body": BAD_BODY})
                self.assertFalse(edit.get("isError"), edit_text)
                self.assertIn("OK", edit_text)
                self.assertNotIn("diagnostics[", edit_text)
                report["findings"]["inline_edit_result"] = edit_text[:300]
                self.assertIn('return "wrong"', (projects[0] / "sample.py").read_text(encoding="utf-8"))
                self.assertIn("return 2", (projects[1] / "sample.py").read_text(encoding="utf-8"))

                error_diag = self._await_diagnostics(first, want_error=True)
                report["findings"]["error_diagnostics"] = error_diag
                _, limited_text = first.tool("get_diagnostics_for_file", {
                    "relative_path": "sample.py", "max_answer_chars": 40})
                _, full_text = first.tool("get_diagnostics_for_file", {"relative_path": "sample.py"})
                report["findings"]["limited_diagnostics"] = limited_text[:500]
                report["findings"]["full_diagnostics"] = full_text[:1500]
                report["findings"]["output_limit_replaces_details"] = (
                    "too long" in limited_text.lower() and "oracle_value" not in limited_text)
                report["findings"]["full_details_available"] = self._looks_like_error(full_text)

                first.tool("replace_symbol_body", {
                    "name_path": "oracle_value", "relative_path": "sample.py", "body": GOOD_BODY.replace("42", "1")})
                clearance = self._await_diagnostics(first, want_error=False)
                report["findings"]["clearance_diagnostics"] = clearance
                report["findings"]["clearance_kind"] = self._classify_clearance(clearance)

                (projects[0] / "sample.py").write_text(SAMPLE, encoding="utf-8")
                noop, noop_text = first.tool("replace_in_files", {
                    "needle": "does-not-occur-in-fixture", "repl": "x", "mode": "literal",
                    "relative_path": "sample.py", "dry_run": False})
                report["findings"]["noop_replace"] = {"isError": bool(noop.get("isError")), "text": noop_text[:400]}
                self.assertTrue(noop.get("isError") or "NO changes" in noop_text or "No occurrences" in noop_text, noop_text)
                self.assertEqual((projects[0] / "sample.py").read_text(encoding="utf-8"), SAMPLE)
                dry, dry_text = first.tool("replace_in_files", {
                    "needle": "return 1", "repl": "return 7", "mode": "literal",
                    "relative_path": "sample.py", "dry_run": True})
                self.assertFalse(dry.get("isError"), dry_text)
                self.assertEqual((projects[0] / "sample.py").read_text(encoding="utf-8"), SAMPLE)
                failed, failed_text = first.tool("replace_in_files", {
                    "needle": "return 1", "repl": "return 7", "mode": "literal",
                    "relative_path": "sample.py", "dry_run": False, "expected_count": 2})
                report["findings"]["partial_failure"] = {"isError": bool(failed.get("isError")), "text": failed_text[:500]}
                self.assertTrue(failed.get("isError") or "NO changes" in failed_text, failed_text)
                self.assertEqual((projects[0] / "sample.py").read_text(encoding="utf-8"), SAMPLE)
                applied, applied_text = first.tool("replace_in_files", {
                    "needle": 'return "keep"', "repl": 'return "kept"', "mode": "literal",
                    "relative_path": "sample.py", "dry_run": False, "expected_count": 1})
                self.assertFalse(applied.get("isError"), applied_text)
                self.assertIn('return "kept"', (projects[0] / "sample.py").read_text(encoding="utf-8"))

                inserted, inserted_text = first.tool("insert_after_symbol", {
                    "name_path": "helper", "relative_path": "sample.py",
                    "body": "def added() -> int:\n    return 0\n"})
                report["findings"]["insert_after"] = {"isError": bool(inserted.get("isError")), "text": inserted_text[:300]}
                self.assertFalse(inserted.get("isError"), inserted_text)
                self.assertIn("def added", (projects[0] / "sample.py").read_text(encoding="utf-8"))
                deleted, deleted_text = first.tool("safe_delete_symbol", {
                    "name_path_pattern": "added", "relative_path": "sample.py"})
                report["findings"]["safe_delete"] = {"isError": bool(deleted.get("isError")), "text": deleted_text[:300]}
                self.assertFalse(deleted.get("isError"), deleted_text)
                (projects[0] / "sample.py").write_text(SAMPLE, encoding="utf-8")

                (projects[0] / "external.py").write_text(
                    'def leaked() -> int:\n    return "external"\n', encoding="utf-8")
                external = self._await_diagnostics(first, relative="external.py", want_error=True)
                sample_after_external = self._await_diagnostics(first, want_error=False)
                report["findings"]["filesystem_sync_new_file"] = external
                report["findings"]["filesystem_sync_other_file_result"] = sample_after_external
                report["findings"]["filesystem_sync_scope"] = (
                    "get_diagnostics_for_file polls every tracked source file and notifies every "
                    "project language server, then returns only the requested file")

                pairs = []
                for index in range(3):
                    (projects[0] / "sample.py").write_text(SAMPLE, encoding="utf-8")
                    serena_started = time.perf_counter()
                    nav, nav_text = first.tool("find_symbol", {
                        "relative_path": "sample.py", "name_path_pattern": "oracle_value", "include_body": True})
                    self.assertFalse(nav.get("isError"), nav_text)
                    self.assertIn("oracle_value", nav_text)
                    mutated, mutated_text = first.tool("replace_symbol_body", {
                        "name_path": "oracle_value", "relative_path": "sample.py", "body": GOOD_BODY})
                    self.assertFalse(mutated.get("isError"), mutated_text)
                    serena_oracle = self._native_oracle(python, projects[0])
                    serena_s = time.perf_counter() - serena_started
                    self.assertEqual(serena_oracle["exit"], 0, serena_oracle)
                    (projects[0] / "sample.py").write_text(SAMPLE, encoding="utf-8")
                    native_started = time.perf_counter()
                    current = (projects[0] / "sample.py").read_text(encoding="utf-8")
                    tree = ast.parse(current)
                    self.assertTrue(any(isinstance(node, ast.FunctionDef) and node.name == "oracle_value" for node in tree.body))
                    (projects[0] / "sample.py").write_text(replace_oracle_body(current, GOOD_BODY), encoding="utf-8")
                    native_oracle = self._native_oracle(python, projects[0])
                    native_s = time.perf_counter() - native_started
                    self.assertEqual(native_oracle["exit"], 0, native_oracle)
                    delta = abs(serena_s - native_s)
                    baseline = min(serena_s, native_s)
                    material = delta > 0.2 and (baseline == 0 or delta > 0.10 * baseline)
                    winner = "serena" if serena_s < native_s else "native" if native_s < serena_s else "tie"
                    pairs.append({"pair": index + 1, "phase": "warm", "serena_s": round(serena_s, 3),
                                  "native_s": round(native_s, 3), "winner": winner, "material": material,
                                  "serena_oracle": serena_oracle, "native_oracle": native_oracle})
                    if len(pairs) >= 2 and pairs[-1]["winner"] == pairs[-2]["winner"] and pairs[-1]["material"] == pairs[-2]["material"]:
                        break
                spread = 0.0
                if len(pairs) >= 2:
                    serena_spread = max(item["serena_s"] for item in pairs) - min(item["serena_s"] for item in pairs)
                    native_spread = max(item["native_s"] for item in pairs) - min(item["native_s"] for item in pairs)
                    spread = max(serena_spread, native_spread)
                mean_serena = sum(item["serena_s"] for item in pairs) / len(pairs)
                mean_native = sum(item["native_s"] for item in pairs) / len(pairs)
                mean_delta = abs(mean_serena - mean_native)
                overall_material = mean_delta > (spread + 0.2) and mean_delta > 0.10 * min(mean_serena, mean_native)
                consistent = len(pairs) >= 2 and pairs[0]["winner"] == pairs[1]["winner"] and pairs[0]["material"] == pairs[1]["material"]
                report["timing"]["oracle_pairs"] = pairs
                report["timing"]["repeat_spread_s"] = round(spread, 3)
                report["timing"]["mean_serena_s"] = round(mean_serena, 3)
                report["timing"]["mean_native_s"] = round(mean_native, 3)
                report["timing"]["material_after_spread"] = overall_material
                report["timing"]["two_consistent"] = consistent
                report["timing"]["comparison"] = (
                    "inconclusive" if not consistent or not overall_material else
                    ("serena-faster" if mean_serena < mean_native else "native-faster"))

                reasons = [
                    "ENABLE_DIAGNOSTICS is False on installed 1.7.0; this check did not flip it",
                    "Codex context excludes create_text_file, replace_content, read_file and shell",
                    "get_diagnostics_for_file polls every tracked source file and notifies every project language server",
                    "solidlsp accepts nonempty published diagnostics and can return [] when no accepted result is obtained",
                ]
                if report["findings"]["clearance_kind"] != "explicit-empty-unauthoritative":
                    reasons.append("clearance classification: " + report["findings"]["clearance_kind"])
                else:
                    reasons.append("empty {} after correction is not treated as an authoritative clean result")
                if report["findings"].get("output_limit_replaces_details"):
                    reasons.append("max_answer_chars replaces an oversized result with a too-long notice")
                if not report["findings"].get("full_details_available"):
                    reasons.append("current error findings were not observed on the requested file")
                report["automatic_diagnostics"] = "rejected"
                report["automatic_rejection_reasons"] = reasons
                report["explicit_limits"] = {
                    "languages_qualified": ["python"],
                    "inline_diagnostics": False,
                    "creation": "excluded",
                    "provider": "Serena 1.7.0 / language_backend LSP / project language_servers=[python]",
                    "harness_lsp": "retired; not used",
                    "timing_does_not_promote_automatic": True,
                }
                report["passed"] = True
                if owner is not None and owner.is_running():
                    broker.retire()
                    owner.wait(timeout=15)
                    owner = None
        finally:
            for client in clients:
                try:
                    client.close()
                except Exception:
                    pass
            if owner is not None:
                report["retire_skipped_outside_owned_environment"] = True
            report["proxy_stderr"] = [b"".join(client.errors).decode("utf-8", errors="replace")[-4000:] for client in clients]
            broken = {}
            for path, expected in preserved.items():
                current = sha256(Path(path)) if Path(path).is_file() else None
                if current != expected:
                    broken[path] = {"expected": expected, "current": current}
                    report["passed"] = False
            report["preserved"] = {"unchanged": not broken, "changed": broken}
            (scratch / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
            print("Serena efficiency evidence: " + str(scratch / "report.json"), file=sys.stderr)
            print(json.dumps({"passed": report.get("passed"),
                "automatic_diagnostics": report.get("automatic_diagnostics"),
                "details": str(scratch / "report.json")}))
            self.assertTrue(report.get("passed"), json.dumps({k: report.get(k) for k in ("findings", "preserved", "retire_error")}, default=str)[:4000])

    def _looks_like_error(self, text: str) -> bool:
        lowered = text.lower()
        return bool(text.strip() not in ("{}", "[]", "") and (
            "reportreturntype" in lowered or "return type" in lowered or "type" in lowered and "str" in lowered));

    def _classify_clearance(self, result: dict) -> str:
        text = result.get("text") or ""
        if result.get("isError"):
            return "tool-error"
        if self._looks_like_error(text):
            return "stale-or-cached-error"
        if text.strip() == "{}":
            return "explicit-empty-unauthoritative"
        if text.strip() in ("", "[]"):
            return "missing-or-unauthoritative"
        return "nonempty-without-known-error"

    def _await_diagnostics(self, client, relative="sample.py", want_error=False, attempts=8, delay=1.5):
        last = {"isError": True, "text": ""}
        for _ in range(attempts):
            result, text = client.tool("get_diagnostics_for_file", {"relative_path": relative})
            last = {"isError": bool(result.get("isError")), "text": text, "parsed": parse_json(text)}
            if result.get("isError"):
                time.sleep(delay)
                continue
            has_error = self._looks_like_error(text)
            if want_error and has_error:
                return last
            if not want_error:
                return last
            time.sleep(delay)
        return last

    def _native_oracle(self, python, project):
        started = time.perf_counter()
        completed = subprocess.run(
            [python, "-c", "from sample import oracle_value; raise SystemExit(0 if oracle_value() == 42 else 1)"],
            cwd=project, capture_output=True, text=True, timeout=30)
        return {"exit": completed.returncode, "stdout": completed.stdout[-200:],
                "stderr": completed.stderr[-200:], "seconds": round(time.perf_counter() - started, 3)}


if __name__ == "__main__":
    unittest.main(verbosity=2)
