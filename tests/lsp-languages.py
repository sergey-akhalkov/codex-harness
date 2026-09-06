"""Real installed language-server acceptance, confined to disposable workspaces.

No dependency installation. Each selected language must detect an intentional
error, clear it after correction and return actual document symbols.
"""
from __future__ import annotations

import argparse
import json
import logging
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools/lsp"))
from backend import Backend


CASES = {
    "typescript": dict(file="example.ts", bad='export function example(): number { return "wrong"; }\n', good='export function example(): number { return 42; }\n', support={"tsconfig.json": '{"compilerOptions":{"strict":true,"noEmit":true}}'}),
    "javascript": dict(file="example.js", bad='// @ts-check\n/** @returns {number} */\nexport function example() { return "wrong"; }\n', good='// @ts-check\n/** @returns {number} */\nexport function example() { return 42; }\n', support={"jsconfig.json": '{"compilerOptions":{"checkJs":true,"noEmit":true}}'}),
    "markdown": dict(file="example.md", bad='# Example\n\n[missing](#absent)\n', good='# Example\n\n[self](#example)\n', support={}),
    "toml": dict(file="example.toml", bad='example = \n', good='example = 42\n', support={}),
    "cmake": dict(file="CMakeLists.txt", bad='include( (\n', good='set(EXAMPLE value)\n', support={}),
    "bash": dict(file="example.sh", bad='#!/bin/bash\nexample() {\n  echo "unterminated\n}\n', good='#!/bin/bash\nexample() {\n  echo "value"\n}\n', support={}),
    "json": dict(file="example.json", bad='{"$schema":"./schema.json","example":"wrong"}\n', good='{"$schema":"./schema.json","example":42}\n',
        support={"schema.json": '{"type":"object","properties":{"example":{"type":"integer"}},"required":["example"]}'}),
    "xml": dict(file="example.xml", bad='<example xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="schema.xsd">wrong</example>\n',
        good='<example xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="schema.xsd">42</example>\n',
        support={"schema.xsd": '<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="example" type="xs:int"/></xs:schema>'}),
    "csharp": dict(file="Example.cs", bad='namespace Example { public static class Values { public static int Value() { return "wrong"; } } }\n', good='namespace Example { public static class Values { public static int Value() { return 42; } } }\n',
        support={"Example.csproj": '<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>'}),
    "css": dict(file="example.css", bad='.example { color: ; }\n', good='.example { color: red; }\n', support={}),
    "html": dict(file="example.html", bad='<html><head><script>function example() { return ( ; }</script></head><body id="example"></body></html>\n',
        good='<html><head><script>function example() { return 42; }</script></head><body id="example"></body></html>\n', support={}),
    "cpp": dict(file="example.cpp", bad='int example() { return "wrong"; }\n', good='int example() { return 42; }\n', support={}),
    "python": dict(file="example.py", bad='def example() -> int:\n    return "wrong"\n', good='def example() -> int:\n    return 42\n', support={}),
    "powershell": dict(file="example.ps1", bad='function Get-Example { $value = }\n', good='function Get-Example { return 42 }\n', support={}),
    "rust": dict(file="src/lib.rs", bad='pub fn example() -> i32 { "wrong" }\n', good='pub fn example() -> i32 { 42 }\n',
        support={"Cargo.toml": '[package]\nname = "harness_probe"\nversion = "0.1.0"\nedition = "2021"\n',
                 "Cargo.lock": 'version = 4\n[[package]]\nname = "harness_probe"\nversion = "0.1.0"\n'}),
    "pascal": dict(file="units/Example.pas", bad="unit Example;\ninterface\n{$I Build.inc}\nuses SysUtils;\nfunction Value: Integer;\nfunction Caption: string;\nimplementation\nfunction Value: Integer;\nbegin\n{$IFDEF HARNESS_TEST}\n  Result := 'wrong';\n{$ELSE}\n  Result := 0;\n{$ENDIF}\nend;\nfunction Caption: string;\nbegin\n  Result := IntToStr(Value);\nend;\nend.\n",
        good="unit Example;\ninterface\n{$I Build.inc}\nuses SysUtils;\nfunction Value: Integer;\nfunction Caption: string;\nimplementation\nfunction Value: Integer;\nbegin\n{$IFDEF HARNESS_TEST}\n  Result := 42;\n{$ELSE}\n  Result := 0;\n{$ENDIF}\nend;\nfunction Caption: string;\nbegin\n  Result := IntToStr(Value);\nend;\nend.\n",
        support={"include/Build.inc": '{$DEFINE HARNESS_TEST}\n',
            "DelphiFixture.dpr": "program DelphiFixture;\n{$APPTYPE CONSOLE}\nuses Example in 'units\\Example.pas';\nbegin\n  Writeln(Caption);\nend.\n",
            "DelphiFixture.dproj": '<Project xmlns="http://schemas.microsoft.com/developer/msbuild/2003"><PropertyGroup><ProjectVersion>12.0</ProjectVersion><MainSource>DelphiFixture.dpr</MainSource><Config Condition="\'$(Config)\'==\'\'">Debug</Config><DCC_UnitSearchPath>units</DCC_UnitSearchPath><DCC_IncludePath>include</DCC_IncludePath></PropertyGroup><PropertyGroup Condition="\'$(Config)\'==\'Debug\'"><DCC_Define>DEBUG</DCC_Define></PropertyGroup><Import Project="$(BDS)\\Bin\\CodeGear.Delphi.Targets"/></Project>'}),
}


