"""Use already installed language servers through Serena's LSP transport.

No dependency provider is instantiated here: those providers may install packages
at runtime. The TypeScript adapter uses correlated tsserver diagnostic requests;
an unversioned push notification is never relabelled as current after an edit.
"""
from __future__ import annotations

import hashlib
import json
import logging
import os
from pathlib import Path
import re
import shutil
import subprocess
import threading
import time
from urllib.parse import unquote, urlparse


EXTENSIONS = {
    ".ts": "typescript", ".tsx": "typescript", ".mts": "typescript", ".cts": "typescript",
    ".js": "javascript", ".jsx": "javascript", ".mjs": "javascript", ".cjs": "javascript",
    ".rs": "rust", ".ps1": "powershell", ".psm1": "powershell", ".psd1": "powershell",
    ".py": "python", ".pyi": "python", ".pas": "pascal", ".dpr": "pascal", ".dpk": "pascal", ".inc": "pascal",
    ".c": "cpp", ".cc": "cpp", ".cpp": "cpp", ".cxx": "cpp", ".h": "cpp", ".hpp": "cpp", ".hxx": "cpp",
    ".cs": "csharp", ".json": "json", ".jsonc": "json", ".md": "markdown", ".markdown": "markdown",
    ".yaml": "yaml", ".yml": "yaml",
    ".toml": "toml", ".xml": "xml", ".xsd": "xml", ".xsl": "xml", ".xslt": "xml", ".ui": "xml", ".qrc": "xml",
    ".csproj": "xml", ".dproj": "xml", ".props": "xml", ".targets": "xml", ".vcxproj": "xml", ".filters": "xml",
    ".cmake": "cmake", ".sh": "bash", ".bash": "bash", ".qml": "qml", ".html": "html", ".htm": "html",
    ".css": "css", ".scss": "css",
}


def language_for(path: Path) -> str | None:
    if path.name == "CMakeLists.txt":
        return "cmake"
    if path.name == "Cargo.lock":
        return "toml"
    language = EXTENSIONS.get(path.suffix.lower())
    if path.suffix.lower() == ".ts" and path.is_file():
        # Qt Linguist translations share .ts with TypeScript.
        with path.open("rb") as source:
            raw = source.read(4096)
            prefix = raw.decode("utf-16" if raw.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8-sig", errors="replace").lstrip()
        if prefix.startswith("<?xml") or prefix.startswith("<!DOCTYPE TS") or prefix.startswith("<TS"):
            return "xml"
    return language


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def runtime_home() -> Path:
    return Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))) / "harness" / "runtime" / "lsp"


def registry_path() -> Path:
    return Path(os.environ.get("HARNESS_LSP_REGISTRY", str(runtime_home().parent.parent / "lsp-servers.json")))


def registry() -> dict:
    path = registry_path()
    if not path.exists():
        return {}
    result = json.loads(path.read_text(encoding="utf-8-sig"))
    return result.get("servers", result)


