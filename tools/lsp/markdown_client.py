"""Client methods required by the existing official VS Code Markdown LSP."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess
from urllib.parse import unquote, urlparse


def default_settings():
    return {"markdown": {"server": {"log": "off"}, "preferredMdPathExtensionStyle": "auto",
        "suggest": {"paths": {"enabled": True}}, "validate": {"enabled": True,
            "fileLinks": {"enabled": "error", "markdownFragmentLinks": "inherit"},
            "referenceLinks": {"enabled": "error"}, "fragmentLinks": {"enabled": "error"},
            "unusedLinkDefinitions": {"enabled": "warning"}, "duplicateLinkDefinitions": {"enabled": "error"}, "ignoredLinks": []}}}


class MarkdownClient:
    def __init__(self, backend):
        self.backend = backend
        self.cache = {}
        self.watchers = {}
        connection = backend.connection
        connection.on_request("markdown/parse", self.parse)
        connection.on_request("markdown/fs/readFile", lambda params: list(self.path(params).read_bytes()))
        connection.on_request("markdown/fs/readDirectory", self.read_directory)
        connection.on_request("markdown/fs/stat", self.stat)
        connection.on_request("markdown/findMarkdownFilesInWorkspace", self.find)
        connection.on_request("markdown/fs/watcher/create", self.watch)
        connection.on_request("markdown/fs/watcher/delete", lambda params: self.watchers.pop(params["id"], None) and None)
        connection.on_request("workspace/diagnostic/refresh", lambda _: None)

    def path(self, params):
        uri = urlparse(params["uri"])
        if uri.scheme != "file" or uri.netloc not in ("", "localhost"):
            raise ValueError("Markdown resource is outside the applicable local workspace")
        decoded = unquote(uri.path)
        if os.name == "nt" and len(decoded) > 2 and decoded[0] == "/" and decoded[2] == ":":
            decoded = decoded[1:]
        path = Path(decoded).resolve()
        if not path.is_relative_to(self.backend.root):
            raise ValueError("Markdown resource requires an additional workspace root")
        return path

    def parse(self, params):
        text = params.get("text")
        if text is None:
            path = self.path(params)
            document = self.backend.documents.get(path.relative_to(self.backend.root).as_posix())
            text = document["text"] if document else path.read_text(encoding="utf-8-sig")
        signature = hashlib.sha256(text.encode("utf-8")).hexdigest()
        if signature not in self.cache:
            parser = self.backend.definition.get("parser_path")
            if not parser or not Path(parser).exists():
                raise FileNotFoundError("Installed Markdown parser is unavailable; runtime installation is disabled")
            response = subprocess.run([self.backend.definition["command"][0], str(Path(__file__).with_name("markdown-parse.cjs")), parser],
                input=json.dumps({"text": text}).encode("utf-8"), capture_output=True, timeout=10,
                creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0, check=False)
            if response.returncode:
                raise RuntimeError("Markdown parser failed: " + response.stderr.decode("utf-8", errors="replace")[-1000:])
            if len(self.cache) >= 128:
                self.cache.clear()
            self.cache[signature] = json.loads(response.stdout)
        return self.cache[signature]

    def stat(self, params):
        path = self.path(params)
        return {"isDirectory": path.is_dir()} if path.exists() else None

    def read_directory(self, params):
        path = self.path(params)
        return [[item.name, {"isDirectory": item.is_dir()}] for item in path.iterdir()
            if item.resolve().is_relative_to(self.backend.root)]

    def find(self, params):
        from journal import snapshot
        from backend import language_for
        files, problems = snapshot(self.backend.root)
        if problems:
            raise RuntimeError("Markdown workspace scan incomplete: " + "; ".join(problems))
        return [self.backend.document_uri(self.backend.root / name) for name in files if language_for(self.backend.root / name) == "markdown"]

    def watch(self, params):
        self.path(params)
        self.watchers[params["id"]] = {**params, "revision": self.revision(params)}
        return None

    def revision(self, params):
        path = self.path(params)
        if path.is_file():
            return hashlib.sha256(path.read_bytes()).hexdigest()
        if path.is_dir():
            return tuple(sorted(item.name for item in path.iterdir()))
        return None

    def reconcile(self):
        for identifier, params in list(self.watchers.items()):
            now = self.revision(params)
            previous = params["revision"]
            if now != previous:
                params["revision"] = now
                kind = "delete" if now is None else "create" if previous is None else "change"
                self.backend.connection.send_request("markdown/fs/watcher/onChange", {"id": identifier, "uri": params["uri"], "kind": kind})
