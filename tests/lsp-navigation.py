"""Actual advertised cross-file definitions/references in disposable projects.

No backend installation or project outside the fixture is modified. Unsupported
operations are reported from negotiated capabilities, never emulated as results.
"""
from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import json
import logging
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from urllib.parse import unquote, urlparse

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools/lsp"))
from backend import Backend


def case(files, caller, declaration, symbol, support=None, reference_symbol=None, references_file=None, limitation=None):
    return {"files": files, "caller": caller, "declaration": declaration, "symbol": symbol,
            "support": support or {}, "reference_symbol": reference_symbol or symbol,
            "references_file": references_file or caller, "limitation": limitation}


CASES = {
    "typescript": case({"lib.ts": "export function answer(): number { return 42; }\n", "main.ts": 'import { answer } from "./lib";\nexport const value = answer();\n'}, "main.ts", "lib.ts", "answer", {"tsconfig.json": '{"compilerOptions":{"strict":true,"noEmit":true}}'}),
    "javascript": case({"lib.js": "export function answer() { return 42; }\n", "main.js": 'import { answer } from "./lib.js";\nexport const value = answer();\n'}, "main.js", "lib.js", "answer", {"jsconfig.json": '{"compilerOptions":{"checkJs":true,"noEmit":true}}'}),
    "python": case({"lib.py": "def answer() -> int:\n    return 42\n", "main.py": "from lib import answer\nvalue = answer()\n"}, "main.py", "lib.py", "answer", {"pyrightconfig.json": '{"include":["*.py"],"typeCheckingMode":"strict"}'}),
    "powershell": case({"lib.ps1": "function Get-Answer { return 42 }\n", "main.ps1": '. "$PSScriptRoot/lib.ps1"\nGet-Answer\n'}, "main.ps1", "lib.ps1", "Get-Answer"),
    "pascal": case({"units/Helper.pas": "unit Helper;\ninterface\nfunction Answer: Integer;\nimplementation\nfunction Answer: Integer;\nbegin\n  Result := 42;\nend;\nend.\n", "units/Main.pas": "unit Main;\ninterface\nfunction Value: Integer;\nimplementation\nuses Helper, SysUtils;\nfunction Value: Integer;\nbegin\n  Result := Answer;\nend;\nend.\n"}, "units/Main.pas", "units/Helper.pas", "Answer", {"Navigation.dpr": "program Navigation;\nuses Main in 'units\\Main.pas', Helper in 'units\\Helper.pas';\nbegin\n  Writeln(Value);\nend.\n", "Navigation.dproj": '<Project xmlns="http://schemas.microsoft.com/developer/msbuild/2003"><PropertyGroup><ProjectVersion>12.0</ProjectVersion><MainSource>Navigation.dpr</MainSource><Config>Debug</Config><DCC_UnitSearchPath>units</DCC_UnitSearchPath></PropertyGroup><Import Project="$(BDS)\\Bin\\CodeGear.Delphi.Targets"/></Project>'}),
    "bash": case({"helper.sh": "answer() {\n  echo value\n}\n", "main.sh": "#!/bin/bash\nsource ./helper.sh\nanswer\n"}, "main.sh", "helper.sh", "answer"),
    "cpp": case({"lib.hpp": "inline int answer() { return 42; }\n", "main.cpp": '#include "lib.hpp"\nint value() { return answer(); }\n'}, "main.cpp", "lib.hpp", "answer"),
    "rust": case({"src/helper.rs": "pub fn answer() -> i32 { 42 }\n", "src/lib.rs": "mod helper;\npub fn value() -> i32 { helper::answer() }\n"}, "src/lib.rs", "src/helper.rs", "answer", {"Cargo.toml": '[package]\nname="navigation_probe"\nversion="0.1.0"\nedition="2021"\n', "Cargo.lock": 'version = 4\n[[package]]\nname = "navigation_probe"\nversion = "0.1.0"\n'}),
    "csharp": case({"Lib.cs": "namespace Probe;\npublic static class Lib { public static int Answer() => 42; }\n", "Main.cs": "namespace Probe;\npublic static class MainValue { public static int Value() => Lib.Answer(); }\n"}, "Main.cs", "Lib.cs", "Answer", {"Probe.csproj": '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>'}),
    "markdown": case({"guide.md": "# Answer\n\nDetails.\n", "readme.md": "[Go](guide.md#answer)\n"}, "readme.md", "guide.md", "answer", reference_symbol="Answer"),
    "cmake": case({"helpers.cmake": 'function(answer)\n  message(STATUS "value")\nendfunction()\n', "CMakeLists.txt": "cmake_minimum_required(VERSION 3.20)\nproject(Navigation NONE)\ninclude(helpers.cmake)\nanswer()\n"}, "CMakeLists.txt", "helpers.cmake", "answer"),
    "xml": case({"lib.xsd": '<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">\n<xs:simpleType name="AnswerType"><xs:restriction base="xs:int"/></xs:simpleType>\n<xs:element name="localAnswer" type="AnswerType"/>\n</xs:schema>\n', "main.xsd": '<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">\n<xs:include schemaLocation="lib.xsd"/>\n<xs:element name="answer" type="AnswerType"/>\n</xs:schema>\n'}, "main.xsd", "lib.xsd", "AnswerType", references_file="lib.xsd", limitation="LemMinX XSDUtils.searchXSOriginAttributes scans the declaration's owner document: cross-schema definitions work, XSD references are document-local."),
    "json": case({"example.json": '{"answer":42}\n'}, "example.json", "example.json", "answer", {"schema.json": '{"type":"object","properties":{"answer":{"type":"integer"}}}'}),
    "toml": case({"example.toml": "#:schema ./schema.json\nanswer = 42\n"}, "example.toml", "schema.json", "answer", {"schema.json": '{"type":"object","properties":{"answer":{"type":"integer"}}}'}),
}


