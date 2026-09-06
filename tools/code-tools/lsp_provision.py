"""Explicit missing-backend provisioning through shared existing runtime managers.

The caller supplies the lifecycle module so locks and transaction identifiers have
one owner. Runtime launchers never import this file or download packages.
"""
from __future__ import annotations

import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tomllib
import uuid


def run(command, timeout=180):
    result = subprocess.run(list(map(str, command)), capture_output=True, text=True, timeout=timeout,
                            creationflags=0x08000000 if os.name == "nt" else 0)
    if result.returncode:
        raise RuntimeError("Explicit backend provisioning command failed: " + result.stderr[-2000:])
    return result.stdout.strip()


def stage_release(identifier, user_home, state_dir, version=None, *, lifecycle):
    d = lifecycle
    current = d.discovery.Discovery(user_home)
    contracts = {
        "cpp": ("clangd/clangd", lambda v: "clangd-windows-" + v + ".zip", "clangd.exe"),
        "powershell": ("PowerShell/PowerShellEditorServices", lambda _v: "PowerShellEditorServices.zip", "Start-EditorServices.ps1"),
        "delphi": ("zen010101/pascal-language-server", lambda _v: "pasls-x86_64-win64.zip", "pasls.exe"),
    }
    if identifier not in contracts or os.name != "nt":
        raise ValueError("No declared Windows release contract for this backend")
    repository, asset_name, filename = contracts[identifier]
    url = "https://api.github.com/repos/" + repository + "/releases/" + ("tags/" + version if version else "latest")
    release = d.fetch_json(url)
    selected = release["tag_name"]
    if release.get("prerelease") or not d.stable_version(selected):
        raise ValueError("Release does not identify a stable backend")
    asset = next(a for a in release["assets"] if a["name"] == asset_name(selected))
    raw = d.fetch(asset["browser_download_url"])
    digest = asset.get("digest", "")
    if digest != "sha256:" + hashlib.sha256(raw).hexdigest():
        raise ValueError("Official backend archive digest absent or mismatched")
    stage = Path(state_dir) / "staging" / (identifier + "-missing-" + uuid.uuid4().hex)
    candidate = stage / "candidate"
    d.safe_extract_zip(raw, candidate)
    entries = list(candidate.glob("**/" + filename))
    if len(entries) != 1:
        raise ValueError("Backend archive entrypoint is missing or ambiguous")
    entry = entries[0]
    evidence = {"archive_integrity": "matched"}
    if identifier == "cpp":
        evidence["version_output"] = run([entry, "--version"], 20)
        if selected not in evidence["version_output"]:
            raise ValueError("clangd executable version differs from release")
        candidate = entry.parent.parent
        destination = current.home / ".cache/opencode/bin" / ("clangd_" + selected)
    elif identifier == "powershell":
        if not current.pwsh:
            raise RuntimeError("Existing PowerShell runtime is required")
        destination = current.serena / "PowerShellLanguageServer" / ("powershell-" + selected)
        analyzers = list(candidate.glob("**/PSScriptAnalyzer.psd1"))
        if len(analyzers) != 1:
            raise ValueError("PSES bundle lacks an unambiguous bundled PSScriptAnalyzer")
        evidence["analyzer_relative_path"] = str(analyzers[0].relative_to(candidate))
    else:
        # Keep runtime prerequisites beside the server and outside its replaceable directory.
        destination = current.serena / "PascalLanguageServer" / ("pasls-" + selected)
    relative = entry.relative_to(candidate)
    result = {"schema_version": 1, "id": identifier, "state": "staged", "version": selected,
              "candidate": str(candidate), "destination": str(destination), "entrypoint_relative": str(relative),
              "source": asset["browser_download_url"], "digest": digest, "evidence": evidence}
    d.atomic_json(stage / "stage.json", result)
    result["stage_manifest"] = str(stage / "stage.json")
    return result