def installed_server(language: str, root: Path | None = None, *, discover_only: bool = False) -> dict:
    """Resolve host-owned overrides first, then verified existing standard paths."""
    key = "typescript" if language == "javascript" else language
    settings = None if discover_only else registry().get(key)
    if settings:
        if settings.get("status") == "unavailable":
            raise FileNotFoundError(settings.get("reason", f"Configured {key} backend is unavailable"))
        if key == "rust" and settings.get("project_toolchain"):
            selected = installed_server(key, root, discover_only=True)
            return {**settings, **selected,
                "initialization_options": {**selected.get("initialization_options", {}), **settings.get("initialization_options", {})},
                "env": {**settings.get("env", {}), **selected.get("env", {})}}
        command = settings.get("command")
        if not isinstance(command, list) or not command or not all(isinstance(x, str) for x in command):
            raise ValueError(f"Invalid command array for {key} in {registry_path()}")
        executable = shutil.which(command[0]) or (command[0] if Path(command[0]).is_file() else None)
        if not executable:
            raise FileNotFoundError(f"Configured {key} executable is missing: {command[0]}")
        return {**settings, "command": [executable, *command[1:]], "id": key}
    static = Path(os.environ.get("SERENA_HOME", str(Path.home() / ".serena"))) / "language_servers" / "static"
    if key == "typescript":
        script = static / "TypeScriptLanguageServer/ts-lsp/node_modules/typescript-language-server/lib/cli.mjs"
        node = shutil.which("node")
        if node and script.is_file():
            return {"id": key, "command": [node, str(script), "--stdio"], "adapter": "typescript",
                    "initialization_options": {"preferences": {"disableAutomaticTypingAcquisition": True}}}
    elif key == "rust":
        # Resolve existing toolchain executables directly; rustup proxies can install.
        import tomllib
        rustup_home = Path(os.environ.get("RUSTUP_HOME", str(Path.home() / ".rustup")))
        settings_file = rustup_home / "settings.toml"
        if settings_file.is_file():
            toolchain = tomllib.loads(settings_file.read_text()).get("default_toolchain", "")
            if root:
                for directory in (root, *root.parents):
                    project_toml = directory / "rust-toolchain.toml"
                    project_plain = directory / "rust-toolchain"
                    if project_toml.is_file():
                        toolchain = tomllib.loads(project_toml.read_text()).get("toolchain", {}).get("channel", toolchain)
                        break
                    if project_plain.is_file():
                        text = project_plain.read_text().strip()
                        toolchain = tomllib.loads(text).get("toolchain", {}).get("channel", toolchain) if text.startswith("[") else text
                        break
            if not isinstance(toolchain, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", toolchain):
                raise ValueError("Rust toolchain channel must be an installed toolchain name, not a project-supplied path")
            toolchains = (rustup_home / "toolchains").resolve()
            if not (toolchains / toolchain).is_dir():
                candidates = list(toolchains.glob(toolchain + "-x86_64-pc-windows-msvc"))
                if len(candidates) == 1:
                    toolchain = candidates[0].name
            binary = (toolchains / toolchain / "bin/rust-analyzer.exe").resolve()
            if not binary.is_relative_to(toolchains):
                raise ValueError("Rust toolchain executable leaves the verified installation root; custom linked toolchains require an explicit host-owned backend registration")
            if binary.is_file():
                return {"id": key, "command": [str(binary)], "adapter": "pull", "env": {"RUSTUP_TOOLCHAIN": toolchain},
                    "initialization_options": {"cargo": {"extraArgs": ["--locked", "--offline"]}}}
            raise FileNotFoundError(f"rust-analyzer for the project's installed toolchain {toolchain} is missing; runtime installation is disabled")
    elif key == "pascal":
        binary = static / "PascalLanguageServer/pasls.exe"
        if binary.is_file():
            return {"id": key, "command": [str(binary)], "adapter": "push",
                    "initialization_options": {"checkSyntax": True, "publishDiagnostics": True}}
    elif key == "cpp":
        candidates = sorted((Path.home() / ".cache/opencode/bin").glob("clangd_*/bin/clangd.exe"))
        binary = shutil.which("clangd") or (str(candidates[-1]) if candidates else None)
        if binary:
            return {"id": key, "command": [binary, "--background-index=false"], "adapter": "push"}
    elif key == "powershell":
        script = static / "PowerShellLanguageServer/powershell/PowerShellEditorServices/Start-EditorServices.ps1"
        pwsh = shutil.which("pwsh")
        if script.is_file() and pwsh:
            return {"id": key, "command": [pwsh, "-NoLogo", "-NoProfile", "-File", str(script),
                    "-HostName", "CodexHarness", "-HostProfileId", "CodexHarness", "-HostVersion", "1.0.0",
                    "-BundledModulesPath", str(script.parent), "-Stdio"], "adapter": "push"}
    names = {"python": ["basedpyright-langserver", "pyright-langserver"], "csharp": ["csharp-ls"],
             "json": ["vscode-json-languageserver", "vscode-json-language-server"],
             "markdown": ["marksman"], "yaml": ["yaml-language-server"], "toml": ["taplo"],
             "xml": ["lemminx"], "cmake": ["cmake-language-server"], "bash": ["bash-language-server"],
             "qml": ["qmlls"], "html": ["vscode-html-language-server"], "css": ["vscode-css-language-server"]}
    for name in names.get(key, []):
        executable = shutil.which(name)
        if executable:
            args = {"marksman": ["server"], "taplo": ["lsp", "stdio"], "bash-language-server": ["start"],
                    "cmake-language-server": [], "qmlls": []}.get(name, ["--stdio"])
            return {"id": key, "command": [executable, *args], "adapter": "push"}
    raise FileNotFoundError(f"No installed {key} language server. Provision through kit lifecycle; configure {registry_path()}")


class Backend:
    def __init__(self, root: Path, language: str, state_dir: Path, source_file: str | None = None):
        from solidlsp.ls_config import LanguageServerId
        from solidlsp.ls_process import StdioLanguageServer
        from solidlsp.lsp_protocol_handler.server import ProcessLaunchInfo

        self.root = root.resolve()
        self.language = language
        self.definition = installed_server(language, self.root)
        self.delphi_project = None
        self.delphi_diagnostic_files = set()
        project_root = self.root
        if language == "pascal" and self.definition.get("sdk_candidates"):
            from delphi import configure
            self.delphi_project = configure(self.root, self.definition, self.root / source_file if source_file else None)
            self.delphi_project["workspace"] = str(self.root)
            self.definition["initialization_options"] = self.delphi_project["initialization_options"]
            project_root = Path(self.delphi_project["root"])
        self.documents: dict[str, dict] = {}
        self.published: dict[str, dict] = {}
        self.condition = threading.Condition()
        self.lock = threading.RLock()
        self.generation = 0
        self.server_status = None
        self.project_ready = threading.Event()
        self.project_problem = None
        self.sync_ack_generation = 0
        self.sync_ack_message = ""
        self.sync_failed = False
        self.log_error_generation = 0
        self.last_log_error = ""
        if self.language == "css":
            self.definition.setdefault("settings", {"css": {"validate": True, "lint": {}}, "scss": {"validate": True, "lint": {}}})
        command = self.definition["command"]
        if self.language == "bash" and os.name == "nt" and Path(command[0]).stem.lower() == "node":
            command = [command[0], "--require", str(Path(__file__).with_name("bash-windows.cjs")), *command[1:]]
        if self.language == "powershell":
            state_dir.mkdir(parents=True, exist_ok=True)
            command = [*command, "-LogPath", str(state_dir / "pses.log"), "-SessionDetailsPath", str(state_dir / "session.json")]
        elif self.language == "csharp":
            state_dir.mkdir(parents=True, exist_ok=True)
            command = [argument for argument in command if not argument.startswith("--extensionLogDirectory=")]
            command.append("--extensionLogDirectory=" + str(state_dir / "logs"))
        self.state_dir = state_dir
        if self.language == "cmake":
            # cmake --system-information creates a directory in process cwd.
            # Keep those runtime probes outside project sources; rootUri below
            # still identifies the actual CMake project for navigation.
            state_dir.mkdir(parents=True, exist_ok=True)
            process_root = state_dir
        else:
            process_root = project_root
        # The enum only labels the reused transport; XML/CMake are outside Serena's own catalogue.
        enum_id = {"css": "scss", "xml": "json", "cmake": "cpp"}.get(self.definition["id"], self.definition["id"])
        self.connection = StdioLanguageServer(ProcessLaunchInfo(cmd=command, cwd=str(process_root), env=self.definition.get("env", {})),
            LanguageServerId(enum_id), lambda _: logging.DEBUG, request_timeout=15.0)
        for method in ("window/logMessage", "window/showMessage", "$/typescriptVersion", "$/progress", "textDocument/publishDiagnostics",
                       "experimental/serverStatus", "powerShell/executionStatusChanged"):
            self.connection.on_notification(method, lambda _: None)
        self.connection.on_any_notification(self._notification)
        self.connection.on_request("client/registerCapability", self._register)
        self.connection.on_request("window/workDoneProgress/create", lambda _: None)
        self.connection.on_request("workspace/configuration", self._configuration)
        self.connection.on_request("workspace/workspaceFolders", lambda _: [{"uri": root.as_uri(), "name": root.name}])
        self.connection.on_request("workspace/applyEdit", lambda _: {"applied": False, "failureReason": "Diagnostics never edit sources"})
        self.connection.on_request("workspace/_roslyn_projectNeedsRestore", self._needs_restore)
        self.connection.on_notification("workspace/projectInitializationComplete", lambda _: None)
        self.dynamic_capabilities: set[str] = set()
        self.markdown_client = None
        if self.definition.get("adapter") == "markdown-vscode":
            from markdown_client import MarkdownClient, default_settings
            self.markdown_client = MarkdownClient(self)
            self.definition.setdefault("settings", default_settings())
        try:
            self.connection.start()
            response = self.connection.send_request("initialize", {
                "processId": os.getpid(), "clientInfo": {"name": "codex-harness", "version": "1"},
                "rootUri": project_root.as_uri(), "workspaceFolders": [{"uri": project_root.as_uri(), "name": project_root.name}],
                "capabilities": {"workspace": {"configuration": True, "workspaceFolders": True},
                    "experimental": {"serverStatusNotification": True},
                    "textDocument": {"publishDiagnostics": {"versionSupport": True},
                        "synchronization": {"didSave": True}, "diagnostic": {},
                        "documentSymbol": {"hierarchicalDocumentSymbolSupport": True}}},
                "initializationOptions": self._initialization_options(),
            })
            self.capabilities = response.get("capabilities", {})
            self.connection.send_notification("initialized", {})
            if self.definition.get("settings"):
                self.connection.send_notification("workspace/didChangeConfiguration", {"settings": self.definition["settings"]})
            if self.language == "csharp":
                from journal import snapshot
                inputs, problems = snapshot(self.root)
                projects = [name for name in inputs if Path(name).suffix.lower() == ".csproj"]
                if problems or not projects:
                    self.project_problem = "C# project inputs are unavailable: " + "; ".join(problems or ["No .csproj found in the applicable root"])
                else:
                    self.connection.send_notification("project/open", {"projects": [(self.root / name).as_uri() for name in projects]})
        except Exception:
            self.close()
            raise

    def _register(self, params):
        self.dynamic_capabilities.update(item.get("method", "") for item in params.get("registrations", []))
        return None

    def _needs_restore(self, params):
        self.project_problem = "C# project requires dependency restore; automatic diagnostics never restores packages"
        return None

    def _initialization_options(self):
        options = dict(self.definition.get("initialization_options", {}))
        if self.definition.get("adapter") == "typescript":
            project_tsserver = self.root / "node_modules/typescript/lib/tsserver.js"
            if project_tsserver.is_file():
                options["tsserver"] = {**options.get("tsserver", {}), "path": str(project_tsserver)}
        return options

    def _configuration(self, params):
        result = []
        for item in params.get("items", []):
            selected = self.definition.get("settings", {})
            for part in item.get("section", "").split("."):
                if part:
                    selected = selected.get(part) if isinstance(selected, dict) else None
            result.append(selected)
        return result

    @staticmethod
    def uri_key(uri: str) -> str:
        return unquote(uri).casefold() if os.name == "nt" else unquote(uri)

    def document_uri(self, path: Path) -> str:
        uri = path.as_uri()
        if self.markdown_client and os.name == "nt":
            # VS Code's Markdown workspace converts URI.parse(...).toString()
            # before querying TextDocuments. On Windows that lowercases and
            # escapes the drive; a plain Path.as_uri() otherwise looks unopened
            # and causes an authoritative-looking empty diagnostic response.
            uri = re.sub(r"^file:///([A-Za-z]):", lambda match: "file:///" + match[1].lower() + "%3A", uri)
        return uri

    def _notification(self, method, params):
        if method in ("window/logMessage", "window/showMessage") and isinstance(params, dict) and params.get("type") == 1:
            with self.condition:
                self.log_error_generation += 1
                self.last_log_error = str(params.get("message", ""))
        if method == "window/logMessage" and isinstance(params, dict) and self.language == "cmake":
            message = str(params.get("message", ""))
            if message.startswith(("Opened file ", "update file: ")):
                with self.condition:
                    self.sync_ack_generation += 1
                    self.sync_ack_message = message
                    self.condition.notify_all()
            return
        if method == "workspace/projectInitializationComplete":
            self.project_ready.set()
            return
        if method == "experimental/serverStatus" and isinstance(params, dict):
            with self.condition:
                self.server_status = params
                self.condition.notify_all()
            return
        if method != "textDocument/publishDiagnostics" or not isinstance(params, dict):
            return
        if not isinstance(params.get("diagnostics"), list) or not isinstance(params.get("uri"), str):
            return
        with self.condition:
            previous = self.published.get(self.uri_key(params["uri"]))
            if previous and isinstance(previous.get("version"), int) and isinstance(params.get("version"), int):
                if params["version"] < previous["version"]:
                    return
            self.generation += 1
            self.published[self.uri_key(params["uri"])] = {**params, "received_generation": self.generation}
            self.condition.notify_all()

    def sync(self, relative: str, *, diagnostics: bool = False) -> tuple[Path, dict]:
        if self.sync_failed:
            raise RuntimeError("Backend synchronization was lost; restart this language client before claiming current diagnostics")
        path = (self.root / relative).resolve()
        if not path.is_relative_to(self.root):
            raise ValueError("Source path escapes workspace")
        content = path.read_bytes()
        revision = hashlib.sha256(content).hexdigest()
        if self.language == "pascal" and not self.definition.get("encoding"):
            try:
                text = content.decode("utf-16" if content.startswith((b"\xff\xfe", b"\xfe\xff")) else "utf-8-sig")
            except UnicodeDecodeError:
                text = content.decode("mbcs" if os.name == "nt" else "utf-8")
        else:
            text = content.decode(self.definition.get("encoding", "utf-8-sig"))
        previous = self.documents.get(relative)
        version = previous["version"] + 1 if previous else 1
        language_id = {"typescript": "typescriptreact" if path.suffix == ".tsx" else "typescript",
                       "javascript": "javascriptreact" if path.suffix == ".jsx" else "javascript",
                       "json": "jsonc" if path.suffix == ".jsonc" else "json",
                       "css": "scss" if path.suffix == ".scss" else "css",
                       "cpp": "c" if path.suffix == ".c" else "cpp"}.get(self.language, self.language)
        doc = {"uri": self.document_uri(path), "version": version, "revision": revision, "text": text}
        self.documents[relative] = doc
        ack_before = self.sync_ack_generation
        if previous:
            change = {"text": text}
            synchronization = self.capabilities.get("textDocumentSync", {})
            change_kind = synchronization.get("change") if isinstance(synchronization, dict) else synchronization
            if change_kind == 2:
                lines = re.split(r"\r\n|\r|\n", previous["text"])
                change["range"] = {"start": {"line": 0, "character": 0},
                    "end": {"line": len(lines) - 1, "character": len(lines[-1].encode("utf-16-le")) // 2}}
            parameters = {"textDocument": {"uri": doc["uri"], "version": version}, "contentChanges": [change]}
            if self.language == "cpp":
                # clangd's documented extension guarantees diagnostics for this
                # version, including an unchanged buffer's empty diagnostic set.
                parameters["wantDiagnostics"] = True
                if diagnostics and previous["revision"] == revision:
                    parameters["forceRebuild"] = True
            self.connection.send_notification("textDocument/didChange", parameters)
        else:
            self.connection.send_notification("textDocument/didOpen", {"textDocument": {"uri": doc["uri"], "languageId": language_id,
                "version": version, "text": text}})
        self.connection.send_notification("textDocument/didSave", {"textDocument": {"uri": doc["uri"]}})
        if self.language == "cmake":
            # neocmakelsp handles messages concurrently and returns an empty
            # pull for a not-yet-open buffer. Its observed acknowledgement is
            # emitted after update_cache for both open and change operations.
            deadline = time.monotonic() + 5
            with self.condition:
                while self.sync_ack_generation <= ack_before and time.monotonic() < deadline:
                    self.condition.wait(max(0, deadline - time.monotonic()))
                expected = doc["uri"] if previous else str(path)
                if self.sync_ack_generation <= ack_before or self.uri_key(expected) not in self.uri_key(self.sync_ack_message):
                    self.sync_failed = True
                    raise RuntimeError("CMake document synchronization was not acknowledged")
        return path, doc

    def sync_open_dependencies(self, relative: str, deadline: float):
        """Refresh this client's existing buffers before querying one source.

        An LSP can retain an old opened import even when its disk file changed
        in the same tool batch. Closed files are already read from disk by the
        server; only our own previously opened buffers need reconciliation.
        """
        for candidate, document in list(self.documents.items()):
            if candidate == relative:
                continue
            if time.monotonic() >= deadline:
                raise TimeoutError("Open dependency synchronization exceeded the diagnostic budget")
            path = self.root / candidate
            if not path.is_file():
                self.forget(candidate)
            elif digest(path) != document["revision"]:
                self.sync(candidate)

    def diagnostics(self, relative: str, timeout: float = 20.0) -> dict:
        with self.lock:
            deadline = time.monotonic() + timeout
            if self.language == "csharp":
                if self.project_problem or not self.project_ready.wait(max(0, deadline - time.monotonic())):
                    return {"file": relative, "revision": digest(self.root / relative), "backend": self.definition["id"],
                        "status": "unavailable" if self.project_problem else "pending", "diagnostics": [],
                        "reason": self.project_problem or "C# project loading has not completed"}
            if self.language == "rust":
                with self.condition:
                    while not self.server_status or not self.server_status.get("quiescent"):
                        remaining = deadline - time.monotonic()
                        if remaining <= 0:
                            return {"file": relative, "revision": digest(self.root / relative), "backend": self.definition["id"],
                                "status": "pending", "diagnostics": [], "reason": "Rust project loading has not completed"}
                        self.condition.wait(min(0.1, remaining))
                    if self.server_status.get("health") == "error":
                        return {"file": relative, "revision": digest(self.root / relative), "backend": self.definition["id"],
                            "status": "failed", "diagnostics": [], "reason": "Rust project load failed: " + str(self.server_status.get("message", "unknown error"))}
            try:
                self.sync_open_dependencies(relative, deadline)
            except TimeoutError as error:
                return {"file": relative, "revision": digest(self.root / relative), "backend": self.definition["id"],
                    "status": "pending", "diagnostics": [], "reason": str(error)}
            before = self.generation
            errors_before = self.log_error_generation
            path, doc = self.sync(relative, diagnostics=True)
            if self.markdown_client:
                self.markdown_client.reconcile()
            result = {"file": relative, "revision": doc["revision"], "backend": self.definition["id"], "status": "pending", "diagnostics": []}
            self.connection.set_request_timeout(max(0.1, deadline - time.monotonic()))
            if self.language == "powershell":
                result = self._powershell_diagnostics(doc["text"], result, max(0.1, deadline - time.monotonic()))
            elif self.language == "bash":
                result = self._bash_diagnostics(doc["text"], result, max(0.1, deadline - time.monotonic()))
            elif self.language == "toml":
                result = self._toml_diagnostics(path, result, max(0.1, deadline - time.monotonic()))
            elif self.language == "xml":
                result = self._xml_diagnostics(doc, result, max(0.1, deadline - time.monotonic()))
            elif self.delphi_project:
                from delphi import diagnostics
                result = diagnostics(self.delphi_project, path, self.state_dir, result, max(0.1, deadline - time.monotonic()))
                if result["status"] == "clean":
                    for previous_file in self.delphi_diagnostic_files - {relative}:
                        previous_path = self.root / previous_file
                        if previous_path.is_file():
                            result.setdefault("related_results", []).append({"file": previous_file, "revision": digest(previous_path),
                                "backend": "pascal", "status": "clean", "diagnostics": [], "freshness": "correlated-delphi-compiler"})
                    self.delphi_diagnostic_files.clear()
                elif result["status"] == "diagnostics":
                    self.delphi_diagnostic_files.update(item["file"] for item in [result, *result.get("related_results", [])] if item["diagnostics"])
            elif self.definition.get("adapter") == "pasls-0.2":
                # pasls v0.2's standard text loop dispatches synchronously.
                # didSave performs CheckSyntax and ALWAYS publishes its complete
                # diagnostic set (including Clear on success) before returning.
                # A subsequent correlated request is a completion barrier for
                # those unversioned notifications, unlike async push backends.
                # Verified upstream PasLS.TextLoop/PasLS.Synchronization/
                # PasLS.Diagnostics at v0.2.0; not enabled for unknown versions.
                self.connection.send_request("textDocument/documentSymbol", {"textDocument": {"uri": doc["uri"]}})
                with self.condition:
                    notification = self.published.get(self.uri_key(doc["uri"]))
                    if notification and notification["received_generation"] > before:
                        items = notification["diagnostics"]
                        result.update(status="diagnostics" if items else "clean", diagnostics=items, freshness="synchronous-pasls-save-barrier")
                    else:
                        result["reason"] = "Pascal save completed without the required diagnostic set"
            elif self.definition.get("adapter") == "typescript":
                found = []
                for kind in ("syntacticDiagnosticsSync", "semanticDiagnosticsSync", "suggestionDiagnosticsSync"):
                    self.connection.set_request_timeout(max(0.1, deadline - time.monotonic()))
                    response = self.connection.send_request("workspace/executeCommand", {"command": "typescript.tsserverRequest",
                        "arguments": [kind, {"file": doc["uri"], "includeLinePosition": True}, {"expectsResult": True}]})
                    if not isinstance(response, dict) or not response.get("success") or not isinstance(response.get("body"), list):
                        return {**result, "reason": f"Incomplete correlated {kind} response"}
                    for item in response["body"]:
                        start, end = item.get("startLocation"), item.get("endLocation")
                        if not isinstance(start, dict):
                            start = item.get("start", {"line": 1, "offset": 1})
                            end = item.get("end", start)
                        found.append({"range": {"start": {"line": start["line"] - 1, "character": start["offset"] - 1},
                            "end": {"line": end["line"] - 1, "character": end["offset"] - 1}},
                            "message": item.get("text", item.get("message", "")), "code": item.get("code"), "source": "typescript",
                            "severity": {"error": 1, "warning": 2, "suggestion": 4, "message": 3}.get(item.get("category"), 1)})
                result.update(status="diagnostics" if found else "clean", diagnostics=found, freshness="correlated-tsserver-response")
            elif "diagnosticProvider" in self.capabilities or "textDocument/diagnostic" in self.dynamic_capabilities:
                for attempt in range(3):
                    try:
                        self.connection.set_request_timeout(max(0.1, deadline - time.monotonic()))
                        response = self.connection.send_request("textDocument/diagnostic", {"textDocument": {"uri": doc["uri"]}})
                        break
                    except Exception as error:
                        code = getattr(getattr(error, "cause", None), "code", None)
                        if code not in (-32800, -32802) or attempt == 2 or deadline - time.monotonic() < 0.2:
                            raise
                        # Roslyn cancels a pull while applying a just-sent change.
                        # Retry that request without another document mutation.
                        time.sleep(0.1)
                if isinstance(response, dict) and response.get("kind") == "full" and isinstance(response.get("items"), list):
                    result.update(status="diagnostics" if response["items"] else "clean", diagnostics=response["items"], freshness="correlated-pull-response")
            else:
                with self.condition:
                    while time.monotonic() < deadline:
                        notification = self.published.get(self.uri_key(doc["uri"]))
                        if notification and notification["received_generation"] > before and notification.get("version") == doc["version"]:
                            items = notification["diagnostics"]
                            result.update(status="diagnostics" if items else "clean", diagnostics=items, freshness="versioned-push")
                            break
                        self.condition.wait(min(0.1, max(0, deadline - time.monotonic())))
                if result["status"] == "pending":
                    result["reason"] = "No authoritative diagnostics associated with current document version"
            if not path.is_file() or digest(path) != doc["revision"]:
                result["status"] = "stale"
                result["reason"] = "File changed while diagnostics were running"
            elif result["status"] == "clean" and self.log_error_generation > errors_before:
                # Some VS Code adapters catch validation exceptions and return
                # an empty pull response. Their accompanying error log means
                # that response cannot establish successful clearance.
                result.update(status="failed", reason="Backend reported an analysis failure: " + self.last_log_error[-2000:])
            return result

    def _powershell_diagnostics(self, text, result, timeout):
        static = Path(os.environ.get("SERENA_HOME", str(Path.home() / ".serena"))) / "language_servers/static/PowerShellLanguageServer/powershell/PowerShellEditorServices"
        modules = sorted(static.glob("PSScriptAnalyzer/*/PSScriptAnalyzer.psd1"))
        analyzer = self.definition.get("analyzer_path") or (str(modules[-1]) if modules else None)
        pwsh = self.definition["command"][0]
        if not analyzer or not Path(analyzer).is_file():
            return {**result, "status": "unavailable", "reason": "Installed PSScriptAnalyzer module is missing"}
        settings = self.root / "PSScriptAnalyzerSettings.psd1"
        payload = {"text": text, "analyzer": analyzer,
                   "settings": str(settings) if settings.is_file() else None}
        response = subprocess.run([pwsh, "-NoLogo", "-NoProfile", "-File", str(Path(__file__).with_name("powershell-diagnostics.ps1"))],
            input=json.dumps(payload), text=True, encoding="utf-8", capture_output=True, timeout=timeout,
            cwd=self.root,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0, check=False)
        if response.returncode:
            return {**result, "status": "failed", "reason": response.stderr[-2000:]}
        data = json.loads(response.stdout)
        if data.get("complete") is not True or not isinstance(data.get("diagnostics"), list):
            return {**result, "status": "pending", "reason": "Incomplete PSScriptAnalyzer response"}
        return {**result, "status": "diagnostics" if data["diagnostics"] else "clean", "diagnostics": data["diagnostics"],
                "freshness": "correlated-source-snapshot", "diagnostic_provider": "PSScriptAnalyzer"}

    def _bash_diagnostics(self, text, result, timeout):
        shellcheck = (self.definition.get("env", {}).get("SHELLCHECK_PATH")
            or self.definition.get("settings", {}).get("bashIde", {}).get("shellcheckPath") or shutil.which("shellcheck"))
        if not shellcheck or not Path(shellcheck).is_file():
            return {**result, "status": "unavailable", "reason": "Installed ShellCheck is missing; Bash LSP alone can publish empty diagnostics"}
        response = subprocess.run([shellcheck, "--format=json", "--shell=bash", "-"], input=text.encode("utf-8"),
            capture_output=True, cwd=self.root, timeout=timeout,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0, check=False)
        if response.returncode not in (0, 1):
            return {**result, "status": "failed", "reason": "ShellCheck failed: " + response.stderr.decode("utf-8", errors="replace")[-2000:]}
        items = json.loads(response.stdout)
        if not isinstance(items, list):
            return {**result, "status": "pending", "reason": "Incomplete ShellCheck response"}
        diagnostics = [{"range": {"start": {"line": item["line"] - 1, "character": item["column"] - 1},
            "end": {"line": item.get("endLine", item["line"]) - 1, "character": item.get("endColumn", item["column"]) - 1}},
            "severity": {"error": 1, "warning": 2, "info": 3, "style": 4}.get(item.get("level"), 1),
            "message": item["message"], "code": "SC" + str(item["code"]), "source": "ShellCheck"} for item in items]
        return {**result, "status": "diagnostics" if diagnostics else "clean", "diagnostics": diagnostics,
            "freshness": "correlated-source-snapshot", "diagnostic_provider": "ShellCheck"}

    def _toml_diagnostics(self, path, result, timeout):
        # Taplo's CLI validates the same file identity as its LSP, preserving
        # project rule/schema associations. It never formats or writes sources.
        binary = self.definition["command"][0]
        response = subprocess.run([binary, "lint", "--colors", "never", "--cache-path", str(self.state_dir / "taplo-cache"), str(path)],
            capture_output=True, cwd=self.root, timeout=timeout,
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0, check=False)
        stderr = response.stderr.decode("utf-8", errors="replace")
        if response.returncode == 0:
            return {**result, "status": "clean", "diagnostics": [], "freshness": "correlated-file-check", "diagnostic_provider": "Taplo lint"}
        diagnostics = []
        for block in re.split(r"(?m)(?=^error:)", stderr):
            position = re.search(r":(\d+):(\d+)\s*\n", block)
            if block.startswith("error:") and position:
                line, column = (int(value) - 1 for value in position.groups())
                diagnostics.append({"range": {"start": {"line": line, "character": column}, "end": {"line": line, "character": column + 1}},
                    "severity": 1, "source": "Taplo", "message": block.strip()})
        if not diagnostics:
            return {**result, "status": "failed", "reason": "Taplo validation did not complete: " + stderr[-2000:]}
        return {**result, "status": "diagnostics", "diagnostics": diagnostics, "freshness": "correlated-file-check", "diagnostic_provider": "Taplo lint"}

    def _xml_diagnostics(self, doc, result, timeout):
        pwsh = shutil.which("pwsh")
        if not pwsh:
            return {**result, "status": "unavailable", "reason": "Installed PowerShell/.NET XML validator is unavailable"}
        response = subprocess.run([pwsh, "-NoLogo", "-NoProfile", "-File", str(Path(__file__).with_name("xml-diagnostics.ps1"))],
            input=json.dumps({"text": doc["text"], "workspace": str(self.root), "uri": doc["uri"], "schemas": self.definition.get("schema_files", [])}).encode("utf-8"),
            capture_output=True, cwd=self.root, timeout=timeout, creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0, check=False)
        if response.returncode:
            return {**result, "status": "failed", "reason": response.stderr.decode("utf-8", errors="replace")[-2000:]}
        data = json.loads(response.stdout)
        if data.get("complete") is not True:
            return {**result, "status": "unavailable", "reason": data.get("reason", "Incomplete XML validation")}
        return {**result, "status": "diagnostics" if data["diagnostics"] else "clean", "diagnostics": data["diagnostics"],
            "freshness": "correlated-source-snapshot", "diagnostic_provider": "System.Xml"}

    def navigation(self, relative: str, operation: str, line: int = 0, character: int = 0,
                   query: str = "", new_name: str = "", item: dict | None = None):
        methods = {"definition": "textDocument/definition", "references": "textDocument/references",
                   "hover": "textDocument/hover", "symbols": "textDocument/documentSymbol", "implementation": "textDocument/implementation",
                   "prepare_call_hierarchy": "textDocument/prepareCallHierarchy", "type_definition": "textDocument/typeDefinition",
                   "declaration": "textDocument/declaration", "workspace_symbols": "workspace/symbol",
                   "incoming_calls": "callHierarchy/incomingCalls", "outgoing_calls": "callHierarchy/outgoingCalls",
                   "rename_preview": "textDocument/rename"}
        if operation == "capabilities":
            return {"static": self.capabilities, "dynamic": sorted(self.dynamic_capabilities)}
        if operation not in methods:
            raise ValueError("Unsupported read operation")
        with self.lock:
            self.sync_open_dependencies(relative, time.monotonic() + 15)
            _, doc = self.sync(relative)
            args = {"textDocument": {"uri": doc["uri"]}}
            if operation == "workspace_symbols":
                args = {"query": query}
            elif operation in ("incoming_calls", "outgoing_calls"):
                if not isinstance(item, dict) or self.uri_key(item.get("uri", "")) != self.uri_key(doc["uri"]):
                    raise ValueError("Call hierarchy requires a returned item for the requested source")
                args = {"item": item}
            elif operation != "symbols":
                args["position"] = {"line": line, "character": character}
            if operation == "references":
                args["context"] = {"includeDeclaration": True}
            elif operation == "rename_preview":
                if not new_name:
                    raise ValueError("Rename preview requires new_name")
                args["newName"] = new_name
            self.connection.set_request_timeout(15.0)
            return self.connection.send_request(methods[operation], args)

    def forget(self, relative: str):
        """Release deleted buffers before checking surviving dependent sources."""
        with self.lock:
            document = self.documents.pop(relative, None)
            uri = document["uri"] if document else self.document_uri(self.root / relative)
            if document:
                self.connection.send_notification("textDocument/didClose", {"textDocument": {"uri": uri}})
            self.published.pop(self.uri_key(uri), None)
            self.connection.send_notification("workspace/didChangeWatchedFiles", {"changes": [{"uri": uri, "type": 3}]})

    def dependent_files(self, relative: str) -> list[str]:
        """Ask the TypeScript project service for its actual project file set."""
        if self.definition.get("adapter") != "typescript":
            return []
        with self.lock:
            response = self.connection.send_request("workspace/executeCommand", {"command": "typescript.tsserverRequest",
                "arguments": ["projectInfo", {"file": self.documents[relative]["uri"], "needFileNameList": True}, {"expectsResult": True}]})
            if not isinstance(response, dict) or not response.get("success"):
                raise RuntimeError("TypeScript project file discovery did not complete")
            result = []
            for filename in response.get("body", {}).get("fileNames", []):
                path = Path(filename).resolve()
                if path.is_relative_to(self.root) and path.is_file() and "node_modules" not in path.parts:
                    candidate = path.relative_to(self.root).as_posix()
                    if candidate != relative and language_for(path) in ("typescript", "javascript"):
                        result.append(candidate)
            return result

    def close(self):
        with self.lock:
            self._close()

    def _close(self):
        process = getattr(self.connection, "_process", None)
        if self.language == "cmake" and process and process.poll() is None:
            # neocmakelsp 0.11 writes .cache/neocmakelsp under project_root on
            # LSP shutdown and offers no cache-dir setting. Terminate only this
            # adapter-owned process, avoiding unsolicited project mutations.
            self.connection._is_stopping = True
            process.terminate(timeout=2.0)
        self.connection.stop(timeout=2.0)
        if process:
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream and not stream.closed:
                    stream.close()
