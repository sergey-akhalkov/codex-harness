"""Exercise installed upstream provisioning seams without launching/installing tools."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import subprocess

entry, registry, probe = sys.argv[1:4]
os.environ["CODEX_HOME"] = str(Path(probe) / "guard-codex-home")
spec = importlib.util.spec_from_file_location("serena_entry", entry)
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
inventory = json.loads(Path(registry).read_text(encoding="utf-8-sig"))
guard.install_runtime_guard(inventory)

from solidlsp.language_servers.common import RuntimeDependencyCollection
from solidlsp.language_servers.pascal_server import PascalLanguageServer
from solidlsp.language_servers.powershell_language_server import PowerShellLanguageServer
from solidlsp.ls import SolidLanguageServer
from solidlsp.ls_config import LanguageServerId
from solidlsp.language_servers import powershell_language_server

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


def denied(action):
    try:
        action()
    except guard.RuntimeProvisioningDenied as error:
        assert "disabled during MCP sessions" in str(error)
    else:
        raise AssertionError("Provisioning guard did not reject operation")


target = Path(probe) / "must-not-be-created"
denied(lambda: RuntimeDependencyCollection.install(None, str(target)))
assert not target.exists(), "Dependency installer mutated destination before denial"

# All unsupported constructors are rejected before their provisioning code.
class MissingBackend:
    ls_id = LanguageServerId.JAVA

denied(lambda: SolidLanguageServer.create(MissingBackend(), probe))

# Existing Pascal executable is selected without calling upstream's release
# lookup, checksum/download, metadata rewrite or replacement methods.
pascal = PascalLanguageServer._setup_runtime_dependencies(None)
assert Path(pascal).is_file()
ps = PowerShellLanguageServer._setup_runtime_dependency(None)
assert all(Path(path).exists() for path in ps)

records = {item["id"]: item for item in inventory["languages"]}
records["powershell"]["analyzers"] = []
denied(lambda: PowerShellLanguageServer._setup_runtime_dependency(None))
records["delphi"]["paths"]["executable"] = str(target / "pasls.exe")
denied(lambda: PascalLanguageServer._setup_runtime_dependencies(None))
assert not target.exists()
print("PASS collection, unmapped constructor, Pascal reuse/missing, PowerShell reuse/missing analyzer")
