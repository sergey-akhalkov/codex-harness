"""Exercise installed upstream provisioning seams without launching/installing tools."""
import importlib.util
from importlib import import_module
import json
import os
from pathlib import Path
import sys
import subprocess
from collections.abc import Callable
from typing import Protocol, TypeGuard, cast, runtime_checkable


class JSONObject(dict[str, object]):
    """Preserve registry object identity while checking decoded values."""


def is_array(value: object) -> TypeGuard[list[object]]:
    return isinstance(value, list)


@runtime_checkable
class GuardModule(Protocol):
    RuntimeProvisioningDenied: type[RuntimeError]

    def install_runtime_guard(self, inventory: dict[str, object]) -> None: ...

entry, registry, probe = sys.argv[1:4]
os.environ["CODEX_HOME"] = str(Path(probe) / "guard-codex-home")
spec = importlib.util.spec_from_file_location("serena_entry", entry)
if spec is None or spec.loader is None:
    raise ImportError("Cannot load the Serena runtime guard")
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
if not isinstance(guard, GuardModule):
    raise TypeError("Serena entry point does not expose the runtime guard")
guard_api: GuardModule = guard
inventory = cast(object, json.loads(Path(registry).read_text(encoding="utf-8-sig"), object_hook=JSONObject))
assert isinstance(inventory, JSONObject), "Registry must be a JSON object"
guard_api.install_runtime_guard(inventory)

class DependencyCollection(Protocol):
    def install(self, target_dir: str) -> object: ...


@runtime_checkable
class CommonModule(Protocol):
    def RuntimeDependencyCollection(self, dependencies: list[object]) -> DependencyCollection: ...


class ServerFactory(Protocol):
    def create(self, config: object, repository_root_path: str) -> object: ...


@runtime_checkable
class LsModule(Protocol):
    SolidLanguageServer: ServerFactory


class ServerIds(Protocol):
    JAVA: object


@runtime_checkable
class ConfigModule(Protocol):
    LanguageServerId: ServerIds
    def LanguageServerConfig(self, *, ls_id: object) -> object: ...


class TempDirectory(Protocol):
    def gettempdir(self) -> str: ...


@runtime_checkable
class PowerShellModule(Protocol):
    tempfile: TempDirectory
    PowerShellLanguageServer: object


@runtime_checkable
class PascalModule(Protocol):
    PascalLanguageServer: object


# The guard replaces these APIs at runtime. Describe and check that patched
# interface here; upstream SolidLSP does not publish a py.typed/stub contract.
common = import_module('solidlsp.language_servers.common')
ls = import_module('solidlsp.ls')
config = import_module('solidlsp.ls_config')
powershell_language_server = import_module('solidlsp.language_servers.powershell_language_server')
pascal_module = import_module('solidlsp.language_servers.pascal_server')
assert isinstance(common, CommonModule) and isinstance(ls, LsModule)
assert isinstance(config, ConfigModule) and isinstance(powershell_language_server, PowerShellModule)
assert isinstance(pascal_module, PascalModule)
common_api: CommonModule = common
ls_api: LsModule = ls
config_api: ConfigModule = config

owned_temp = powershell_language_server.tempfile.gettempdir()
if "--temp-child" in sys.argv:
    print(owned_temp)
    raise SystemExit(0)
assert powershell_language_server.tempfile.gettempdir() == owned_temp
child = subprocess.run([sys.executable, "-B", __file__, entry, registry, probe, "--temp-child"],
                       text=True, capture_output=True, check=True, timeout=20)
child_temp = child.stdout.strip()
assert owned_temp != child_temp, "Concurrent consumers share a PowerShell temp directory"
assert Path(owned_temp).is_dir() and not Path(child_temp).exists(), "Child cleanup affected another consumer"


def denied(action: Callable[[], object]) -> None:
    try:
        _ = action()
    except guard_api.RuntimeProvisioningDenied as error:
        assert "disabled during MCP sessions" in str(error)
    else:
        raise AssertionError("Provisioning guard did not reject operation")


target = Path(probe) / "must-not-be-created"
denied(lambda: common_api.RuntimeDependencyCollection([]).install(str(target)))
assert not target.exists(), "Dependency installer mutated destination before denial"

# All unsupported constructors are rejected before their provisioning code.
denied(lambda: ls_api.SolidLanguageServer.create(config_api.LanguageServerConfig(ls_id=config_api.LanguageServerId.JAVA), probe))

# Existing Pascal executable is selected without calling upstream's release
# lookup, checksum/download, metadata rewrite or replacement methods.
@runtime_checkable
class PascalSetup(Protocol):
    def __call__(self, settings: None) -> str: ...


@runtime_checkable
class PowerShellSetup(Protocol):
    def __call__(self, settings: None) -> tuple[str, str, str]: ...


# These private upstream methods are replaced by the guard above. Check their
# callable contracts at this deliberate compatibility seam before exercising them.
pascal_method = cast(object, getattr(pascal_module.PascalLanguageServer, "_setup_runtime_dependencies"))
powershell_method = cast(object, getattr(powershell_language_server.PowerShellLanguageServer, "_setup_runtime_dependency"))
assert isinstance(pascal_method, PascalSetup) and isinstance(powershell_method, PowerShellSetup)
pascal_setup: PascalSetup = pascal_method
powershell_setup: PowerShellSetup = powershell_method
pascal = pascal_setup(None)
assert Path(pascal).is_file()
ps = powershell_setup(None)
assert all(Path(path).exists() for path in ps)

languages = inventory["languages"]
assert is_array(languages)
records: dict[str, JSONObject] = {}
for item in languages:
    assert isinstance(item, JSONObject)
    identifier = item["id"]
    assert isinstance(identifier, str)
    records[identifier] = item
records["powershell"]["analyzers"] = []
denied(lambda: powershell_setup(None))
pascal_paths = records["delphi"]["paths"]
assert isinstance(pascal_paths, JSONObject)
pascal_paths["executable"] = str(target / "pasls.exe")
denied(lambda: pascal_setup(None))
assert not target.exists()
print("PASS collection, unmapped constructor, Pascal reuse/missing, PowerShell reuse/missing analyzer")
