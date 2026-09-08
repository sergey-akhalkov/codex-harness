"""Drop-in stdio client for project-isolated shared Serena workers."""
from __future__ import annotations

import json
import os
import secrets
import sys
from typing import Any

from serena_broker import MAX_MESSAGE, ensure_endpoint, exchange, source_identity


def main() -> None:
    expected = source_identity()
    client = secrets.token_hex(16)
    endpoint = None
    connected_token = None
    initialize = None
    route = None
    initialized_message = None

    def output(value: dict[str, Any]) -> None:
        raw = json.dumps(value, ensure_ascii=False).encode("utf-8") + b"\n"
        if len(raw) > MAX_MESSAGE:
            raise ValueError("Serena response exceeds transport bound")
        sys.stdout.buffer.write(raw)
        sys.stdout.buffer.flush()

    try:
        while True:
            raw = sys.stdin.buffer.readline(MAX_MESSAGE + 1)
            if not raw:
                break
            if len(raw) > MAX_MESSAGE:
                raise ValueError("Serena request exceeds transport bound")
            message = json.loads(raw)
            method = message.get("method")
            if "id" not in message:
                continue
            try:
                if method == "initialize":
                    initialize = message.get("params", {})
                if initialize is None:
                    raise RuntimeError("Serena client must initialize first")
                if source_identity() != expected:
                    raise RuntimeError("Serena proxy source/runtime changed; restart this client")
                endpoint = ensure_endpoint(expected)
                if connected_token != endpoint["token"] or method == "initialize":
                    response = exchange(endpoint, "connect", {"client": client, "cwd": os.getcwd(),
                                         "arguments": sys.argv[1:], "initialize": initialize, "route": route}, 240)
                    connected_token = endpoint["token"]
                    route = response["route"]
                    initialized_message = response["message"]
                if method == "initialize":
                    result = initialized_message
                    native_result = result["result"]
                    capabilities = native_result.get("capabilities", {})
                    if "tools" in capabilities:
                        result = {**result, "result": {**native_result, "capabilities": {
                            **capabilities, "tools": {**capabilities["tools"], "listChanged": True}}}}
                else:
                    response = exchange(endpoint, "rpc", {"client": client, "method": method,
                                         "params": message.get("params", {}), "route": route,
                                         "initialize": initialize}, 240)
                    route = response["route"]
                    result = response["message"]
                    if response.get("tools_changed"):
                        output({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"})
                output({**result, "jsonrpc": "2.0", "id": message["id"]})
            except Exception as error:
                output({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32603, "message": f"{type(error).__name__}: {error}"}})
    finally:
        if endpoint and connected_token:
            try:
                exchange(endpoint, "disconnect", {"client": client}, 1)
            except Exception:
                pass


if __name__ == "__main__":
    main()