def position(text, symbol, last=False):
    offset = text.rindex(symbol) if last else text.index(symbol)
    line = text.count("\n", 0, offset)
    character = offset - text.rfind("\n", 0, offset) - 1
    return line, character + 1


def result_paths(value):
    if value is None:
        return []
    values = value if isinstance(value, list) else [value]
    paths = []
    for item in values:
        uri = item.get("uri") or item.get("targetUri")
        if uri and urlparse(uri).scheme == "file":
            name = unquote(urlparse(uri).path)
            if os.name == "nt" and name.startswith("/"):
                name = name[1:]
            paths.append(Path(name).resolve())
    return paths


def advertised(backend, operation):
    key = operation + "Provider"
    return (key in backend.capabilities and backend.capabilities[key] is not None and backend.capabilities[key] is not False
            or "textDocument/" + operation in backend.dynamic_capabilities)


def extra_operations(backend, fixture, contents, root):
    """Exercise source-advertised requests without claiming semantics for empty results."""
    checks = {}
    declaration = fixture["declaration"]
    caller = fixture["caller"]
    for operation, provider in (("hover", "hover"), ("implementation", "implementation"),
                                ("type_definition", "typeDefinition"), ("declaration", "declaration"),
                                ("workspace_symbols", "workspaceSymbol"), ("rename_preview", "rename"),
                                ("prepare_call_hierarchy", "callHierarchy")):
        supported = advertised(backend, provider)
        if operation == "workspace_symbols":
            supported = supported or "workspace/symbol" in backend.dynamic_capabilities
        if not supported:
            checks[operation] = {"state": "not-advertised"}
            continue
        source = declaration if operation in ("rename_preview", "prepare_call_hierarchy") else caller
        symbol = fixture["reference_symbol"] if source == declaration else fixture["symbol"]
        line, character = position(contents[source], symbol, last=source == caller)
        arguments = {"query": fixture["symbol"], "new_name": fixture["reference_symbol"] + "Renamed"}
        result = backend.navigation(source, operation, line, character, **arguments)
        check = {"state": "answered" if result else "empty-result", "semantic_evidence": bool(result)}
        if isinstance(result, list):
            check["items"] = len(result)
        if operation == "rename_preview" and result:
            edits = result.get("documentChanges", [])
            uris = list(result.get("changes", {})) + [edit.get("textDocument", {}).get("uri") for edit in edits]
            paths = result_paths([{"uri": uri} for uri in uris if uri])
            if not paths or any(not path.is_relative_to(root.resolve()) for path in paths):
                raise AssertionError("Rename preview returned no source edit or escaped the disposable project")
            check["edited_files"] = len(set(paths))
            check["applied"] = False
        if operation == "prepare_call_hierarchy" and result:
            item = result[0]
            for nested in ("incoming_calls", "outgoing_calls"):
                calls = backend.navigation(source, nested, item=item)
                checks[nested] = {"state": "answered" if calls else "empty-result", "items": len(calls or []), "semantic_evidence": bool(calls)}
        checks[operation] = check
    for name, content in fixture["files"].items():
        if (root / name).read_text(encoding="utf-8") != content:
            raise AssertionError("Read-only navigation unexpectedly changed fixture source: " + name)
    return checks


