"""Read-only dependency discovery. No package import, installation or network requests.

An adopted record proves package identity and files, not MCP/LSP operation. In particular,
unknown integrity blocks automatic update even when an existing installation can be reused.
All nonstandard installation roots can be supplied explicitly; another --user-home never
inherits this process's PATH or installation environment.
"""
from __future__ import annotations

import argparse
import base64
import csv
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from email.parser import Parser
from pathlib import Path


CATALOGUE = Path(__file__).resolve().parents[2] / "global" / "code-tools.json"


def canonical(path):
    return Path(path).expanduser().resolve()


def read_json(path):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8-sig"))
    except (OSError, ValueError):
        return None


def unique_paths(paths):
    return list(dict.fromkeys(canonical(p) for p in paths))


def contained(path, root):
    try:
        canonical(path).relative_to(canonical(root))
        return True
    except ValueError:
        return False


def identity(name):
    return re.sub(r"[-_.]+", "-", name).lower()


def fingerprint(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def base_record(spec):
    return {
        "id": spec["id"], "identity": spec["package"], "manager": spec["manager"],
        "version": None, "executable": None, "command": [], "installation_root": None,
        "status": "missing", "ownership": "not-found", "provenance": {"source": spec["source"]},
        "health": {"installed": False, "identity_verified": False, "integrity": "unknown",
                   "callable": None, "checked_operations": []},
        "update_safe": False, "active_consumers": {"state": "not-checked", "processes": []},
        "evidence": [], "candidates": [], "verification": {"state": "unverified", "evidence": []},
    }


def verify_record(dist_info, install_root, full=False, package_dirs=()):
    """Verify wheel RECORD without following any entry outside the selected installation.

    The default includes entry points, package Python sources and METADATA. Full mode also
    checks native/data files. RECORD is local installation evidence, not an upstream signature.
    """
    record = dist_info / "RECORD"
    result = {"state": "unknown", "checked_files": 0, "issues": [], "basis": str(record)}
    if not record.is_file():
        return result
    try:
        with record.open(encoding="utf-8", newline="") as stream:
            rows = list(csv.reader(stream))
        for row in rows:
            if len(row) < 3:
                result["issues"].append({"reason": "malformed-record"})
                continue
            name, encoded, _size = row
            target = canonical(dist_info.parent / name)
            if not contained(target, install_root):
                result["issues"].append({"reason": "record-path-outside-installation", "path": name})
                continue
            in_package = any(name.startswith(prefix + "/") for prefix in package_dirs)
            selected = full or target.suffix == ".py" and in_package or name.endswith(("/METADATA", "/entry_points.txt"))
            if not selected or not encoded:
                continue
            if not target.is_file():
                result["issues"].append({"reason": "missing-recorded-file", "path": name})
                continue
            algorithm, value = encoded.split("=", 1)
            if algorithm != "sha256":
                result["issues"].append({"reason": "unsupported-record-hash", "path": name})
                continue
            actual = base64.urlsafe_b64encode(bytes.fromhex(fingerprint(target))).rstrip(b"=").decode("ascii")
            result["checked_files"] += 1
            if actual != value:
                result["issues"].append({"reason": "record-hash-mismatch", "path": name})
        result["state"] = "modified" if result["issues"] else "record-matches" if result["checked_files"] else "unknown"
    except (OSError, ValueError, csv.Error) as error:
        result["issues"].append({"reason": type(error).__name__})
    return result


def uv_candidate(spec, tool_root, full):
    env_root = canonical(tool_root / spec["package"])
    sites = [env_root / "Lib" / "site-packages", *env_root.glob("lib/python*/site-packages")]
    result = None
    for site in sites:
        if not site.is_dir():
            continue
        for dist in site.glob("*.dist-info"):
            metadata_file = dist / "METADATA"
            try:
                metadata = Parser().parsestr(metadata_file.read_text(encoding="utf-8"))
            except OSError:
                continue
            if identity(metadata.get("Name", "")) != identity(spec["package"]):
                continue
            python = env_root / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
            if not python.is_file():
                alternatives = [env_root / "Scripts/python.exe", env_root / "bin/python"]
                python = next((p for p in alternatives if p.is_file()), python)
            modules = ("serena", "solidlsp") if spec["id"] == "serena" else ("graphify",)
            integrity = verify_record(dist, env_root, full, modules)
            module_entry = site / ("serena/cli.py" if spec["id"] == "serena" else "graphify/serve.py")
            receipt = env_root / "uv-receipt.toml"
            receipt_identity = False
            if receipt.is_file():
                try:
                    data = tomllib.loads(receipt.read_text(encoding="utf-8"))
                    requirements = data.get("tool", {}).get("requirements", [])
                    receipt_identity = any(identity(r.get("name", "")) == identity(spec["package"]) for r in requirements)
                except (OSError, ValueError):
                    pass
            installed = python.is_file() and module_entry.is_file()
            status = "adopted" if installed else "broken"
            if integrity["state"] == "modified":
                status = "modified"
            command = [str(python), "-u", "-c", "from serena.cli import top_level; top_level()"] if spec["id"] == "serena" else [str(python), "-u", "-m", "graphify.serve"]
            result = {
                "manager": "uv", "version": metadata.get("Version"), "executable": str(python),
                "command": command, "paths": {"python": str(python), "module_root": str(site), "entrypoint": str(module_entry)},
                "installation_root": str(env_root), "status": status,
                "ownership": "adopted-shared" if receipt_identity else "unconfirmed",
                "health": {"installed": installed, "identity_verified": True,
                           "integrity": integrity["state"], "callable": None, "checked_operations": []},
                "update_safe": receipt_identity and integrity["state"] == "record-matches" and installed,
                "provenance": {"source": spec["source"], "metadata": str(metadata_file), "receipt": str(receipt),
                               "receipt_identity_verified": receipt_identity, "module": str(module_entry)},
                "evidence": [{"kind": "wheel-record", **integrity}],
            }
    return result


def npm_candidate(package, node_modules, node, command_name=None, owner="npm-global"):
    root = canonical(node_modules / package)
    manifest = root / "package.json"
    data = read_json(manifest)
    if not isinstance(data, dict) or data.get("name") != package:
        return None
    bins = data.get("bin", {})
    if isinstance(bins, str):
        bins = {package.rsplit("/", 1)[-1]: bins}
    entry = bins.get(command_name) if command_name else next(iter(bins.values()), None)
    script = canonical(root / entry) if isinstance(entry, str) else None
    safe_entry = script is not None and contained(script, root)
    installed = bool(safe_entry and script.is_file() and node and Path(node).is_file())
    evidence = [{"kind": "npm-package-identity", "path": str(manifest), "sha256": fingerprint(manifest)}]
    if safe_entry and script.is_file():
        evidence.append({"kind": "entrypoint-fingerprint", "path": str(script), "sha256": fingerprint(script)})
    result = {
        "manager": "npm" if owner == "npm-global" else owner, "version": data.get("version"),
        "executable": str(node) if node else None, "command": [str(node), str(script)] if installed else [],
        "paths": {"node": str(node) if node else None, "module_root": str(root), "entrypoint": str(script) if script else None},
        "installation_root": str(root), "status": "adopted" if installed else "broken",
        "ownership": "adopted-shared", "update_safe": False,
        "health": {"installed": installed, "identity_verified": True, "integrity": "unknown",
                   "callable": None, "checked_operations": []},
        "provenance": {"metadata": str(manifest), "package_identity": package, "entrypoint": str(script) if script else None},
        "evidence": evidence + [{"kind": "limitation", "detail": "No trusted per-file npm checksum inventory; compare official tarball before replacement."}],
    }
    if package == "codebase-memory-mcp":
        native = root / "bin" / ("codebase-memory-mcp.exe" if os.name == "nt" else "codebase-memory-mcp")
        result["paths"]["native_executable"] = str(native)
        # Upstream's JS shim downloads when this binary is missing/broken. Bypass it.
        result["command"] = [str(native)] if native.is_file() else []
        result["executable"] = str(native)
        result["status"] = "adopted" if native.is_file() else "broken"
        result["health"]["installed"] = native.is_file()
        if native.is_file():
            result["evidence"].append({"kind": "native-payload-fingerprint", "path": str(native), "sha256": fingerprint(native)})
        else:
            result["evidence"].append({"kind": "missing-native-payload", "path": str(native)})
    elif package == "@nuphus/nuphus-mcp":
        suffix = "win32" if os.name == "nt" else "osx" if sys.platform == "darwin" else "linux"
        arch = "arm64" if platform.machine().lower() in ("arm64", "aarch64") else "x64"
        companion_name = "@nuphus/nuphus-mcp-" + suffix + "-" + arch
        companion_roots = [root / "node_modules" / companion_name, node_modules / companion_name]
        chosen = None
        for companion in companion_roots:
            companion_data = read_json(companion / "package.json")
            if isinstance(companion_data, dict) and companion_data.get("name") == companion_name and companion_data.get("version") == data.get("version"):
                binary = companion / "bin" / ("nuphus-mcp.exe" if os.name == "nt" else "nuphus-mcp")
                patched = companion / "bin/nuphus-mcp-schema-fixed.exe"
                if binary.is_file():
                    chosen = patched if os.name == "nt" and patched.is_file() else binary
                    result["paths"]["native_executable"] = str(chosen)
                    result["paths"]["original_native_executable"] = str(binary)
                    result["provenance"]["platform_package"] = companion_name
                    result["provenance"]["platform_version"] = companion_data["version"]
                    result["evidence"].append({"kind": "native-payload-fingerprint", "path": str(chosen), "sha256": fingerprint(chosen)})
                    if chosen == patched:
                        result["status"] = "modified"
                        result["health"]["integrity"] = "modified"
                        result["evidence"].append({"kind": "local-variant", "detail": "Schema-fixed executable selected by the installed wrapper; preserve and audit before replacement."})
                    break
        if chosen is None:
            result["status"] = "broken"
            result["health"]["installed"] = False
            result["evidence"].append({"kind": "missing-platform-payload", "identity": companion_name})
    return result


def native_candidate(path, manager, version=None, version_basis=None, command=None, probe=False):
    path = canonical(path)
    if not path.is_file():
        return None
    evidence = [{"kind": "executable-fingerprint", "path": str(path), "sha256": fingerprint(path)}]
    installed = read_json(path.parent / ".harness-provisioning.json")
    if isinstance(installed, dict) and installed.get("owner") == "codex-harness-dependencies" and installed.get("executable_sha256") == evidence[0]["sha256"]:
        version = version or installed.get("version")
        evidence.append({"kind": "verified-provisioning-record", "source": installed.get("source"),
                         "path": str(path.parent / ".harness-provisioning.json")})
    if probe:
        try:
            completed = subprocess.run([str(path), "--version"], capture_output=True, text=True, timeout=8,
                                       creationflags=0x08000000 if os.name == "nt" else 0)
            output = (completed.stdout + completed.stderr).strip()
            if completed.returncode == 0 and output:
                version = output.splitlines()[0][:200]
                evidence.append({"kind": "version-command", "exit_code": 0, "value": version})
        except (OSError, subprocess.SubprocessError):
            evidence.append({"kind": "version-command", "result": "failed"})
    if version_basis:
        evidence.append({"kind": "version-metadata", "path": str(version_basis), "value": version})
    return {"manager": manager, "version": version, "executable": str(path), "command": command if command is not None else [str(path)],
            "paths": {"executable": str(path)},
            "installation_root": str(path.parent), "status": "adopted", "ownership": "adopted-shared",
            "health": {"installed": True, "identity_verified": bool(version), "integrity": "unknown",
                       "callable": None, "checked_operations": []}, "update_safe": False,
            "provenance": {"resolved_path": str(path)}, "evidence": evidence}


def select(record, candidates):
    candidates = [c for c in candidates if c]
    # Resolve aliases/symlinks before deciding whether there is more than one installation.
    seen = set()
    distinct = []
    for candidate in candidates:
        key = (candidate["installation_root"], tuple(candidate.get("command", [])))
        if key not in seen:
            seen.add(key)
            distinct.append(candidate)
    candidates = distinct
    record["candidates"] = candidates
    usable = [c for c in candidates if c["status"] == "adopted"]
    chosen = usable[0] if len(usable) == 1 else candidates[0] if len(candidates) == 1 else None
    if chosen:
        source = record["provenance"]["source"]
        record.update(chosen)
        record["provenance"].setdefault("source", source)
    elif candidates:
        record["status"] = "ambiguous"
        record["ownership"] = "selection-required"
        record["evidence"].append({"kind": "selection-conflict", "detail": "Multiple distinct installations; explicit selection required."})
    return record


def attach_consumers(records, processes):
    """Match installation paths, then discard process arguments rather than logging them."""
    for record in records:
        root = record.get("installation_root")
        matches = []
        if root:
            prefix = os.path.normcase(root).replace("/", "\\").rstrip("\\")
            for process in processes:
                executable = process.get("ExecutablePath") or ""
                commandline = process.get("CommandLine") or ""
                exe_normal = os.path.normcase(executable).replace("/", "\\")
                args_normal = os.path.normcase(commandline).replace("/", "\\")
                # Require a path boundary to avoid matching sibling versions by prefix.
                in_root = exe_normal.startswith(prefix + "\\")
                argument_match = bool(re.search(r'(?:^|[\s"=])' + re.escape(prefix) + r'(?=$|[\s"\\])', args_normal))
                if in_root or argument_match:
                    matches.append({"pid": process.get("ProcessId"), "executable": executable,
                                    "evidence": "installation-path-match"})
        record["active_consumers"] = {"state": "observed", "processes": matches}


def delphi_sdks():
    if os.name != "nt":
        return []
    import winreg
    found = []
    for owner in ("Embarcadero", "CodeGear", "Borland"):
        key_name = "SOFTWARE\\" + owner + "\\BDS"
        for view in (winreg.KEY_WOW64_32KEY, winreg.KEY_WOW64_64KEY):
            try:
                with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, key_name, 0, winreg.KEY_READ | view) as key:
                    for index in range(winreg.QueryInfoKey(key)[0]):
                        version = winreg.EnumKey(key, index)
                        try:
                            with winreg.OpenKey(key, version) as version_key:
                                path = winreg.QueryValueEx(version_key, "RootDir")[0]
                            compiler = Path(path) / "bin/dcc32.exe"
                            if compiler.is_file():
                                result = {"product": owner + " BDS " + version, "root": str(canonical(path)),
                                          "compiler": str(canonical(compiler)), "provenance": "HKLM/" + key_name + "/" + version + ":RootDir"}
                                if result not in found:
                                    found.append(result)
                        except OSError:
                            pass
            except OSError:
                pass
    return found