def provision_typescript(current, state_dir, version, *, lifecycle):
    d = lifecycle
    if not current.node:
        return {"state": "prerequisite-missing", "id": "typescript", "reason": "An existing Node.js/npm runtime is required; no duplicate Node installation."}
    npm = Path(current.node).parent / "node_modules/npm/bin/npm-cli.js"
    if not npm.is_file():
        return {"state": "prerequisite-missing", "id": "typescript", "reason": "The selected Node runtime has no existing npm CLI."}
    metadata = d.fetch_json("https://registry.npmjs.org/typescript-language-server/" + (version or "latest"))
    selected = metadata["version"]
    if metadata.get("name") != "typescript-language-server" or not d.stable_version(selected):
        raise ValueError("Unexpected TypeScript server metadata")
    # This shared server runtime pair passed actual TS+JS diagnostics/navigation;
    # project TypeScript versions and lockfiles remain separately selected by tsserver.
    compiler = "5.9.3"
    destination = current.serena / "TypeScriptLanguageServer/ts-lsp"
    with d.installation_lock(state_dir, destination):
        if destination.exists():
            raise RuntimeError("Unknown existing TypeScript prefix must be discovered/repaired, not overwritten")
        candidate = Path(state_dir) / "staging" / ("typescript-new-" + uuid.uuid4().hex)
        candidate.mkdir(parents=True)
        run([current.node, npm, "install", "--prefix", candidate, "--no-audit", "--no-fund", "--ignore-scripts", "--save-exact",
             "typescript-language-server@" + selected, "typescript@" + compiler])
        record = d.discovery.npm_candidate("typescript-language-server", candidate / "node_modules", current.node, "typescript-language-server", "serena-cache")
        if not record or record["status"] != "adopted":
            raise RuntimeError("npm did not create the selected language-server entrypoint")
        if d.discovery.read_json(candidate / "node_modules/typescript/package.json").get("version") != compiler:
            raise RuntimeError("Unexpected shared TypeScript compiler runtime version")
        destination.parent.mkdir(parents=True, exist_ok=True)
        journal = d.activate_new_directory(candidate, destination, state_dir, "typescript")
    return {"state": "installed-unverified", "id": "typescript", "version": selected, "installation": str(destination),
            "shared_compiler_version": compiler, "transaction_journal": journal,
            "remaining": "Actual TS and JS project diagnostics, clearance and navigation."}


