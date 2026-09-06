"""Deterministic discovery failures in disposable installations; never launches a backend."""
import base64
import csv
import hashlib
import importlib.util
import json
import os
import platform
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

MODULE_PATH = Path(__file__).resolve().parents[1] / "tools/code-tools/discovery.py"
SPEC = importlib.util.spec_from_file_location("code_tools_discovery", MODULE_PATH)
discovery = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(discovery)


def write(path, text="fixture"):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def npm_fixture(node_modules, name, version="1.0.0", command="fixture"):
    root = node_modules / name
    write(root / "package.json", json.dumps({"name": name, "version": version, "bin": {command: "bin/cli.js"}}))
    write(root / "bin/cli.js", "// fixture, must never execute\n")
    return root


class DiscoveryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="harness-discovery-")
        self.root = Path(self.temp.name)
        self.node = write(self.root / "runtime/node.exe")

    def tearDown(self):
        self.temp.cleanup()

    def test_catalogue_matches_confirmed_scope(self):
        catalogue = discovery.read_json(discovery.CATALOGUE)
        required = {x["id"] for x in catalogue["languages"] if x["required"]}
        optional = {x["id"] for x in catalogue["languages"] if not x["required"]}
        self.assertEqual(required, {"rust", "typescript", "javascript", "powershell", "python", "delphi", "cpp", "csharp", "json", "markdown", "toml", "xml", "cmake", "bash"})
        self.assertEqual(optional, {"yaml", "qml", "html", "css"})
        self.assertEqual({x["package"] for x in catalogue["mcp"]}, {"serena-agent", "graphifyy", "codebase-memory-mcp", "@nuphus/nuphus-mcp"})
        self.assertFalse(catalogue["runtime_downloads"])
        self.assertEqual(catalogue["operation_policy"]["exposed_operations"], [])

    def test_empty_foreign_home_does_not_adopt_current_process_path(self):
        report = discovery.Discovery(self.root, include_process_environment=False).run()
        self.assertTrue(all(x["status"] == "missing" for x in report["mcp"] + report["languages"]))
        self.assertTrue(all(x["health"]["callable"] is None for x in report["mcp"]))

    @unittest.skipUnless(os.name == "nt", "Host CIM inspection is Windows-specific")
    def test_alternate_home_observes_real_host_consumer_without_adopting_its_runtime(self):
        installation = self.root / "alternate-installation"
        installation.mkdir()
        owned = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)", str(installation), "fixture-secret-argument"],
                                 creationflags=0x08000000)
        try:
            reader = discovery.Discovery(self.root, include_process_environment=False)
            self.assertIsNone(reader.pwsh)
            records = [{"installation_root": str(installation)}]
            reader.inspect_consumers(records)
            self.assertEqual(records[0]["active_consumers"]["state"], "observed")
            self.assertIn(owned.pid, [p["pid"] for p in records[0]["active_consumers"]["processes"]])
            self.assertNotIn("fixture-secret-argument", json.dumps(records))
        finally:
            owned.terminate()
            owned.wait(timeout=10)

    def test_discovery_does_not_write_files(self):
        def snapshot():
            return {str(p.relative_to(self.root)): p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        before = snapshot()
        discovery.Discovery(self.root, include_process_environment=False).run()
        self.assertEqual(before, snapshot())

    def test_delphi_uses_actual_fpc_target_instead_of_host_architecture(self):
        cache = self.root / "cache"
        compiler = write(cache / "PascalLanguageServer/prerequisites/fpc-fixture/bin/unlabelled/fpc.exe")
        (compiler.parents[2] / "source/rtl").mkdir(parents=True)
        reader = discovery.Discovery(self.root, serena_cache=cache, include_process_environment=False)
        spec = next(value for value in reader.catalogue["languages"] if value["id"] == "delphi")
        commands = []
        def query(command, **_arguments):
            commands.append(command)
            return subprocess.CompletedProcess(command, 0, "win32\n" if command[-1] == "-iTO" else "i386\n", "")
        with mock.patch.object(discovery.subprocess, "run", side_effect=query):
            report = reader.language(spec)
        self.assertEqual(report["lsp"]["env"]["FPCTARGET"], "win32")
        self.assertEqual(report["lsp"]["env"]["FPCTARGETCPU"], "i386")
        self.assertEqual(commands, [[str(compiler), "-n", "-iTO"], [str(compiler), "-n", "-iTP"]])

    def test_npm_identity_rejects_same_command_different_distribution(self):
        modules = self.root / "node_modules"
        npm_fixture(modules, "@dreamtree-org/graphify", command="graphify")
        self.assertIsNone(discovery.npm_candidate("graphifyy", modules, self.node, "graphify"))

    def test_missing_native_codebase_binary_is_broken_without_running_downloader(self):
        modules = self.root / "node_modules"
        npm_fixture(modules, "codebase-memory-mcp", command="codebase-memory-mcp")
        report = discovery.npm_candidate("codebase-memory-mcp", modules, self.node, "codebase-memory-mcp")
        self.assertEqual(report["status"], "broken")
        self.assertEqual(report["command"], [])

    def test_codebase_launch_resolves_direct_payload(self):
        modules = self.root / "node_modules"
        root = npm_fixture(modules, "codebase-memory-mcp", command="codebase-memory-mcp")
        native = write(root / "bin" / ("codebase-memory-mcp.exe" if os.name == "nt" else "codebase-memory-mcp"))
        report = discovery.npm_candidate("codebase-memory-mcp", modules, self.node, "codebase-memory-mcp")
        self.assertEqual(report["command"], [str(native)])
        self.assertEqual(report["paths"]["native_executable"], str(native))
        self.assertFalse(report["update_safe"])

    def test_npm_bin_path_cannot_escape_package(self):
        modules = self.root / "node_modules"
        root = npm_fixture(modules, "fixture")
        write(root / "package.json", json.dumps({"name": "fixture", "version": "1", "bin": "../../runtime/node.exe"}))
        report = discovery.npm_candidate("fixture", modules, self.node)
        self.assertEqual(report["status"], "broken")
        self.assertEqual(report["command"], [])

    def test_nuphus_requires_matching_platform_package(self):
        modules = self.root / "node_modules"
        npm_fixture(modules, "@nuphus/nuphus-mcp", command="nuphus-mcp")
        report = discovery.npm_candidate("@nuphus/nuphus-mcp", modules, self.node, "nuphus-mcp")
        self.assertEqual(report["status"], "broken")

    @unittest.skipUnless(os.name == "nt", "Windows local schema-fixed variant")
    def test_nuphus_local_variant_is_preserved_and_blocks_update(self):
        modules = self.root / "node_modules"
        npm_fixture(modules, "@nuphus/nuphus-mcp", command="nuphus-mcp")
        arch = "arm64" if platform.machine().lower() in ("arm64", "aarch64") else "x64"
        companion = npm_fixture(modules, "@nuphus/nuphus-mcp-win32-" + arch)
        original = write(companion / "bin/nuphus-mcp.exe")
        patched = write(companion / "bin/nuphus-mcp-schema-fixed.exe", "modified")
        report = discovery.npm_candidate("@nuphus/nuphus-mcp", modules, self.node, "nuphus-mcp")
        self.assertEqual(report["status"], "modified")
        self.assertFalse(report["update_safe"])
        self.assertEqual(report["paths"]["native_executable"], str(patched))
        self.assertEqual(report["paths"]["original_native_executable"], str(original))

    def wheel(self):
        root = self.root / "uv/serena-agent"
        site = root / "Lib/site-packages"
        dist = site / "serena_agent-1.7.0.dist-info"
        module = write(site / "serena/cli.py", "# local installed fixture\n")
        metadata = write(dist / "METADATA", "Name: serena-agent\nVersion: 1.7.0\n")
        write(root / "Scripts/python.exe")
        write(root / "uv-receipt.toml", '[tool]\nrequirements = [{name = "serena-agent"}]\n')
        with (dist / "RECORD").open("w", encoding="utf-8", newline="") as stream:
            writer = csv.writer(stream)
            for path in (module, metadata):
                digest = base64.urlsafe_b64encode(hashlib.sha256(path.read_bytes()).digest()).rstrip(b"=").decode()
                writer.writerow([path.relative_to(site).as_posix(), "sha256=" + digest, path.stat().st_size])
        return root, dist, module

    def test_modified_python_wrapper_is_detected(self):
        root, dist, module = self.wheel()
        self.assertEqual(discovery.verify_record(dist, root, package_dirs=("serena",))["state"], "record-matches")
        write(module, "# user's local modification\n")
        self.assertEqual(discovery.verify_record(dist, root, package_dirs=("serena",))["state"], "modified")
        spec = next(x for x in discovery.read_json(discovery.CATALOGUE)["mcp"] if x["id"] == "serena")
        candidate = discovery.uv_candidate(spec, root.parent, False)
        self.assertEqual(candidate["status"], "modified")
        self.assertFalse(candidate["update_safe"])

    def test_record_path_escape_is_reported_without_reading_target(self):
        root, dist, _module = self.wheel()
        with (dist / "RECORD").open("a", encoding="utf-8") as stream:
            stream.write("../../../../../outside-secret.py,sha256=ignored,2\n")
        report = discovery.verify_record(dist, root, full=True)
        self.assertEqual(report["state"], "modified")
        self.assertTrue(any(x["reason"] == "record-path-outside-installation" for x in report["issues"]))

    def test_graphify_manifest_serializes_only_whitelisted_nonsecret_fields(self):
        path = write(self.root / "manifest.json", json.dumps({"secret": "DO-NOT-PRINT", "graphify": {"apiKey": "DO-NOT-PRINT", "configuration": {"python": {"path": "python.exe"}, "graph": {"path": "graph.json", "token": "DO-NOT-PRINT"}, "module": {"name": "graphify.serve", "packageVersion": "0.9.44"}}}}))
        result = discovery.Discovery(self.root, include_process_environment=False, graphify_manifest=path).graphify_service()
        self.assertNotIn("DO-NOT-PRINT", json.dumps(result))
        self.assertEqual(result["graph_path"], "graph.json")

    def test_multiple_installations_require_selection(self):
        spec = {"id": "fixture", "package": "fixture", "manager": "npm", "source": "https://example.invalid"}
        first = discovery.native_candidate(write(self.root / "one/tool.exe"), "fixture", "1")
        second = discovery.native_candidate(write(self.root / "two/tool.exe"), "fixture", "2")
        record = discovery.select(discovery.base_record(spec), [first, second])
        self.assertEqual(record["status"], "ambiguous")
        self.assertIsNone(record["executable"])
        self.assertFalse(record["update_safe"])

    def test_consumer_matching_preserves_no_commandline_or_secret(self):
        root = str(self.root / "node_modules/fixture")
        record = {"installation_root": root}
        discovery.attach_consumers([record], [
            {"ProcessId": 17, "ExecutablePath": str(self.node), "CommandLine": 'node "' + root + '/bin/cli.js" --token DO-NOT-PRINT'},
            {"ProcessId": 18, "ExecutablePath": str(self.node), "CommandLine": 'node "' + root + '-other/bin/cli.js"'},
        ])
        self.assertEqual([x["pid"] for x in record["active_consumers"]["processes"]], [17])
        self.assertNotIn("DO-NOT-PRINT", json.dumps(record))
        self.assertNotIn("CommandLine", json.dumps(record))


if __name__ == "__main__":
    unittest.main()