class Discovery:
    def __init__(self, user_home, catalogue=CATALOGUE, npm_prefixes=(), uv_tools_dir=None,
                 serena_cache=None, rustup_home=None, include_process_environment=None,
                 verify_records=False, probe_versions=False, graphify_manifest=None, processes=False):
        self.home = canonical(user_home)
        self.environment = self.home == canonical(Path.home()) if include_process_environment is None else include_process_environment
        self.catalogue_path = canonical(catalogue)
        self.catalogue = read_json(self.catalogue_path)
        if not self.catalogue or self.catalogue.get("schema_version") != 1:
            raise ValueError("Unsupported or invalid code-tools catalogue")
        self.verify_records = verify_records
        self.probe_versions = probe_versions
        self.processes = processes
        self.path_dirs = unique_paths(p for p in os.environ.get("PATH", "").split(os.pathsep) if p) if self.environment else []
        self.uv = canonical(uv_tools_dir or (os.environ.get("UV_TOOL_DIR") if self.environment else None) or self.home / "AppData/Roaming/uv/tools")
        self.serena = canonical(serena_cache or self.home / ".serena/language_servers/static")
        self.rustup = canonical(rustup_home or (os.environ.get("RUSTUP_HOME") if self.environment else None) or self.home / ".rustup")
        prefixes = [*map(Path, npm_prefixes), self.home / "AppData/Roaming/npm", *self.path_dirs]
        if self.environment and os.environ.get("NPM_CONFIG_PREFIX"):
            prefixes.insert(0, Path(os.environ["NPM_CONFIG_PREFIX"]))
        self.npm_roots = unique_paths(p / "node_modules" for p in prefixes if (p / "node_modules").is_dir())
        self.node = self.find_command("node")
        self.pwsh = self.find_command("pwsh")
        self.graphify_manifest = canonical(graphify_manifest) if graphify_manifest else canonical(Path(os.environ.get("PROGRAMDATA", "C:/ProgramData")) / "OpenCodeWorkstation/manifest.json") if self.environment else None

    def find_command(self, name):
        for parent in self.path_dirs:
            for suffix in ([".exe", ""] if os.name == "nt" else [""]):
                candidate = parent / (name + suffix)
                if candidate.is_file():
                    return str(canonical(candidate))
        return None

    def npm(self, package, command=None, cache_class=None):
        candidates = [npm_candidate(package, root, self.node, command) for root in self.npm_roots]
        if cache_class:
            cache = self.serena / cache_class
            if cache.is_dir():
                # Installed resources are small and bounded to a single backend's cache.
                for root in cache.glob("*/node_modules"):
                    candidates.append(npm_candidate(package, root, self.node, command, "serena-cache"))
                candidates.append(npm_candidate(package, cache / "node_modules", self.node, command, "serena-cache"))
        return [c for c in candidates if c]

    def language(self, spec):
        lang = spec["id"]
        candidates = []
        npm_backends = {
            "typescript": ("typescript-language-server", "typescript-language-server", "TypeScriptLanguageServer"),
            "javascript": ("typescript-language-server", "typescript-language-server", "TypeScriptLanguageServer"),
            "python": ("basedpyright", "basedpyright-langserver", "BasedPyrightLanguageServer"),
            "json": ("vscode-langservers-extracted", "vscode-json-language-server", "JsonLanguageServer"),
            "yaml": ("yaml-language-server", "yaml-language-server", "YamlLanguageServer"),
            "bash": ("bash-language-server", "bash-language-server", "BashLanguageServer"),
            "html": ("vscode-langservers-extracted", "vscode-html-language-server", "VsCodeHtmlLanguageServer"),
            "css": ("vscode-langservers-extracted", "vscode-css-language-server", "VscodeCssLanguageServer"),
        }
        if lang in npm_backends:
            candidates.extend(self.npm(*npm_backends[lang]))
            if lang == "python":
                candidates.extend(self.npm("pyright", "pyright-langserver", "PyrightServer"))
            elif lang == "json":
                candidates.extend(self.npm("vscode-json-languageserver", "vscode-json-languageserver", "JsonLanguageServer"))
            elif lang in ("html", "css"):
                candidates.extend(self.npm("vscode-langservers-extracted", "vscode-" + lang + "-language-server", "JsonLanguageServer"))
        elif lang == "powershell":
            cache = self.serena / "PowerShellLanguageServer"
            for manifest in cache.glob("*/PowerShellEditorServices/PowerShellEditorServices.psd1"):
                content = manifest.read_text(encoding="utf-8-sig")
                match = re.search(r"ModuleVersion\s*=\s*['\"]([^'\"]+)", content)
                entry = manifest.parent / "Start-EditorServices.ps1"
                version = match.group(1) if match else None
                found = native_candidate(entry, "serena-cache", version, manifest,
                                         [self.pwsh, "-NoLogo", "-NoProfile", "-File", str(entry)] if self.pwsh else [])
                if found:
                    if not self.pwsh:
                        found["status"] = "broken"
                        found["health"]["installed"] = False
                        found["evidence"].append({"kind": "missing-runtime", "name": "pwsh"})
                    found["analyzers"] = []
                    analyzers = unique_paths([*manifest.parent.glob("PSScriptAnalyzer/*/PSScriptAnalyzer.psd1"), *manifest.parent.parent.glob("PSScriptAnalyzer/*/PSScriptAnalyzer.psd1")])
                    for analyzer in analyzers:
                        found["analyzers"].append({"version": analyzer.parent.name, "path": str(analyzer)})
                    if len(analyzers) == 1:
                        found["paths"]["analyzer"] = str(analyzers[0])
                        found["paths"]["bundled_modules"] = str(analyzers[0].parents[2])
                    candidates.append(found)
        elif lang == "delphi":
            cache = self.serena / "PascalLanguageServer"
            metadata = cache / ".meta/version"
            version = metadata.read_text().strip() if metadata.is_file() else None
            candidates.append(native_candidate(cache / "pasls.exe", "serena-cache", version, metadata))
            for entry in cache.glob("pasls-*/**/pasls.exe"):
                candidates.append(native_candidate(entry, "serena-cache"))
        elif lang == "rust":
            settings = self.rustup / "settings.toml"
            toolchain = None
            if settings.is_file():
                try:
                    toolchain = tomllib.loads(settings.read_text()).get("default_toolchain")
                except (OSError, ValueError):
                    pass
            if toolchain and re.fullmatch(r"[A-Za-z0-9_.-]+", toolchain):
                path = self.rustup / "toolchains" / toolchain / "bin" / ("rust-analyzer.exe" if os.name == "nt" else "rust-analyzer")
                candidates.append(native_candidate(path, "rustup", probe=self.probe_versions))
            if not any(candidates):
                direct = self.find_command("rust-analyzer")
                # Do not execute rustup proxies: querying a missing toolchain can install it.
                if direct and ".cargo" not in Path(direct).parts:
                    candidates.append(native_candidate(direct, "path", probe=self.probe_versions))
        elif lang == "cpp":
            cache = self.home / ".cache/opencode/bin"
            for entry in cache.glob("clangd*/bin/clangd.exe"):
                match = re.search(r"clangd[_-](\d+(?:\.\d+)+)", str(entry))
                candidate = native_candidate(entry, "opencode-cache", probe=self.probe_versions)
                if candidate and match:
                    candidate["evidence"].append({"kind": "directory-version-hint", "value": match.group(1)})
                candidates.append(candidate)
            direct = self.find_command("clangd")
            if direct:
                candidates.append(native_candidate(direct, "path", probe=self.probe_versions))
        elif lang == "markdown":
            for candidate in self.npm("vscode-langservers-extracted", "vscode-markdown-language-server", "JsonLanguageServer"):
                parser_root = Path(candidate["paths"]["module_root"]).parent / "markdown-it"
                parser_metadata = read_json(parser_root / "package.json")
                parser_path = canonical(parser_root / (parser_metadata.get("main", "index.mjs") if parser_metadata else "index.mjs"))
                if parser_metadata and parser_metadata.get("name") == "markdown-it" and contained(parser_path, parser_root) and parser_path.is_file():
                    candidate["paths"]["parser"] = str(parser_path)
                    candidate["paths"]["parser_path"] = str(parser_path)
                    candidate["evidence"].append({"kind": "parser-dependency", "identity": "markdown-it", "version": parser_metadata.get("version"), "path": str(parser_path)})
                    candidates.append(candidate)
            if not candidates:
                cache = self.serena / "Marksman"
                for entry in [*cache.glob("**/marksman.exe"), *cache.glob("**/marksman")]:
                    candidates.append(native_candidate(entry, "serena-cache", probe=self.probe_versions))
                direct = self.find_command("marksman")
                if direct:
                    candidates.append(native_candidate(direct, "path", probe=self.probe_versions))
        elif lang == "csharp":
            cache = self.serena / "CSharpLanguageServer"
            for entry in cache.glob("**/Microsoft.CodeAnalysis.LanguageServer.exe"):
                candidates.append(native_candidate(entry, "serena-cache"))
            # VS Code's installed C# extension can own the same Roslyn distribution.
            for extensions in [self.home / ".vscode/extensions", self.home / ".vscode-insiders/extensions", self.home / ".cursor/extensions", self.home / ".vscodium/extensions"]:
                for extension in extensions.glob("ms-dotnettools.csharp-*"):
                    for entry in extension.glob(".roslyn/**/Microsoft.CodeAnalysis.LanguageServer.exe"):
                        candidates.append(native_candidate(entry, "vscode-extension"))
            for entry in (self.home / ".dotnet/tools/.store").glob("**/Microsoft.CodeAnalysis.LanguageServer.exe"):
                candidates.append(native_candidate(entry, "dotnet-tool"))
            for name in ("csharp-ls", "OmniSharp"):
                direct = self.find_command(name)
                if direct:
                    candidate = native_candidate(direct, "path")
                    if candidate:
                        candidate["provenance"]["alternative_backend"] = name
                        candidates.append(candidate)
        elif lang == "qml":
            for name in ("qmlls", "qmlls6"):
                direct = self.find_command(name)
                if direct:
                    candidates.append(native_candidate(direct, "qt-sdk", probe=self.probe_versions))
        elif lang == "xml":
            # Distribution identity must be verified before selecting an arbitrary JAR.
            direct = self.find_command("lemminx")
            if direct:
                candidates.append(native_candidate(direct, "path"))
            for entry in (self.serena / "Lemminx").glob("**/lemminx*.exe"):
                candidates.append(native_candidate(entry, "serena-cache"))
        elif lang == "toml":
            for entry in (self.serena / "Taplo").glob("**/taplo.exe"):
                candidates.append(native_candidate(entry, "serena-cache", probe=self.probe_versions))
        elif lang == "cmake":
            for entry in (self.serena / "NeoCMakeLanguageServer").glob("**/neocmakelsp.exe"):
                candidates.append(native_candidate(entry, "serena-cache", probe=self.probe_versions))
            direct = self.find_command("neocmakelsp")
            if direct:
                candidates.append(native_candidate(direct, "path", probe=self.probe_versions))
        record = select(base_record(spec), candidates)
        record.update({"required": spec["required"], "backend": spec["backend"], "serena_id": spec.get("serena_id"),
                       "project_inputs": spec["project_inputs"], "runtime_prerequisites": spec["runtime"],
                       "automatic_diagnostics": "unverified", "exposed_operations": []})
        if lang == "delphi":
            record["sdk_candidates"] = delphi_sdks() if self.environment else []
            compilers = list((self.serena / "PascalLanguageServer/prerequisites").glob("*/bin/*/fpc.exe"))
            if self.environment:
                for value in (os.environ.get("PP"), self.find_command("fpc")):
                    if value and Path(value).name.lower() == "fpc.exe" and Path(value).is_file():
                        compilers.append(canonical(value))
                for root in map(Path, ("D:/laz32", "D:/laz64", "C:/lazarus", "D:/lazarus", "C:/FPC", "D:/FPC")):
                    if root.is_dir():
                        compilers.extend(root.glob("**/bin/*/fpc.exe"))
            compilers = unique_paths(compilers)
            record["fpc_candidates"] = [{"pp": str(p)} for p in compilers]
            if len(compilers) == 1:
                compiler = compilers[0]
                source_candidates = [compiler.parents[2] / "source", compiler.parents[2] / "fpcsrc", compiler.parents[3] / "fpcsrc"]
                if self.environment and os.environ.get("FPCDIR"):
                    source_candidates.insert(0, canonical(os.environ["FPCDIR"]))
                sources = next((p for p in source_candidates if (p / "rtl").is_dir()), None)
                record.setdefault("paths", {})["pp"] = str(compiler)
                if sources:
                    record["paths"]["fpcdir"] = str(sources)
                    record["lsp"] = {"env": {"PP": str(compiler), "FPCDIR": str(sources)}}
                    target = {}
                    try:
                        for option, key in (("-iTO", "FPCTARGET"), ("-iTP", "FPCTARGETCPU")):
                            queried = subprocess.run([str(compiler), "-n", option], capture_output=True, text=True, timeout=5,
                                                     creationflags=0x08000000 if os.name == "nt" else 0)
                            value = queried.stdout.strip()
                            if queried.returncode or not re.fullmatch(r"[a-zA-Z0-9_]+", value):
                                raise ValueError("FPC target query did not return one target identifier")
                            target[key] = value
                        record["lsp"]["env"].update(target)
                        record["evidence"].append({"kind": "fpc-target", "source": "selected compiler -n -iTO / -n -iTP", **target})
                    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
                        record["evidence"].append({"kind": "unverified-fpc-target", "reason": type(error).__name__})
            record["evidence"].append({"kind": "unverified-prerequisite", "detail": "Installed pasls alone does not prove Delphi SDK/dialect support."})
        elif lang == "bash":
            binaries = list((self.serena / "BashLanguageServer").glob("**/shellcheck.exe"))
            direct = self.find_command("shellcheck")
            if direct:
                binaries.append(canonical(direct))
            record["analyzers"] = [{"name": "ShellCheck", "path": str(p), "exists": p.is_file()} for p in unique_paths(binaries)]
            record["health"]["analyzer_available"] = bool(binaries)
            resolved = [p for p in unique_paths(binaries) if p.is_file()]
            if len(resolved) == 1:
                record["paths"]["shellcheck"] = str(resolved[0])
        if not spec["required"]:
            record["conditional"] = spec["conditional"]
            record["eligible_for_activation"] = record["status"] == "adopted"
        return record

    def run(self):
        mcp = []
        for spec in self.catalogue["mcp"]:
            if spec["manager"] == "uv":
                candidates = [uv_candidate(spec, self.uv, self.verify_records)]
            else:
                candidates = self.npm(spec["package"], spec["command"])
            record = select(base_record(spec), candidates)
            if spec["id"] == "graphify":
                record["excluded_installations"] = [
                    {"identity": "@dreamtree-org/graphify", "installation_root": c["installation_root"],
                     "version": c["version"], "reason": "Unrelated package; command name is not identity."}
                    for c in self.npm("@dreamtree-org/graphify")]
                record["shared_service"] = self.graphify_service()
            elif spec["id"] == "nuphus":
                model_root = canonical((os.environ.get("NUPHUS_MODELS_DIR") if self.environment else None) or self.home / "AppData/Roaming/Nuphus/models")
                record["models"] = [{"path": str(model_root / name), "exists": (model_root / name).is_file()}
                                    for name in ("ch_PP-OCRv4_det.onnx", "ch_PP-OCRv4_rec.onnx", "ch_PP-OCR_keys_v1.txt")]
                original = record.get("paths", {}).get("original_native_executable")
                if original:
                    record["paths"]["onnxruntime"] = str(Path(original).parent / "onnxruntime.dll")
                    record["health"]["onnxruntime_exists"] = Path(record["paths"]["onnxruntime"]).is_file()
            mcp.append(record)
        languages = [self.language(spec) for spec in self.catalogue["languages"]]
        if self.processes:
            self.inspect_consumers(mcp + languages)
        return {"schema_version": 1, "observed_at": datetime.now(timezone.utc).isoformat(),
                "catalogue": str(self.catalogue_path), "user_home": str(self.home), "read_only": True,
                "release_checks": "not-requested; explicit lifecycle operation required",
                "mcp": mcp, "languages": languages}

    def inspect_consumers(self, records):
        # --user-home selects dependency roots, not an impersonated OS account.
        # An explicit process check must inspect this real Windows host even for
        # an alternate/clean dependency root; it still emits absolute path matches only.
        inspector = self.pwsh or shutil.which("pwsh") or shutil.which("powershell")
        if os.name != "nt" or not inspector:
            for record in records:
                record["active_consumers"] = {"state": "unavailable", "processes": [], "reason": "Windows host process inspection requires an existing host PowerShell executable."}
            return
        try:
            # Arguments can contain secrets. Capture in memory, match paths, emit PID/exe only.
            process = subprocess.run([inspector, "-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
                                      "Get-CimInstance Win32_Process | Select-Object ProcessId,ExecutablePath,CommandLine | ConvertTo-Json -Compress"],
                                     capture_output=True, text=True, timeout=20, creationflags=0x08000000)
            if process.returncode:
                raise ValueError("Process inspection failed")
            snapshot = json.loads(process.stdout)
            if isinstance(snapshot, dict):
                snapshot = [snapshot]
            if not isinstance(snapshot, list):
                raise ValueError("Invalid process snapshot")
            attach_consumers(records, snapshot)
        except (OSError, ValueError, subprocess.SubprocessError):
            for record in records:
                record["active_consumers"] = {"state": "unavailable", "processes": [], "reason": "Process inspection failed; no command lines are retained."}

    def graphify_service(self):
        data = read_json(self.graphify_manifest) if self.graphify_manifest else None
        result = {"state": "unverified", "manifest": str(self.graphify_manifest) if self.graphify_manifest else None}
        if not isinstance(data, dict):
            return result
        # Whitelist paths only; never serialize the manifest, credentials or endpoint headers.
        config = data.get("graphify", {}).get("configuration", {})
        for key in ("python", "graph"):
            item = config.get(key, {})
            path = item.get("path") if isinstance(item, dict) else None
            if isinstance(path, str):
                result[key + "_path"] = path
                result[key + "_exists"] = Path(path).is_file()
        module = config.get("module", {})
        if isinstance(module, dict):
            result["module_name"] = module.get("name")
            result["package_version"] = module.get("packageVersion")
        result["authentication"] = "not-read; connection layer must resolve securely"
        return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--user-home", default=str(Path.home()))
    parser.add_argument("--catalogue", default=str(CATALOGUE))
    parser.add_argument("--npm-prefix", action="append", default=[])
    parser.add_argument("--uv-tools-dir")
    parser.add_argument("--serena-cache")
    parser.add_argument("--rustup-home")
    parser.add_argument("--graphify-manifest")
    parser.add_argument("--verify-records", action="store_true")
    parser.add_argument("--probe-versions", action="store_true", help="Run bounded --version probes on resolved native binaries, never rustup shims")
    parser.add_argument("--processes", action="store_true", help="Inspect active Windows consumers; persist only matching PID and executable, never arguments")
    parser.add_argument("--output", help="Write JSON explicitly to this path; otherwise stdout only")
    args = parser.parse_args(argv)
    result = Discovery(args.user_home, args.catalogue, args.npm_prefix, args.uv_tools_dir,
                       args.serena_cache, args.rustup_home, verify_records=args.verify_records,
                       probe_versions=args.probe_versions, graphify_manifest=args.graphify_manifest, processes=args.processes).run()
    value = json.dumps(result, indent=2, ensure_ascii=False) + "\n"
    if args.output:
        target = canonical(args.output)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(value, encoding="utf-8")
    else:
        print(value, end="")


if __name__ == "__main__":
    main()
