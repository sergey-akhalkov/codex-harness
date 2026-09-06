"""Direct existing Nuphus schema inspection; no desktop calls in default mode."""
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading


class MCP:
    def __init__(self, executable, environment=None):
        self.id = 0
        self.lines = queue.Queue()
        self.errors = []
        self.process = subprocess.Popen(executable if isinstance(executable, list) else [executable], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, text=True, encoding="utf-8",
                                        env={**os.environ, "NUPHUS_MCP_NO_MODEL_DOWNLOAD": "1", **(environment or {})},
                                        creationflags=subprocess.CREATE_NO_WINDOW)
        def read():
            for line in self.process.stdout:
                self.lines.put(line)
            self.lines.put(None)
        def errors():
            for line in self.process.stderr:
                self.errors.append(line)
        threading.Thread(target=read, daemon=True).start()
        threading.Thread(target=errors, daemon=True).start()
        self.call("initialize", {"protocolVersion":"2024-11-05", "capabilities":{},
                                 "clientInfo":{"name":"harness-nuphus-audit", "version":"1"}})
        self.process.stdin.write(json.dumps({"jsonrpc":"2.0", "method":"notifications/initialized"}) + "\n")
        self.process.stdin.flush()

    def call(self, method, params, timeout=30):
        self.id += 1
        self.process.stdin.write(json.dumps({"jsonrpc":"2.0", "id":self.id, "method":method, "params":params}) + "\n")
        self.process.stdin.flush()
        while True:
            line = self.lines.get(timeout=timeout)
            if line is None:
                raise RuntimeError(f"Nuphus exited: {''.join(self.errors)[-2000:]}")
            message = json.loads(line)
            if message.get("id") == self.id:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message["result"]

    def close(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(timeout=3)


def main():
    inventory = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8-sig"))
    record = next(item for item in inventory["mcp"] if item["id"] == "nuphus")
    output = Path(sys.argv[2])
    output.mkdir(exist_ok=False)
    schemas = {}
    for variant, key in [("original", "original_native_executable"), ("schema-fixed", "native_executable")]:
        mcp = MCP(record["paths"][key])
        try:
            result = mcp.call("tools/list", {})
            schemas[variant] = {item["name"]: item for item in result["tools"]}
            (output / f"{variant}.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
            print(f"{variant}: {len(result['tools'])} tools")
        finally:
            mcp.close()
    for name in sorted(set(schemas["original"]) | set(schemas["schema-fixed"])):
        left = schemas["original"].get(name)
        right = schemas["schema-fixed"].get(name)
        if left != right:
            print(f"CHANGED {name}")
    print(f"Schema evidence: {output}")


if __name__ == "__main__":
    main()
