"""Read Delphi project inputs and use the installed compiler without source writes.

The selected SDK owns Delphi syntax/type diagnostics. Existing pasls/CodeTools
provides navigation. This does not invoke MSBuild, project targets or executables.
"""
from __future__ import annotations

import os
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET


def expand(value: str, properties: dict) -> str:
    return re.sub(r"\$\(([^)]+)\)", lambda match: properties.get(match[1].casefold(), ""), value)


def condition(value: str, properties: dict) -> bool:
    if not value:
        return True
    value = expand(value, properties).strip()
    # The observed Delphi 2010 .dproj uses quoted equality with and/or.
    # Reject other MSBuild expressions instead of executing property functions.
    for alternative in re.split(r"\s+or\s+", value, flags=re.I):
        accepted = True
        for term in re.split(r"\s+and\s+", alternative, flags=re.I):
            match = re.fullmatch(r"\s*(['\"])(.*?)\1\s*(==|!=)\s*(['\"])(.*?)\4\s*", term)
            if not match:
                raise ValueError("Unsupported Delphi project condition: " + value)
            equal = match[2].casefold() == match[5].casefold()
            accepted = accepted and (equal if match[3] == "==" else not equal)
        if accepted:
            return True
    return False


def project_file(root: Path, source: Path | None = None) -> Path | None:
    directory = source.parent if source else root
    while directory.is_relative_to(root):
        candidates = list(directory.glob("*.dproj"))
        if len(candidates) > 1:
            raise ValueError("Multiple Delphi projects in " + str(directory) + "; select the applicable project root")
        if candidates:
            return candidates[0]
        if directory == root:
            break
        directory = directory.parent
    return None


def configure(root: Path, definition: dict, source: Path | None = None) -> dict:
    sdks = [candidate for candidate in definition.get("sdk_candidates", []) if Path(candidate.get("compiler", "")).is_file()]
    if len(sdks) != 1:
        raise FileNotFoundError("Select one installed Delphi SDK in the host LSP registry")
    sdk = sdks[0]
    sdk_root = Path(sdk["root"])
    if sdk_root.name != "7.0":
        raise ValueError("This verified Delphi project adapter requires the installed RAD Studio 2010 (BDS 7.0) SDK")
    project = project_file(root, source)
    directory = project.parent if project else root
    properties = {key.casefold(): value for key, value in os.environ.items()}
    properties.update(bds=str(sdk_root), platform="Win32")
    overrides = definition.get("project_properties", {})
    properties.update({key.casefold(): str(value) for key, value in overrides.items()})
    if project:
        tree = ET.parse(project).getroot()
        for group in tree:
            tag = group.tag.rsplit("}", 1)[-1]
            if tag == "Import":
                imported = group.get("Project", "")
                if imported and not re.search(r"(?:CodeGear|Borland)\.Delphi\.Targets$", imported, re.I):
                    raise ValueError("Additional Delphi property import requires explicit supported settings: " + imported)
            if tag != "PropertyGroup" or not condition(group.get("Condition", ""), properties):
                continue
            for entry in group:
                key = entry.tag.rsplit("}", 1)[-1].casefold()
                if key not in {name.casefold() for name in overrides} and condition(entry.get("Condition", ""), properties):
                    properties[key] = expand(entry.text or "", properties)
    if properties.get("platform", "Win32").casefold() not in ("win32", "x86"):
        raise ValueError("Selected Delphi compiler does not support project platform " + properties["platform"])

    def paths(name):
        result = []
        for value in properties.get(name.casefold(), "").split(";"):
            if value.strip():
                path = (directory / value.strip().strip('"')).resolve()
                if not path.is_dir():
                    raise FileNotFoundError("Delphi project search directory is missing: " + str(path))
                result.append(str(path))
        return result

    units = paths("DCC_UnitSearchPath")
    includes = paths("DCC_IncludePath")
    resources = paths("DCC_ResourcePath")
    objects = paths("DCC_ObjPath")
    defines = [value for value in properties.get("dcc_define", "").split(";") if value]
    aliases = properties.get("dcc_unitalias", "WinTypes=Windows;WinProcs=Windows;DbiProcs=BDE;DbiTypes=BDE;DbiErrs=BDE")
    sdk_sources = [str(path) for path in [sdk_root / "source/Win32/rtl/sys", sdk_root / "source/Win32/rtl/common",
        sdk_root / "source/Win32/rtl/win", sdk_root / "source/Win32/vcl"] if path.is_dir()]
    options = ["-Mdelphi", "-dMSWINDOWS", "-dWIN32", "-dVER210", "-uFPC", *["-d" + value for value in defines],
        *["-Fu" + path for path in [str(directory), *units, *sdk_sources]], *["-Fi" + path for path in [str(directory), *includes]]]
    main = properties.get("mainsource")
    initialization = {**definition.get("initialization_options", {}), "fpcOptions": options}
    if main:
        initialization["program"] = str((directory / main).resolve())
    return {"project": str(project) if project else None, "root": str(directory), "sdk": sdk,
        "properties": {key: properties.get(key, "") for key in ("config", "platform", "projectversion")},
        "units": units, "includes": includes, "resources": resources, "objects": objects, "defines": defines, "aliases": aliases,
        "main": str((directory / main).resolve()) if main else None, "initialization_options": initialization}


