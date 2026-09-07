"""Client methods required by the existing official VS Code Markdown LSP."""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import subprocess
from urllib.parse import unquote, urlparse, urljoin


MAX_RESOURCE_BYTES = 8 * 1024 * 1024


def default_settings():
    return {"markdown": {"server": {"log": "off"}, "preferredMdPathExtensionStyle": "auto",
        "occurrencesHighlight": {"enabled": False},
        "suggest": {"paths": {"enabled": True}}, "validate": {"enabled": True,
            "fileLinks": {"enabled": "error", "markdownFragmentLinks": "inherit"},
            "referenceLinks": {"enabled": "error"}, "fragmentLinks": {"enabled": "error"},
            "unusedLinkDefinitions": {"enabled": "warning"}, "duplicateLinkDefinitions": {"enabled": "error"}, "ignoredLinks": []}}}


class MarkdownClient:
    def __init__(self, backend):
        self.backend = backend
        self.cache = {}
        self.watchers = {}
        self.linked_files = {}
        connection = backend.connection
        connection.on_request("markdown/parse", self.parse)
        connection.on_request("markdown/fs/readFile", lambda params: list(self.read_file(self.path(params))))
        connection.on_request("markdown/fs/readDirectory", self.read_directory)
        connection.on_request("markdown/fs/stat", self.stat)
        connection.on_request("markdown/findMarkdownFilesInWorkspace", self.find)
        connection.on_request("markdown/fs/watcher/create", self.watch)
        connection.on_request("markdown/fs/watcher/delete", lambda params: self.watchers.pop(params["id"], None) and None)
        connection.on_request("workspace/diagnostic/refresh", lambda _: None)

    @staticmethod
    def local_path(value):
        uri = urlparse(value)
        if uri.scheme != "file" or uri.netloc not in ("", "localhost"):
            raise ValueError("Markdown resource is outside the applicable local workspace")
        decoded = unquote(uri.path)
        if decoded.replace("\\", "/").startswith("//"):
            raise ValueError("Markdown network resources are not local dependencies")
        if os.name == "nt" and len(decoded) > 2 and decoded[0] == "/" and decoded[2] == ":":
            decoded = decoded[1:]
        return Path(decoded).resolve()

    def path(self, params):
        path = self.local_path(params["uri"])
        if not path.is_relative_to(self.backend.root) and not any(path in paths for paths in self.linked_files.values()):
            raise ValueError("Markdown resource requires an additional workspace root")
        return path

    @staticmethod
    def read_file(path):
        with path.open("rb") as stream:
            content = stream.read(MAX_RESOURCE_BYTES + 1)
        if len(content) > MAX_RESOURCE_BYTES:
            raise ValueError("Markdown resource exceeds the 8 MiB read limit")
        return content

    def remember_links(self, source, tokens):
        # Only workspace documents can grant exact read-only dependencies.
        # Linked Markdown must not transitively expose the surrounding tree.
        if not source.is_relative_to(self.backend.root):
            return
        linked = set()
        pending = list(tokens)
        while pending:
            token = pending.pop()
            pending.extend(token.get("children") or [])
            for key, value in token.get("attrs") or []:
                if key not in ("href", "src"):
                    continue
                uri = urlparse(value)
                if uri.scheme not in ("", "file") or uri.netloc not in ("", "localhost") or not uri.path:
                    continue
                # Leading slash links are workspace-relative in the VS Code LS.
                base = self.backend.root.as_uri() + "/" if value.startswith("/") else source.as_uri()
                try:
                    target = self.local_path(urljoin(base, value.lstrip("/") if value.startswith("/") else value))
                except ValueError:
                    continue
                if not target.is_relative_to(self.backend.root):
                    linked.add(target)
                    # The server probes extensionless Markdown destinations.
                    if not target.suffix:
                        linked.add(target.with_suffix(".md"))
                if len(linked) > 2048:
                    raise ValueError("Markdown linked resource limit exceeded")
        self.linked_files[source] = linked

    def parse(self, params):
        path = self.path(params)
        text = params.get("text")
        if text is None:
            document = self.backend.documents.get(path.relative_to(self.backend.root).as_posix()) if path.is_relative_to(self.backend.root) else None
            text = document["text"] if document else self.read_file(path).decode("utf-8-sig")
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
        tokens = self.cache[signature]
        self.remember_links(path, tokens)
        return tokens

    def stat(self, params):
        path = self.path(params)
        return {"isDirectory": path.is_dir()} if path.exists() else None

    def read_directory(self, params):
        path = self.path(params)
        if not path.is_relative_to(self.backend.root):
            return []
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
            return hashlib.sha256(self.read_file(path)).hexdigest()
        if path.is_dir():
            if not path.is_relative_to(self.backend.root):
                return ("directory",)
            return tuple(sorted(item.name for item in path.iterdir()))
        return None

    def reconcile(self):
        for identifier, params in list(self.watchers.items()):
            try:
                now = self.revision(params)
            except ValueError:
                # A document can remove a formerly allowed external link.
                self.watchers.pop(identifier, None)
                continue
            previous = params["revision"]
            if now != previous:
                params["revision"] = now
                kind = "delete" if now is None else "create" if previous is None else "change"
                # The custom watcher tracks existence (ignoreChange=true).
                # The standard workspace event invalidates cached target text.
                self.backend.connection.send_notification("workspace/didChangeWatchedFiles", {
                    "changes": [{"uri": params["uri"], "type": {"create": 1, "change": 2, "delete": 3}[kind]}]})
                self.backend.connection.send_request("markdown/fs/watcher/onChange", {"id": identifier, "uri": params["uri"], "kind": kind})
