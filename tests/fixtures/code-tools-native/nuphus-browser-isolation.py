"""Exercise real owned browsers and cleanup containment without accessing user state."""
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import os
from pathlib import Path
import sys
from urllib.request import urlopen

source, probe = (Path(arg).resolve() for arg in sys.argv[1:3])
probe.mkdir(exist_ok=False)
os.environ["CODEX_HOME"] = str(probe)
spec = importlib.util.spec_from_file_location("nuphus_proxy", source / "tools/code-tools/nuphus_proxy.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
first, second = module.OwnedBrowser(), module.OwnedBrowser()
try:
    with ThreadPoolExecutor(max_workers=2) as pool:
        tasks = [pool.submit(first.ensure_started) for _ in range(2)]
        for task in tasks:
            task.result()
    runtime = probe / "harness/runtime/nuphus"
    assert len(list(runtime.iterdir())) == 1, "Concurrent first calls launched multiple browsers"
    second.ensure_started()
    assert first.process.pid != second.process.pid and first.endpoint != second.endpoint
    assert first.directory != second.directory
    sentinel = probe / "foreign"
    sentinel.mkdir()
    (sentinel / "keep.txt").write_text("foreign sentinel", encoding="utf-8")
    (first.directory / "foreign-link").symlink_to(sentinel, target_is_directory=True)
    first.close()
    assert (sentinel / "keep.txt").read_text() == "foreign sentinel"
    with urlopen(second.endpoint + "/json/version", timeout=5) as response:
        assert json.load(response)["webSocketDebuggerUrl"]
    second.close()
    assert not list(runtime.iterdir())
    print("PASS concurrent first-call reuse, distinct profiles, foreign symlink preservation, independent shutdown, owned cleanup")
finally:
    first.close()
    second.close()
    print(f"Owned browser evidence: {probe}")
