"""Generate machine-owned LSP configuration from the existing discovery record.

Prints JSON only; the installation transaction owns atomic writes and rollback.
No downloads, package probes, server starts or filesystem mutations.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

REQUIRED = {"rust", "typescript", "javascript", "powershell", "python", "delphi", "cpp", "csharp", "json", "markdown", "toml", "xml", "cmake", "bash"}
CONDITIONAL = {"yaml", "qml", "html", "css"}


def generate(inventory: dict) -> dict:
    servers = {}
    records = {record["id"]: record for record in inventory.get("languages", [])}
    for language in sorted(REQUIRED | CONDITIONAL):
        record = records.get(language, {})
        key = "pascal" if language == "delphi" else language
        command = list(record.get("command") or [])
        item = {"command": command, "adapter": "push", "language": language,
            "required": language in REQUIRED, "version": record.get("version"),
            "provenance": record.get("provenance", {}), "status": "configured-unverified"}
        paths = record.get("paths") or {}
        if not command:
            item.update(status="unavailable", reason="No installed compatible backend in discovery")
        elif language in {"typescript", "javascript", "python", "json", "yaml", "html", "css"}:
            if "--stdio" not in command:
                command.append("--stdio")
            if language in {"typescript", "javascript"}:
                item.update(adapter="typescript", initialization_options={"preferences": {"disableAutomaticTypingAcquisition": True}})
        elif language == "powershell":
            script = paths.get("executable") or command[-1]
            command.extend(["-HostName", "CodexHarness", "-HostProfileId", "CodexHarness", "-HostVersion", "1.0.0",
                "-BundledModulesPath", paths.get("bundled_modules") or str(Path(script).parent), "-Stdio"])
            if paths.get("analyzer"):
                item["analyzer_path"] = paths["analyzer"]
        elif language == "rust":
            item.update(adapter="pull", project_toolchain=True,
                initialization_options={"cargo": {"extraArgs": ["--locked", "--offline"]}})
        elif language == "delphi":
            item.update(initialization_options={"checkSyntax": True, "publishDiagnostics": True})
            item["sdk_candidates"] = record.get("sdk_candidates", [])
            if record.get("version") == "v0.2.0":
                item["adapter"] = "pasls-0.2"
            if paths.get("pp") and paths.get("fpcdir"):
                item["env"] = {"PP": paths["pp"], "FPCDIR": paths["fpcdir"]}
                # CodeTools otherwise follows the 64-bit host even when the
                # selected, provisioned compiler/RTL cohort is Win32 i386.
                # Restrict this inference to the compiler's explicit target dir.
                if Path(paths["pp"]).parent.name.lower() == "i386-win32":
                    item["env"].update(FPCTARGETCPU="i386", FPCTARGET="win32")
        elif language == "cpp":
            command.append("--background-index=false")
        elif language == "csharp":
            command.extend(["--stdio", "--logLevel=Information"])
            item["adapter"] = "pull"
        elif language == "markdown":
            if any("vscode-markdown-language-server" in argument for argument in command):
                command.append("--stdio")
                item.update(adapter="markdown-vscode", parser_path=paths.get("parser"))
            else:
                command.append("server")
        elif language == "toml":
            if Path(command[0]).stem.lower() != "taplo":
                item.update(status="unavailable", reason="This Taplo distribution has no LSP; select the native LSP-enabled build")
            else:
                command.extend(["lsp", "stdio"])
        elif language == "cmake":
            if Path(command[0]).stem.lower() != "neocmakelsp":
                item.update(status="unavailable", reason="Classic cmake-language-server has no diagnostics; select neocmakelsp")
            else:
                command.append("stdio")
                item["initialization_options"] = {"lint": {"enable": True}, "format": {"enable": False}, "scan_cmake_in_package": False}
        elif language == "bash":
            command.append("start")
            if paths.get("shellcheck"):
                item["env"] = {"SHELLCHECK_PATH": paths["shellcheck"]}
        # Explicit settings are host-owned configuration, never copied from an
        # unrelated editor configuration during runtime.
        for option in ("settings", "initialization_options", "encoding", "env", "analyzer_path", "schema_files", "project_properties"):
            if option in record.get("lsp", {}):
                item[option] = ({**item.get("env", {}), **record["lsp"][option]} if option == "env" else record["lsp"][option])
        servers[key] = item
    return {"schema_version": 1, "servers": servers}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--inventory", required=True)
    arguments = parser.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")
    inventory = json.loads(Path(arguments.inventory).read_text(encoding="utf-8-sig"))
    print(json.dumps(generate(inventory), ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