def run_one(language, base):
    root = base / language
    root.mkdir()
    backend = None
    report = {"language": language, "passed": False, "checks": {}}
    began = time.monotonic()
    try:
        fixture = CASES[language]
        contents = {**fixture["support"], **fixture["files"]}
        for name, content in contents.items():
            target = root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8", newline="\n")
        if language == "cpp":
            (root / "compile_commands.json").write_text(json.dumps([{"directory": str(root), "file": str(root / "main.cpp"), "arguments": ["clang++", "-std=c++17", "-I" + str(root), "-c", str(root / "main.cpp")]}]))
        if language == "csharp":
            feed = root / "empty-nuget-feed"
            feed.mkdir()
            restored = subprocess.run(["dotnet", "restore", "--source", str(feed)], cwd=root, capture_output=True, timeout=30)
            if restored.returncode:
                raise RuntimeError("C# fixture offline restore failed")
        backend = Backend(root, language, base / "state" / language, fixture["caller"])
        for name in fixture["files"]:
            backend.sync(name)
        readiness = backend.diagnostics(fixture["caller"], timeout=25)
        if readiness["status"] == "pending":
            readiness = backend.diagnostics(fixture["caller"], timeout=25)
        report["readiness"] = {key: readiness.get(key) for key in ("status", "reason", "freshness")}
        report["capabilities"] = {key: backend.capabilities.get(key) for key in ("definitionProvider", "referencesProvider", "documentSymbolProvider") if key in backend.capabilities}
        report["project_inputs"] = sorted(fixture["support"])
        if fixture["limitation"]:
            report["limitation"] = fixture["limitation"]
        if language == "cpp":
            report["project_inputs"].append("compile_commands.json")
        symbols = backend.navigation(fixture["declaration"] if language != "toml" else fixture["caller"], "symbols")
        if not symbols:
            raise AssertionError("Representative declaration must expose actual document symbols")
        report["checks"]["symbols"] = {"state": "passed", "items": len(symbols)}
        for operation in ("definition", "references"):
            if not advertised(backend, operation):
                report["checks"][operation] = {"state": "not-advertised"}
                continue
            source = fixture["caller"] if operation == "definition" else fixture["declaration"]
            symbol = fixture["symbol"] if operation == "definition" else fixture["reference_symbol"]
            line, character = position(contents[source], symbol, last=operation == "definition")
            result = backend.navigation(source, operation, line, character)
            paths = result_paths(result)
            expected = (root / (fixture["declaration"] if operation == "definition" else fixture["references_file"])).resolve()
            if expected not in paths:
                raise AssertionError(operation + " did not resolve the expected project file; returned " + json.dumps(result, ensure_ascii=False))
            report["checks"][operation] = {"state": "passed", "locations": len(paths), "cross_file": (root / source).resolve() != expected}
        report["advertised_requests"] = extra_operations(backend, fixture, contents, root)
        report["passed"] = True
    except Exception as error:
        report["failure"] = f"{type(error).__name__}: {error}"
    finally:
        if backend:
            backend.close()
        report["elapsed_seconds"] = round(time.monotonic() - began, 2)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("languages", nargs="*", choices=sorted(CASES))
    parser.add_argument("--registry", required=True)
    parser.add_argument("--report", required=True)
    parser.add_argument("--workers", type=int, default=3, choices=range(1, 4))
    arguments = parser.parse_args()
    logging.basicConfig(level=logging.ERROR)
    os.environ["HARNESS_LSP_REGISTRY"] = str(Path(arguments.registry).resolve())
    with tempfile.TemporaryDirectory(prefix="harness-navigation-") as temporary:
        base = Path(temporary)
        os.environ["CODEX_HOME"] = str(base / "codex-home")
        with ThreadPoolExecutor(max_workers=arguments.workers) as pool:
            reports = list(pool.map(lambda language: run_one(language, base), arguments.languages or sorted(CASES)))
        Path(arguments.report).write_text(json.dumps(reports, ensure_ascii=False, indent=2), encoding="utf-8")
        for report in reports:
            print(json.dumps(report, ensure_ascii=False))
    return 0 if all(report["passed"] for report in reports) else 1


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8")
    raise SystemExit(main())