def run(language, base):
    case = CASES[language]
    root = base / language
    root.mkdir()
    for name, text in {**case["support"], case["file"]: case["bad"]}.items():
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8", newline="\n")
    backend = None
    began = time.monotonic()
    report = {"language": language, "passed": False}
    try:
        if language == "csharp":
            empty_feed = base / "empty-nuget-feed"
            empty_feed.mkdir()
            restored = subprocess.run(["dotnet", "restore", "--source", str(empty_feed)], cwd=root, capture_output=True, timeout=30)
            if restored.returncode:
                raise RuntimeError("Disposable C# fixture offline restore failed: " + restored.stdout.decode("utf-8", errors="replace"))
        backend = Backend(root, language, base / "state" / language, case["file"])
        report["command"] = backend.definition["command"]
        report["capabilities"] = backend.capabilities
        report["error"] = backend.diagnostics(case["file"], timeout=20)
        report["server_status"] = backend.server_status
        report["notifications"] = list(backend.published.values())
        if report["error"]["status"] != "diagnostics" or not report["error"]["diagnostics"]:
            raise AssertionError("Known erroneous source must produce current diagnostics")
        (root / case["file"]).write_text(case["good"], encoding="utf-8", newline="\n")
        report["clearance"] = backend.diagnostics(case["file"], timeout=20)
        if report["clearance"]["status"] != "clean":
            raise AssertionError("Corrected source must produce an authoritative empty result")
        report["symbols"] = backend.navigation(case["file"], "symbols")
        if not report["symbols"]:
            raise AssertionError("Representative source must yield document symbols")
        if language == "pascal":
            report["project"] = backend.delphi_project
            report["cross_unit_definition"] = backend.navigation("DelphiFixture.dpr", "definition", 4, 12)
            if not report["cross_unit_definition"]:
                raise AssertionError("Delphi project navigation did not cross unit boundaries")
            report["sdk_definition"] = backend.navigation(case["file"], "definition", 17, 15)
            if "SysUtils.pas".casefold() not in json.dumps(report["sdk_definition"]).casefold() or "DelphiRAD".casefold() not in json.dumps(report["sdk_definition"]).casefold():
                raise AssertionError("Delphi SDK definition did not resolve to the actual installed SDK")
            report["references"] = backend.navigation(case["file"], "references", 4, 11)
            if len(report["references"]) < 2:
                raise AssertionError("Delphi references did not include the representative caller")
        report["passed"] = True
    except Exception as error:
        report["failure"] = f"{type(error).__name__}: {error}"
    finally:
        if backend:
            backend.close()
    if language in ("cmake", "pascal"):
        actual_files = {path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file()}
        expected_files = {*case["support"], case["file"]}
        report["unexpected_project_files"] = sorted(actual_files - expected_files)
        if report["unexpected_project_files"]:
            report.update(passed=False, failure="Language analysis wrote runtime files into the project")
    report["elapsed_seconds"] = round(time.monotonic() - began, 3)
    return report


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser()
    parser.add_argument("languages", nargs="+", choices=sorted(CASES))
    parser.add_argument("--registry")
    parser.add_argument("--report")
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--describe", action="store_true", help="Print fixture inputs without starting any backend")
    options = parser.parse_args()
    if options.describe:
        print(json.dumps({language: CASES[language] for language in options.languages}, ensure_ascii=False))
        return 0
    logging.basicConfig(level=logging.DEBUG if options.verbose else logging.ERROR)
    if options.registry:
        os.environ["HARNESS_LSP_REGISTRY"] = str(Path(options.registry).resolve())
    with tempfile.TemporaryDirectory(prefix="harness-languages-") as temporary:
        base = Path(temporary)
        os.environ["CODEX_HOME"] = str(base / "codex-home")
        reports = []
        for language in options.languages:
            result = run(language, base)
            reports.append(result)
            print(json.dumps(result, ensure_ascii=False), flush=True)
        if options.report:
            Path(options.report).write_text(json.dumps(reports, ensure_ascii=False, indent=2), encoding="utf-8")
    return 0 if all(item["passed"] for item in reports) else 1


if __name__ == "__main__":
    raise SystemExit(main())