def provision_rust(current, state_dir, *, lifecycle):
    d = lifecycle
    settings = current.rustup / "settings.toml"
    rustup = current.find_command("rustup")
    if not rustup or not settings.is_file():
        return {"state": "prerequisite-missing", "id": "rust", "reason": "An existing rustup manager and installed selected toolchain are required; no toolchain will be installed or changed."}
    selected = tomllib.loads(settings.read_text(encoding="utf-8"))["default_toolchain"]
    if not isinstance(selected, str) or any(c not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.-" for c in selected):
        raise ValueError("Invalid selected rustup toolchain")
    installation = current.rustup / "toolchains" / selected
    manifest = installation / "lib/rustlib/multirust-channel-manifest.toml"
    components = installation / "lib/rustlib/components"
    executable = installation / "bin/rust-analyzer.exe"
    if not manifest.is_file() or not components.is_file():
        return {"state": "prerequisite-missing", "id": "rust", "reason": "Selected installed toolchain lacks its rustup receipt; do not invoke a toolchain download."}
    if executable.exists():
        return {"state": "pending", "id": "rust", "reason": "An existing analyzer needs explicit identity repair; do not overwrite it."}
    data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    target = selected.removeprefix("stable-").removeprefix("beta-")
    if target not in data["pkg"]["rust-analyzer-preview"]["target"]:
        target = next((name for name in data["pkg"]["rust-analyzer-preview"]["target"] if selected.endswith(name)), None)
    package = data["pkg"]["rust-analyzer-preview"]["target"].get(target, {})
    if not package.get("available") or not package.get("xz_url") or not package.get("xz_hash"):
        return {"state": "prerequisite-missing", "id": "rust", "reason": "This installed compiler cohort has no available declared analyzer component."}
    raw = d.fetch(package["xz_url"])
    if hashlib.sha256(raw).hexdigest() != package["xz_hash"]:
        raise ValueError("Installed-cohort Rust analyzer archive checksum mismatch")
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:*") as archive:
        files = [entry for entry in archive if entry.isfile() and Path(entry.name).name == "rust-analyzer.exe"]
        if len(files) != 1:
            raise ValueError("Rust component executable is missing or ambiguous")
        executable_hash = hashlib.sha256(archive.extractfile(files[0]).read()).hexdigest()
    with d.installation_lock(state_dir, installation):
        probes = [{"installation_root": str(installation)}]
        current.inspect_consumers(probes)
        if probes[0]["active_consumers"]["state"] != "observed" or probes[0]["active_consumers"]["processes"]:
            return {"state": "pending", "id": "rust", "reason": "Existing toolchain consumers prevent component modification; no process stopped."}
        before_components = components.read_text(encoding="utf-8").splitlines()
        component = "rust-analyzer-preview-" + target
        if component in before_components or executable.exists():
            raise RuntimeError("Analyzer component state changed before provisioning")
        journal_path = d.transaction_path(state_dir, "rust-component")
        journal = {"schema_version": 1, "owner": "codex-harness-dependencies", "kind": "rustup-component-install", "phase": "prepared",
                   "installation": str(installation), "rustup": str(rustup), "toolchain": selected, "component": component,
                   "executable": str(executable), "executable_sha256": executable_hash, "components_before": before_components,
                   "manifest_sha256": d.discovery.fingerprint(manifest), "transaction_id": d.TRANSACTION_ID}
        d.atomic_json(journal_path, journal)
        run([rustup, "component", "add", "rust-analyzer", "--toolchain", selected])
        if d.discovery.fingerprint(executable) != executable_hash or d.discovery.fingerprint(manifest) != journal["manifest_sha256"]:
            raise RuntimeError("Rust component differs from the existing cohort artifact; retain recovery journal")
        if set(components.read_text(encoding="utf-8").splitlines()) != set(before_components) | {component}:
            raise RuntimeError("Unexpected Rust component receipt change; retain recovery journal")
        journal["phase"] = "committed"
        d.atomic_json(journal_path, journal)
    return {"state": "installed-unverified", "id": "rust", "version": data["pkg"]["rust"]["version"], "transaction_journal": str(journal_path),
            "installation": str(installation), "source": package["xz_url"], "sha256": package["xz_hash"],
            "remaining": "Actual Rust project diagnostic, clearance and navigation; compiler/toolchain version was preserved."}


def recover_rust(journal, path, *, lifecycle):
    d = lifecycle
    installation = Path(journal["installation"])
    manifest = installation / "lib/rustlib/multirust-channel-manifest.toml"
    components = installation / "lib/rustlib/components"
    executable = Path(journal["executable"])
    expected_components = set(journal["components_before"])
    current_components = set(components.read_text(encoding="utf-8").splitlines())
    if not executable.exists() and current_components == expected_components:
        journal["phase"] = "restored"
        d.atomic_json(path, journal)
        return {"state": "restored", "journal": str(path)}
    if (d.discovery.fingerprint(manifest) != journal["manifest_sha256"] or not executable.is_file()
            or d.discovery.fingerprint(executable) != journal["executable_sha256"]
            or current_components != expected_components | {journal["component"]}):
        return {"state": "pending", "journal": str(path), "reason": "Installed Rust cohort/component changed; preserve it."}
    run([journal["rustup"], "component", "remove", "rust-analyzer", "--toolchain", journal["toolchain"]])
    if executable.exists() or set(components.read_text(encoding="utf-8").splitlines()) != expected_components:
        raise RuntimeError("rustup did not restore the exact prior component set")
    journal["phase"] = "restored"
    d.atomic_json(path, journal)
    return {"state": "restored", "journal": str(path)}


def provision(identifier, user_home, state_dir, version=None, *, lifecycle):
    d = lifecycle
    identifier = "typescript" if identifier == "javascript" else identifier
    current = d.discovery.Discovery(user_home, processes=True, probe_versions=True)
    record = next(x for x in current.run()["languages"] if x["id"] == identifier)
    if record["status"] == "adopted":
        return {"state": "reused", "id": identifier, "version": record["version"]}
    if record["status"] != "missing":
        return {"state": "pending", "id": identifier, "reason": "Existing backend must be audited/repaired; a second installation would hide it."}
    if identifier == "typescript":
        return provision_typescript(current, state_dir, version, lifecycle=d)
    if identifier == "rust":
        return provision_rust(current, state_dir, lifecycle=d)
    staged = stage_release(identifier, user_home, state_dir, version, lifecycle=d)
    candidate, destination = Path(staged["candidate"]), Path(staged["destination"])
    entry = destination / staged["entrypoint_relative"]
    with d.installation_lock(state_dir, destination):
        destination.parent.mkdir(parents=True, exist_ok=True)
        journal = d.activate_new_directory(candidate, destination, state_dir, identifier)
    result = {"state": "installed-unverified", "id": identifier, "version": staged["version"], "source": staged["source"],
              "digest": staged["digest"], "installation": str(destination), "transaction_journal": journal,
              "record": d.discovery.native_candidate(entry, "shared-cache", staged["version"]),
              "remaining": "Real backend initialization, project navigation, diagnostics and clearance."}
    d.record_native_provisioning(result)
    if identifier == "delphi":
        result["prerequisite"] = d.provision_fpc(user_home, state_dir)
    return result