def diagnostics(project: dict, path: Path, state: Path, result: dict, timeout: float) -> dict:
    compiler = project["sdk"]["compiler"]
    sdk_root = Path(project["sdk"]["root"])
    output = state / "delphi-output"
    output.mkdir(parents=True, exist_ok=True)
    # Disable implicit DCC32.CFG so output flags cannot be overridden by cwd
    # configuration. Read supported project options explicitly; every compiler
    # output kind goes to this session's own runtime directory.
    command = [compiler, "--no-config", "-Q", "-B", *[prefix + str(output) for prefix in ("-E", "-N0", "-NH", "-NO", "-NB", "-LE", "-LN")],
        "-U" + ";".join([project["root"], *project["units"], str(sdk_root / "lib"), str(sdk_root / "lib/Obj")]),
        "-I" + ";".join([project["root"], *project["includes"]]),
        "-R" + ";".join([project["root"], *project["resources"]]),
        "-O" + ";".join([project["root"], *project["objects"], str(sdk_root / "lib/Obj")])]
    if project["defines"]:
        command.append("-D" + ";".join(project["defines"]))
    if project["aliases"]:
        command.append("-A" + project["aliases"])
    command.append(project.get("main") or str(path))
    completed = subprocess.run(command, capture_output=True, cwd=project["root"], timeout=timeout,
        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0, check=False)
    text = (completed.stdout + completed.stderr).decode("mbcs" if os.name == "nt" else "utf-8", errors="replace")
    items = []
    related = {}
    for line in text.splitlines():
        match = re.search(r"^(.+?)\((\d+)\)\s+(Error|Fatal|Warning|Hint):\s*([A-Z]\d+)\s+(.*)$", line)
        if not match:
            continue
        found = (Path(project["root"]) / match[1]).resolve()
        if match[4] in {"F1026", "F2051", "F2613"}:
            return {**result, "status": "unavailable", "reason": "Delphi project dependency is unavailable: " + line}
        start = {"line": max(0, int(match[2]) - 1), "character": 0}
        diagnostic = {"range": {"start": start, "end": start}, "severity": {"Error": 1, "Fatal": 1, "Warning": 2, "Hint": 4}[match[3]],
            "code": match[4], "source": "Delphi DCC32", "message": match[5]}
        if found == path.resolve():
            items.append(diagnostic)
        else:
            related.setdefault(found, []).append(diagnostic)
    if completed.returncode and not items and not related:
        return {**result, "status": "failed", "reason": "Delphi compiler did not complete: " + text[-2000:]}
    response = {**result, "status": "diagnostics" if items or related else "clean", "diagnostics": items,
        "freshness": "correlated-delphi-compiler", "diagnostic_provider": "Delphi DCC32", "project_inputs": project["properties"]}
    if related:
        from backend import digest
        workspace = Path(project.get("workspace", project["root"]))
        response["related_results"] = []
        for found, diagnostics_for_file in related.items():
            if not found.is_relative_to(workspace) or not found.is_file():
                return {**result, "status": "unavailable", "reason": "Delphi compiler diagnosed a dependency outside the applicable workspace: " + str(found)}
            response["related_results"].append({"file": found.relative_to(workspace).as_posix(), "revision": digest(found),
                "backend": "pascal", "status": "diagnostics", "diagnostics": diagnostics_for_file, "freshness": "correlated-delphi-compiler"})
    return response
