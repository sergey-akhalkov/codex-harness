"""Run adopted Serena without its implicit dependency installation/update paths.

The compatibility seam is deliberately limited to Serena 1.7.0. Native
ls_base_cmd/ls_args settings select existing provider-backed servers; the older
PowerShell/Pascal constructors need read-only setup replacements. No upstream
file or shared Serena configuration is changed.
"""
from __future__ import annotations

import importlib.metadata
import atexit
import json
import os
from pathlib import Path
import sys
from types import SimpleNamespace
import uuid

sys.dont_write_bytecode = True


class RuntimeProvisioningDenied(RuntimeError):
    pass


def unavailable(detail):
    raise RuntimeProvisioningDenied(
        f"Harness Serena: {detail}. Run the kit's explicit dependency lifecycle; "
        "installation and updates are disabled during MCP sessions."
    )


def require_file(value, label):
    if not value or not Path(value).is_file():
        unavailable(f"missing adopted {label}")
    return str(Path(value).resolve())


def install_runtime_guard(inventory):
    if importlib.metadata.version("serena-agent") != "1.7.0":
        unavailable("Serena adapter compatibility has not been verified for this version")

    from serena.config.serena_config import SerenaConfig
    from solidlsp.dependency_provider import LanguageServerDependencyProviderBaseCommand
    from solidlsp.language_servers.common import RuntimeDependencyCollection
    from solidlsp.language_servers.pascal_server import PascalLanguageServer
    from solidlsp.language_servers.powershell_language_server import PowerShellLanguageServer
    from solidlsp.language_servers import powershell_language_server as powershell_module
    from solidlsp.ls import DEFAULT_LS_REQUEST_TIMEOUT, SolidLanguageServer

    records = {item["id"]: item for item in inventory.get("languages", [])}
    private_temp = None

    def powershell_temp():
        nonlocal private_temp
        if private_temp is None:
            runtime = Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))) / "harness/runtime/serena"
            runtime.mkdir(parents=True, exist_ok=True)
            private_temp = runtime / f"{os.getpid()}_{uuid.uuid4().hex}"
            private_temp.mkdir()
            # Save the resolved parent once. Cleanup rejects changed links and
            # removes directory links themselves, never their targets.
            parent = runtime.resolve()

            def cleanup():
                if private_temp.parent.resolve() != parent:
                    return

                def remove_owned(path):
                    if path.is_symlink() or path.is_junction():
                        if path.is_dir():
                            path.rmdir()
                        else:
                            path.unlink()
                    elif path.is_dir():
                        for child in path.iterdir():
                            remove_owned(child)
                        path.rmdir()
                    elif path.exists():
                        path.unlink()
                try:
                    remove_owned(private_temp)
                except OSError:
                    # A forced exit/file still in use leaves only this PID's
                    # owned directory for explicit lifecycle recovery.
                    pass

            atexit.register(cleanup)
        return str(private_temp)

    # This legacy backend uses fixed names beneath tempfile.gettempdir().
    # Replace its module-local handle, leaving Python's global tempfile alone.
    powershell_module.tempfile = SimpleNamespace(gettempdir=powershell_temp)
    allowed = {"typescript": "typescript", "rust": "rust", "python": "python",
               "python_basedpyright": "python", "powershell": "powershell", "pascal": "delphi"}

    def record(ls_id):
        key = allowed.get(ls_id)
        item = records.get(key, {})
        if item.get("status") not in ("present", "adopted", "installed", "ready", "verified"):
            unavailable(f"{ls_id} has no verified reusable dependency ({item.get('status', 'unmapped')})")
        return item

    original_create = SolidLanguageServer.create.__func__

    def create(cls, config, repository_root_path, timeout=DEFAULT_LS_REQUEST_TIMEOUT, solidlsp_settings=None):
        # Gate before a backend constructor can create an install directory,
        # consult a release API, invoke rustup/uvx/npm, or download a binary.
        ls_id = config.ls_id.value
        item = record(ls_id)
        if ls_id not in ("powershell", "pascal"):
            from solidlsp.settings import SolidLSPSettings
            if solidlsp_settings is None:
                solidlsp_settings = SolidLSPSettings()
            settings = solidlsp_settings.ls_specific_settings.setdefault(config.ls_id, {})
            paths = item.get("paths", {})
            if paths.get("node") and paths.get("entrypoint"):
                command = [require_file(paths["node"], "Node runtime"),
                           require_file(paths["entrypoint"], f"{ls_id} entrypoint")]
            else:
                command = [require_file(paths.get("executable"), f"{ls_id} executable")]
            # Retain all project semantic settings. Process selection belongs to
            # the adopted dependency registry, never a runtime package manager.
            settings["ls_base_cmd"] = command
            settings["ls_args"] = [] if ls_id == "rust" else ["--stdio"]
        return original_create(cls, config, repository_root_path, timeout, solidlsp_settings)

    def powershell_setup(cls, settings):
        item = record("powershell")
        command = item.get("command", [])
        pwsh = require_file(command[0] if command else None, "PowerShell runtime")
        script = require_file(item.get("paths", {}).get("executable"), "PSES start script")
        analyzers = item.get("analyzers", [])
        if not any(Path(analyzer.get("path", "")).is_file() for analyzer in analyzers):
            unavailable("PSScriptAnalyzer is absent from the adopted PSES installation")
        return pwsh, script, str(Path(script).parent)

    def pascal_setup(cls, settings):
        return require_file(record("pascal").get("paths", {}).get("executable"), "Pascal executable")

    def deny_install(*args, **kwargs):
        unavailable("an upstream dependency installer was invoked")

    original_command = LanguageServerDependencyProviderBaseCommand.create_launch_command

    def explicit_command(provider):
        if not provider._custom_settings.get("ls_base_cmd") and not provider._custom_settings.get("ls_path"):
            unavailable("language server has no explicit existing launch command")
        return original_command(provider)

    SolidLanguageServer.create = classmethod(create)
    LanguageServerDependencyProviderBaseCommand.create_launch_command = explicit_command
    RuntimeDependencyCollection.install = deny_install
    PowerShellLanguageServer._setup_runtime_dependency = classmethod(powershell_setup)
    PascalLanguageServer._setup_runtime_dependencies = classmethod(pascal_setup)

    # A Codex session may read the existing user configuration and registered
    # projects, but must not migrate/rewrite the configuration used by OpenCode.
    original_load = SerenaConfig.from_config_file.__func__

    def load_config(cls, generate_if_missing=True):
        if not Path(cls._determine_config_file_path()).is_file():
            return cls()
        result = original_load(cls, generate_if_missing=False)
        result._config_file_path = None
        return result

    SerenaConfig._save = lambda *args, **kwargs: None
    SerenaConfig._persist_projects = lambda *args, **kwargs: None
    SerenaConfig._migrate_out_of_project_config_file = classmethod(lambda cls, path: None)
    SerenaConfig.from_config_file = classmethod(load_config)


def main():
    registry = os.environ.get("HARNESS_CODE_TOOLS_REGISTRY")
    if not registry:
        unavailable("the adopted dependency registry is not configured")
    inventory = json.loads(Path(registry).read_text(encoding="utf-8-sig"))
    install_runtime_guard(inventory)
    from serena.cli import top_level
    top_level()


if __name__ == "__main__":
    try:
        main()
    except RuntimeProvisioningDenied as error:
        print(error, file=sys.stderr)
        raise SystemExit(1)
