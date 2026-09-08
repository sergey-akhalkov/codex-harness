"""Direct existing Nuphus schema inspection; no desktop calls in default mode."""
import json
from io import TextIOBase
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
from collections.abc import Iterator
from typing import TypeGuard, cast, final


class JSONObject(dict[str, object]):
    """JSON decoder object hook: nested objects retain a checkable runtime type."""


def object_value(value: object) -> JSONObject:
    if not isinstance(value, JSONObject):
        raise ValueError("Expected a JSON object")
    return value


def load_object(text: str) -> JSONObject:
    # Erase the decoder's Any at the untrusted boundary, then validate below.
    value = cast(object, json.loads(text, object_hook=JSONObject))
    return object_value(value)


def is_array(value: object) -> TypeGuard[list[object]]:
    return isinstance(value, list)


def object_array(value: object) -> list[JSONObject]:
    if not is_array(value):
        raise ValueError("Expected a JSON array")
    return [object_value(item) for item in value]


def string_value(value: object) -> str:
    if not isinstance(value, str):
        raise ValueError("Expected a JSON string")
    return value


def text_lines(stream: TextIOBase) -> Iterator[str]:
    while True:
        line = string_value(cast(object, stream.readline()))
        if not line:
            return
        yield line


@final
class MCP:
    def __init__(self, executable: str | list[str], environment: dict[str, str] | None = None):
        self.id = 0
        self.lines: queue.Queue[str | None] = queue.Queue()
        self.errors: list[str] = []
        self.process = subprocess.Popen[str](executable if isinstance(executable, list) else [executable], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, text=True, encoding="utf-8",
                                        env={**os.environ, "NUPHUS_MCP_NO_MODEL_DOWNLOAD": "1", **(environment or {})},
                                        creationflags=subprocess.CREATE_NO_WINDOW)
        stdin, stdout, stderr = self.process.stdin, self.process.stdout, self.process.stderr
        assert isinstance(stdin, TextIOBase) and isinstance(stdout, TextIOBase) and isinstance(stderr, TextIOBase), "MCP text pipes were not created"
        self.stdin = stdin
        def read() -> None:
            for line in text_lines(stdout):
                self.lines.put(line)
            self.lines.put(None)
        def errors() -> None:
            for line in text_lines(stderr):
                self.errors.append(line)
        threading.Thread(target=read, daemon=True).start()
        threading.Thread(target=errors, daemon=True).start()
        _ = self.call("initialize", {"protocolVersion":"2024-11-05", "capabilities":{},
                                 "clientInfo":{"name":"harness-nuphus-audit", "version":"1"}})
        _ = self.stdin.write(json.dumps({"jsonrpc":"2.0", "method":"notifications/initialized"}) + "\n")
        self.stdin.flush()

    def call(self, method: str, params: dict[str, object], timeout: float = 30) -> JSONObject:
        self.id += 1
        _ = self.stdin.write(json.dumps({"jsonrpc":"2.0", "id":self.id, "method":method, "params":params}) + "\n")
        self.stdin.flush()
        while True:
            line = self.lines.get(timeout=timeout)
            if line is None:
                raise RuntimeError(f"Nuphus exited: {''.join(self.errors)[-2000:]}")
            message = load_object(line)
            if message.get("id") == self.id:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return object_value(message["result"])

    def close(self) -> None:
        self.stdin.close()
        try:
            _ = self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            _ = self.process.wait(timeout=3)


def main() -> None:
    inventory = load_object(Path(sys.argv[1]).read_text(encoding="utf-8-sig"))
    record = next(item for item in object_array(inventory["mcp"]) if item["id"] == "nuphus")
    output = Path(sys.argv[2])
    output.mkdir(exist_ok=False)
    schemas: dict[str, dict[str, JSONObject]] = {}
    for variant, key in [("original", "original_native_executable"), ("schema-fixed", "native_executable")]:
        mcp = MCP(string_value(object_value(record["paths"])[key]))
        try:
            result = mcp.call("tools/list", {})
            listed_tools = object_array(result["tools"])
            schemas[variant] = {string_value(item["name"]): item for item in listed_tools}
            _ = (output / f"{variant}.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
            print(f"{variant}: {len(listed_tools)} tools")
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
