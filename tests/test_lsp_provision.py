"""Missing-provider ownership, checksum and inverse failure paths; no downloads."""
from __future__ import annotations

import hashlib
import importlib.util
import io
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from dataclasses import dataclass
from typing import override, Protocol, runtime_checkable
from collections.abc import Mapping
from unittest import mock
import zipfile


ROOT = Path(__file__).resolve().parents[1]
class DiscoveryAPI(Protocol):
    def fingerprint(self, path: Path) -> str: ...


class Provider(Protocol):
    def provision(self, identifier: str, user_home: Path, state_dir: Path, *, lifecycle: Dependencies) -> Mapping[str, object]: ...
    def provision_typescript(self, current: object, state_dir: Path, version: str | None, *, lifecycle: Dependencies) -> Mapping[str, object]: ...
    def stage_release(self, identifier: str, user_home: Path, state_dir: Path, *, lifecycle: Dependencies) -> object: ...
    def recover_rust(self, journal: Mapping[str, object], path: Path, *, lifecycle: Dependencies) -> Mapping[str, object]: ...


@runtime_checkable
class Dependencies(Protocol):
    discovery: DiscoveryAPI
    def load_lsp_provision(self) -> Provider: ...


def load_dependencies() -> Dependencies:
    spec = importlib.util.spec_from_file_location("dependency_fixture", ROOT / "tools/code-tools/dependencies.py")
    assert spec is not None and spec.loader is not None, "Dependency fixture loader is unavailable"
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    assert isinstance(module, Dependencies)
    return module


dependencies = load_dependencies()
provider = dependencies.load_lsp_provision()


@dataclass
class _Fixture:
    temporary: tempfile.TemporaryDirectory[str]
    root: Path


class ProvisionTests(unittest.TestCase):
    _fixture: _Fixture | None = None

    @property
    def fixture(self) -> _Fixture:
        assert self._fixture is not None, "setUp has not completed"
        return self._fixture

    @override
    def setUp(self) -> None:
        temporary = tempfile.TemporaryDirectory(prefix="harness-providers-")
        root = Path(temporary.name)
        self._fixture = _Fixture(temporary, root)

    @override
    def tearDown(self) -> None:
        self.fixture.temporary.cleanup()

    def test_existing_backend_is_reused_without_metadata_or_duplicate_install(self):
        current = SimpleNamespace(run=lambda: {"languages": [{"id": "cpp", "status": "adopted", "version": "22.1.6"}]})
        with mock.patch.object(dependencies.discovery, "Discovery", return_value=current), mock.patch.object(dependencies, "fetch", side_effect=AssertionError("No download")):
            result = provider.provision("cpp", self.fixture.root, self.fixture.root / "state", lifecycle=dependencies)
        self.assertEqual(result["state"], "reused")
        self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_missing_node_is_explicit_prerequisite_without_installation(self):
        result = provider.provision_typescript(SimpleNamespace(node=None), self.fixture.root, None, lifecycle=dependencies)
        self.assertEqual(result["state"], "prerequisite-missing")
        self.assertEqual(list(self.fixture.root.iterdir()), [])

    def test_bad_official_archive_digest_prevents_extraction(self):
        release = {"tag_name": "22.1.6", "assets": [{"name": "clangd-windows-22.1.6.zip", "browser_download_url": "https://github.com/clangd/clangd/fixture.zip", "digest": "sha256:" + "0" * 64}]}
        with mock.patch.object(dependencies, "fetch_json", return_value=release), mock.patch.object(dependencies, "fetch", return_value=b"changed"):
            with self.assertRaisesRegex(ValueError, "digest"):
                _ = provider.stage_release("cpp", self.fixture.root, self.fixture.root / "state", lifecycle=dependencies)
        self.assertFalse((self.fixture.root / "state").exists())

    def test_powershell_without_its_analyzer_cannot_be_selected(self):
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr("PowerShellEditorServices/Start-EditorServices.ps1", "# fixture")
        raw = buffer.getvalue()
        release = {"tag_name": "v4.7.0", "assets": [{"name": "PowerShellEditorServices.zip", "browser_download_url": "https://github.com/PowerShell/fixture.zip", "digest": "sha256:" + hashlib.sha256(raw).hexdigest()}]}
        current = SimpleNamespace(pwsh="existing-pwsh", serena=self.fixture.root / "shared")
        with mock.patch.object(dependencies.discovery, "Discovery", return_value=current), mock.patch.object(dependencies, "fetch_json", return_value=release), mock.patch.object(dependencies, "fetch", return_value=raw):
            with self.assertRaisesRegex(ValueError, "PSScriptAnalyzer"):
                _ = provider.stage_release("powershell", self.fixture.root, self.fixture.root / "state", lifecycle=dependencies)
        self.assertFalse((self.fixture.root / "shared").exists())

    def rust_state(self):
        installation = self.fixture.root / "toolchain"
        receipt = installation / "lib/rustlib"
        receipt.mkdir(parents=True)
        manifest = receipt / "multirust-channel-manifest.toml"
        _ = manifest.write_text("# existing cohort")
        _ = (receipt / "components").write_text("rustc\nrust-analyzer-preview-fixture\n")
        executable = installation / "bin/rust-analyzer.exe"
        executable.parent.mkdir()
        _ = executable.write_bytes(b"official expected component")
        journal = {"installation": str(installation), "rustup": "existing-rustup", "toolchain": "stable-fixture", "component": "rust-analyzer-preview-fixture",
                   "components_before": ["rustc"], "executable": str(executable), "executable_sha256": dependencies.discovery.fingerprint(executable),
                   "manifest_sha256": dependencies.discovery.fingerprint(manifest), "phase": "prepared"}
        return journal, executable, receipt / "components"

    def test_rust_inverse_preserves_modified_component(self):
        journal, executable, _components = self.rust_state()
        _ = executable.write_bytes(b"operator modification")
        with mock.patch.object(provider, "run", side_effect=AssertionError("Manager must not run")):
            result = provider.recover_rust(journal, self.fixture.root / "journal.json", lifecycle=dependencies)
        self.assertEqual(result["state"], "pending")
        self.assertEqual(executable.read_bytes(), b"operator modification")

    def test_rust_inverse_removes_only_owned_component_after_crash(self):
        journal, executable, components = self.rust_state()
        def remove(command: list[str]) -> None:
            self.assertEqual(command, ["existing-rustup", "component", "remove", "rust-analyzer", "--toolchain", "stable-fixture"])
            executable.unlink()
            _ = components.write_text("rustc\n")
        with mock.patch.object(provider, "run", side_effect=remove):
            result = provider.recover_rust(journal, self.fixture.root / "journal.json", lifecycle=dependencies)
        self.assertEqual(result["state"], "restored")
        self.assertEqual(components.read_text(), "rustc\n")


if __name__ == "__main__":
    _ = unittest.main()
