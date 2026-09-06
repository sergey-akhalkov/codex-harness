"""Deliberately delay only this disposable adapter's MCP handshake."""
import json
from pathlib import Path
import runpy
import sys
import time

entry, marker = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve()
if not marker.parent.name.startswith("lsp-native-"):
    raise RuntimeError("Delayed handshake marker must be inside a disposable probe")
marker.write_text(json.dumps({"started": time.time(), "delay_seconds": 35}), encoding="utf-8")
time.sleep(35)
sys.path.insert(0, str(entry.parent))
sys.argv = [str(entry)]
runpy.run_path(str(entry), run_name="__main__")
