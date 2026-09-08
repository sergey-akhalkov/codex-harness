"""Explicit paired native outcomes; case preparation/oracles remain separately owned.

python tools/outcome_suite.py --inputs PRIVATE/inputs.json --cases focused second
    --run-model-probes --repetitions 2
Use --discovery-only instead for bounded native configuration checks without
model calls. No flags means no work. Detailed evidence stays in a new temp root.
"""
from __future__ import annotations

import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import time
import tomllib
from typing import Any

REPO = Path(__file__).resolve().parents[1]


def module(name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, REPO / "tools" / (name + ".py"))
    if spec is None or spec.loader is None:
        raise RuntimeError("Missing outcome module: " + name)
    loaded = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(loaded)
    return loaded


runner = module("outcome_runner")
reporter = module("outcome_report")


def signal_pattern(case_id: str) -> str:
    """Conservative executed-command filter; unknown forms await the oracle."""
    start = r"(?:^|(?:-Command)\s+[\"']?|[;&|\n])\s*(?:&\s+)?[\"']?"
    python = r"(?:[^\s\"']*[\\/])?python(?:\d+(?:\.\d+)*)?(?:\.exe)?[\"']?\s+(?:-[Bu]\s+)*"
    node = r"(?:[^\s\"']*[\\/])?node(?:\.exe)?[\"']?\s+"
    focused_script = r"(?:[^\s\"']*[\\/])?acceptance[/\\]+focused-check\.ps1(?:[\"']|\s|$)"
    pwsh = r"(?:[^\s\"']*[\\/])?(?:pwsh|powershell)(?:\.exe)?[\"']?\s+(?:(?:-NoLogo|-NoProfile|-NonInteractive)\s+)*-File\s+[\"']?"
    commands = {
        "focused": r"(?:npm(?:\.cmd)?\s+run\s+test:focused:library\b|" + node + r"tools[/\\]+run-focused-test\.ts\s+tools[/\\]+test-library\.ts\b|" + pwsh + focused_script + "|" + focused_script + ")",
        "second": r"(?:npm(?:\.cmd)?\s+run\s+lint\b|(?:npx\s+)?eslint\s)",
        "freshness": python + r"(?:\.\s*[/\\])?(?:build|cli)\.py\b",
        "entrypoint": python + r"(?:\.\s*[/\\])?(?:build|cli)\.py\b",
        "reduction": node + r"(?:tools[/\\]run-focused-test\.ts\s+)?tools[/\\]outcome-(?:original|minimal|wrong)\.mjs\b",
        "process": python + r"(?:\.\s*[/\\])?(?:process_target|check_process)\.py\b",
        "missing": r"(?:Test-Path\s+[^;\n]*unavailable-checker\.exe|" + python + r"-c\s+[^\n]*unavailable-checker\.exe[^\n]*exists\s*\()",
        "negative": r"(?:Test-Path\s+[^;\n]*guide\.md|" + python + r"-c\s+[^\n]*guide\.md[^\n]*exists\s*\()",
    }
    return start + commands.get(case_id, r"(?!)")


def digest(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def identity(value: Any) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":")).encode()).hexdigest()


def normalize(value: Any, roots: dict[str, str]) -> Any:
    """Replace only exact owned path prefixes, including hook-state path keys."""
    if isinstance(value, dict):
        pairs = [(normalize(k, roots), normalize(v, roots)) for k, v in value.items()]
        if len({k for k, _ in pairs}) != len(pairs):
            raise ValueError("Owned-root normalization collided")
        return dict(pairs)
    if isinstance(value, list):
        return [normalize(v, roots) for v in value]
    if isinstance(value, str):
        for root, label in sorted(roots.items(), key=lambda item: -len(item[0])):
            variants = {root.rstrip("\\/"), root.replace("\\", "/").rstrip("/")}
            for variant in sorted(variants, key=len, reverse=True):
                value = re.sub(r"(?<![\w])" + re.escape(variant) + r"(?=$|[\\/:])", lambda _: label,
                               value, flags=re.IGNORECASE if os.name == "nt" else 0)
        return value
    return value


def without_treatment(config: dict[str, Any], treatment_paths: set[str]) -> dict[str, Any]:
    """Retain every non-treatment setting, including other disabled skills."""
    result = copy.deepcopy(config)
    skills = result.get("skills")
    if isinstance(skills, dict) and isinstance(skills.get("config"), list):
        skills["config"] = [s for s in skills["config"] if os.path.normcase(str(Path(s["path"]).absolute())) not in treatment_paths]
    return result


def reject_credentials(value: Any, key: str = "") -> None:
    # Credential references are fine; inline secret values cannot be copied.
    if isinstance(value, dict):
        for name, item in value.items():
            if re.search(r"(^|_)(api_key|access_token|refresh_token|password|bearer_token|secret)$", name, re.I) and item:
                raise ValueError("Inline credentials require an existing external credential reference")
            reject_credentials(item, name)
    elif isinstance(value, list):
        for item in value:
            reject_credentials(item, key)


def dump_toml(config: dict[str, Any]) -> str:
    """Emit native tables, retaining arrays of tables that skills.config extends."""
    lines: list[str] = []

    def emit(table: dict[str, Any], prefix: list[str]) -> None:
        for key, value in table.items():
            if not isinstance(value, dict) and not (isinstance(value, list) and value and isinstance(value[0], dict)):
                lines.append(json.dumps(key) + " = " + runner.toml_value(value))
        for key, value in table.items():
            path = prefix + [key]
            header = ".".join(json.dumps(p) for p in path)
            if isinstance(value, dict):
                lines.extend(["", "[" + header + "]"])
                emit(value, path)
            elif isinstance(value, list) and value and isinstance(value[0], dict):
                for entry in value:
                    lines.extend(["", "[[" + header + "]]"])
                    emit(entry, path)
    emit(config, [])
    text = "\n".join(lines) + "\n"
    if tomllib.loads(text) != config:
        raise ValueError("Configuration serialization changed semantics")
    return text


def seed_frozen_kit(inputs: Path) -> Path:
    """Copy reusable sources once and overlay the recorded accepted hook kit."""
    inputs = inputs.resolve()
    inputs = inputs / "inputs.json" if inputs.is_dir() else inputs
    expected = runner.read_json(inputs).get("hook-source", {})
    snapshot = inputs.parent / "hook-comparison/after/source"
    if not expected or not (snapshot / "tools/lsp").is_dir():
        raise ValueError("Accepted hook snapshot unavailable")
    root = runner.private_root("codex-outcome-frozen-kit-")
    kit = root / "kit"
    kit.mkdir()
    files = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=REPO).decode().split("\0")
    for name in sorted(set(files)):
        if not name or name.startswith(".serena/") or "__pycache__" in Path(name).parts:
            continue
        source = REPO / name
        if not source.is_file():
            continue
        target = kit / name
        target.parent.mkdir(parents=True, exist_ok=True)
        before = digest(source)
        shutil.copyfile(source, target)
        if digest(source) != before or digest(target) != before:
            raise ValueError("Source changed while seeding: " + name)
    shutil.copytree(snapshot / "tools/lsp", kit / "tools/lsp", dirs_exist_ok=True)
    for name, sha in expected.items():
        source = snapshot / name
        if not source.is_file():
            source = REPO / name
        if not source.is_file() or digest(source) != sha:
            raise ValueError("Accepted snapshot hash mismatch: " + name)
        target = kit / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
        if digest(target) != sha:
            raise ValueError("Accepted snapshot changed during copy: " + name)
    manifest = {"source_root": str(REPO), "inputs": str(inputs), "inputs_sha256": digest(inputs),
                "files": {str(p.relative_to(kit)): digest(p) for p in kit.rglob("*") if p.is_file()}}
    runner.write_json(kit / ".outcome-seed.json", manifest)
    return kit


def source_origin() -> Path:
    seed = REPO / ".outcome-seed.json"
    if not seed.is_file():
        return REPO
    record = runner.read_json(seed)
    for name, sha in record["files"].items():
        if not (REPO / name).is_file() or digest(REPO / name) != sha:
            raise ValueError("Frozen kit was modified: " + name)
    return Path(record["source_root"]).resolve()


def prepare_home(base_home: Path, attempt_root: Path, workspace: Path) -> Path:
    """Install only into owned temp roots; borrow auth/tools via non-owning links."""
    user = attempt_root / "user"
    home = user / ".codex"
    state = runner.read_json(base_home / "harness/installation.json")
    origin = source_origin()
    if Path(state["sourceRoot"]).resolve() != origin:
        raise ValueError("Base installation belongs to another source kit")
    base = tomllib.loads((base_home / "config.toml").read_text(encoding="utf-8-sig"))
    reject_credentials(base)
    catalog = Path(base["model_catalog_json"]) if base.get("model_catalog_json") else None
    profile = tomllib.loads((base_home / "harness.config.toml").read_text(encoding="utf-8-sig"))
    if profile.get("model", base.get("model")) != runner.MODEL or profile.get("model_reasoning_effort", base.get("model_reasoning_effort")) != runner.EFFORT:
        raise ValueError("Existing native policy must already be Astra xhigh")
    # Move only owned source/runtime prefixes; retain provider, trust and tools.
    for server in base.get("mcp_servers", {}).values():
        environment = server.get("env", {})
        if "CODEX_HOME" in environment:
            if Path(environment["CODEX_HOME"]).resolve() != base_home:
                raise ValueError("MCP uses another CODEX_HOME; isolation must be explicit")
    base = normalize(base, {str(base_home): str(home), str(origin): str(REPO)})
    base.setdefault("projects", {})[str(workspace)] = {"trust_level": "trusted"}
    skill_settings = base.get("skills", {})
    if "config" in skill_settings:
        skill_settings["config"] = [entry for entry in skill_settings["config"]
            if Path(entry["path"]).name != "SKILL.md" or Path(entry["path"]).parent.name not in runner.CANDIDATES]
        if not skill_settings["config"]:
            del skill_settings["config"]  # Native empty default; allow [[skills.config]] treatment.
    pwsh = shutil.which("pwsh")
    if not pwsh:
        raise ValueError("PowerShell unavailable")
    with (attempt_root / "installation.log").open("w", encoding="utf-8") as log:
        subprocess.run([pwsh, "-NoLogo", "-NoProfile", "-File", str(REPO / "install.ps1"), "-Mode", "Install",
            "-CoreOnly", "-PathScope", "Process", "-CodexHome", str(home), "-UserHome", str(user),
            "-CodexCommand", state["codexCommand"]], stdout=log, stderr=subprocess.STDOUT,
            check=True, timeout=90, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
    config = home / "config.toml"
    if config.is_symlink():
        raise ValueError("Owned config must be a regular file")
    config.write_text(dump_toml(base), encoding="utf-8")
    (home / "skills").mkdir(exist_ok=True)
    for registration in (user / ".agents/skills").iterdir():
        if not registration.name.startswith("."):
            (home / "skills" / registration.name).symlink_to(registration, target_is_directory=True)
    for name in ("auth.json",):
        source = base_home / name
        target = home / name
        if not source.is_file() or target.exists():
            raise ValueError("Required non-owning runtime link unavailable: " + name)
        target.symlink_to(source)
    if catalog is not None:
        if not catalog.is_file() or not catalog.is_relative_to(base_home):
            raise ValueError("Native model catalog must remain an existing owned-home reference")
        target = home / catalog.relative_to(base_home)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.symlink_to(catalog)
    registry = runner.read_json(base_home / "harness/code-tools.json")
    reject_credentials(registry)
    registry = normalize(registry, {str(base_home): str(home), str(origin): str(REPO)})
    runner.write_json(home / "harness/code-tools.json", registry)
    for relative, source in (("hooks.json", REPO / "global/hooks.json"),
                             ("harness/bin/hook.ps1", REPO / "tools/hook.ps1")):
        target = home / relative
        if not target.exists():
            target.symlink_to(source)
    source_hooks = base_home / "hooks.json"
    if not source_hooks.is_file() and (REPO / ".outcome-seed.json").is_file():
        source_hooks = REPO / "global/hooks.json"
    target_hooks = home / "hooks.json"
    if not target_hooks.exists():
        target_hooks.symlink_to(source_hooks)
    if digest(target_hooks) != digest(source_hooks):
        raise ValueError("Isolated hooks differ from the existing accepted consumer")
    if digest(home / "harness.config.toml") != digest(base_home / "harness.config.toml") or digest(home / "AGENTS.md") != digest(base_home / "AGENTS.md"):
        raise ValueError("Seeded profile/instructions differ from the existing native configuration")
    return home


def freeze(inputs: Path, base_home: Path, require_oracle: bool = True) -> dict[str, Any]:
    source_origin()
    records = runner.read_json(inputs)
    expected = records.get("hook-source", {})
    if not expected:
        raise ValueError("Frozen accepted hook identity is missing")
    drift = [name for name, sha in expected.items() if digest(REPO / name) != sha]
    if drift:
        raise ValueError("Frozen accepted hook source drift: " + ", ".join(drift))
    paths = [REPO / name for name in expected]
    treatment_files = {str(p.relative_to(REPO)): digest(p) for name in runner.CANDIDATES
                       for p in (REPO / ".agents/skills" / name).rglob("*")
                       if p.is_file() and "__pycache__" not in p.parts}
    paths += [REPO / name for name in treatment_files]
    paths += [p for directory in ("tools/lsp", "tools/code-tools", "global/agents")
              for p in (REPO / directory).rglob("*") if p.is_file() and p.suffix in (".py", ".ps1", ".js", ".cjs", ".json", ".toml")]
    paths += [REPO / p for p in ("tools/mcp.ps1", "global/code-tools.json", "tools/outcome_cases.py",
                                "tools/outcome_runner.py", "tools/outcome_report.py", "tools/outcome_suite.py")]
    if require_oracle:
        paths.append(REPO / "tools/outcome_oracles.py")
    seed = REPO / ".outcome-seed.json"
    if seed.is_file():
        manifest = runner.read_json(seed)
        if manifest["inputs_sha256"] != digest(inputs):
            raise ValueError("Inputs changed since frozen kit seed")
        paths += [REPO / name for name in manifest["files"]]
        paths.append(seed)
    preparation_paths = [base_home / p for p in ("config.toml", "harness.config.toml", "AGENTS.md", "harness/code-tools.json", "harness/installation.json")]
    if (base_home / "hooks.json").is_file():
        preparation_paths.append(base_home / "hooks.json")
    elif not seed.is_file():
        raise ValueError("Base hooks disconnected; an explicit accepted frozen kit is required")
    base = tomllib.loads((base_home / "config.toml").read_text(encoding="utf-8-sig"))
    if base.get("model_catalog_json"):
        paths.append(Path(base["model_catalog_json"]))
    paths += [inputs, runner.native_executable(base_home), Path(sys.executable)]
    node = shutil.which("node")
    if not node:
        raise ValueError("Recorded Node runtime unavailable")
    version = subprocess.check_output([node, "--version"], text=True, timeout=10).strip()
    if version != records.get("environment", {}).get("node") or os.name != records.get("environment", {}).get("os"):
        raise ValueError("Frozen runtime does not match this host")
    paths.append(Path(node))
    return {"files": {str(p): digest(p) for p in sorted(set(paths))}, "treatment_files": treatment_files,
        "preparation_files": {str(p): digest(p) for p in preparation_paths},
        "hook_connection": "accepted frozen snapshot installed in both arms" if seed.is_file() else "existing consumer",
        "base_hooks_connected": (base_home / "hooks.json").is_file(), "runtime": {
        "os": os.name, "platform": platform.platform(), "python": sys.version, "node": version,
        "native": subprocess.check_output([str(runner.native_executable(base_home)), "--version"], text=True, timeout=10).strip()},
        "role_configuration": {str(p.relative_to(REPO)): digest(p) for p in (REPO / "global/agents").rglob("*.toml")}}


def check_frozen(frozen: dict[str, Any], *, preparation: bool = False) -> list[str]:
    files = {**frozen["files"], **(frozen.get("preparation_files", {}) if preparation else {})}
    return ["identity_drift:" + str(Path(path).name) for path, sha in files.items()
            if not Path(path).is_file() or digest(Path(path)) != sha]


def observe_files(paths: list[Path]) -> dict[str, Any]:
    """Sample explicit non-secret input files, retaining missing/unreadable state."""
    observed: dict[str, Any] = {"started_at": time.time(), "files": {}, "targets": {}, "errors": {}}
    for path in paths:
        key = str(path)
        try:
            observed["targets"][key] = str(path.resolve(strict=True))
            observed["files"][key] = digest(path)
        except OSError as error:
            observed["files"][key] = None
            observed["errors"][key] = type(error).__name__
    observed["ended_at"] = time.time()
    return observed


def execution_inputs(home: Path) -> dict[str, Any]:
    """Pin files consumed by this installation; auth remains an unread borrowed link."""
    paths = [home / name for name in ("config.toml", "harness.config.toml", "AGENTS.md", "hooks.json",
        "harness/code-tools.json", "harness/installation.json", "harness/bin/codex.ps1", "harness/bin/hook.ps1")]
    for name in ("config.toml", "harness.config.toml"):
        config = tomllib.loads((home / name).read_text(encoding="utf-8-sig"))
        reject_credentials(config)
        references = [config["model_catalog_json"]] if config.get("model_catalog_json") else []
        references += [role["config_file"] for role in config.get("agents", {}).values()
                       if isinstance(role, dict) and role.get("config_file")]
        paths.extend(Path(p) if Path(p).is_absolute() else home / p for p in references)
    snapshot = observe_files(sorted(set(paths)))
    if snapshot["errors"]:
        raise ValueError("Consumed configuration is missing or unreadable: " + ", ".join(snapshot["errors"]))
    return snapshot


def observe_execution(row: dict[str, Any], stage: str) -> list[str]:
    expected = row["execution_inputs"]
    observed = observe_files([Path(p) for p in expected["files"]])
    changed = [path for path, sha in expected["files"].items()
               if observed["files"].get(path) != sha or observed["targets"].get(path) != expected["targets"].get(path)]
    observed["changed_paths"] = changed
    row.setdefault("execution_observations", {})[stage] = observed
    return ["identity_drift:" + Path(path).name for path in changed]


def calibration_record(path: Path, source_state: str) -> dict[str, Any]:
    record = runner.read_json(path)
    evidence = Path(record.get("evidence", ""))
    if not evidence.is_absolute():
        evidence = path.parent / evidence
    if record.get("passed") is not True or type(record.get("exit_code")) is not int or record["exit_code"] != 0:
        raise ValueError("Focused calibration has no actual passing execution")
    if record.get("source_state") != source_state or not evidence.is_file():
        raise ValueError("Focused calibration source/log does not match the frozen case")
    return {"record": str(path), "record_sha256": digest(path), "evidence": str(evidence), "evidence_sha256": digest(evidence)}


def dependency_identity(workspace: Path) -> str:
    dependencies = workspace / "node_modules"
    return identity({str(path.relative_to(dependencies)): digest(path) for path in dependencies.rglob("*") if path.is_file()})


def pin_skills(discovery: dict[str, Any]) -> dict[str, str]:
    return {str(p): digest(p) for skill in discovery["skills"] for p in Path(skill["path"]).parent.rglob("*")
            if p.is_file() and "__pycache__" not in p.parts}


def matched_fields(case_id: str, setup: dict[str, Any], workspace: Path, home: Path,
                   discovery: dict[str, Any], frozen: dict[str, Any], inputs: Path) -> dict[str, Any]:
    roots = {str(home): "$CODEX_HOME", str(home.parent): "$USER_HOME", str(workspace): "$WORKSPACE"}
    treatment_paths = {os.path.normcase(str(Path(s["path"]).absolute())) for s in discovery["skills"] if s["name"] in runner.CANDIDATES}
    base = tomllib.loads((home / "config.toml").read_text(encoding="utf-8-sig"))
    profile = tomllib.loads((home / "harness.config.toml").read_text(encoding="utf-8-sig"))
    actual = discovery.get("config", {}).get("config")
    if not isinstance(actual, dict):
        raise ValueError("Native effective base config unavailable")
    configs = {"base": without_treatment(base, treatment_paths), "profile": profile,
               "native_base": without_treatment(actual, treatment_paths)}
    others = sorted((s["name"], s["scope"], s["enabled"], digest(Path(s["path"])))
                    for s in discovery["skills"] if s["name"] not in runner.CANDIDATES)
    # Include every initial file, including stale generated state/history, in
    # addition to the consumer's immutable source manifest.
    initial = {}
    for path in workspace.rglob("*"):
        if path.is_file() and "node_modules" not in path.parts:
            data = path.read_bytes()
            try:
                initial[str(path.relative_to(workspace))] = identity(normalize(data.decode("utf-8"), roots))
            except UnicodeDecodeError:
                initial[str(path.relative_to(workspace))] = hashlib.sha256(data).hexdigest()
    provider_keys = ("model_provider", "model_providers", "openai_base_url", "experimental_realtime_ws_base_url", "service_tier")
    provider = {key: profile.get(key, base.get(key)) for key in provider_keys}
    provider["model_provider"] = provider["model_provider"] or "openai"
    return {
        "case_revision": frozen["files"][str(REPO / "tools/outcome_cases.py")], "source_state": setup["source_state"],
        "input_identity": identity({"inputs": digest(inputs), "initial": initial, "prompt": setup["prompt"]}),
        "runtime": frozen["runtime"], "model": runner.MODEL, "effort": runner.EFFORT, "provider": identity(provider),
        "config_identity": identity(normalize(configs, roots)), "tool_identity": identity({
            "registry": identity(normalize(runner.read_json(home / "harness/code-tools.json"), roots)), "mcp": normalize(base.get("mcp_servers", {}), roots),
            "sources": frozen["files"], "roles": frozen["role_configuration"]}),
        "hook_revision": identity({name: sha for name, sha in frozen["files"].items() if "/lsp/" in name.replace("\\", "/") or name.endswith(("hook.ps1", "hooks.json"))}),
        "allowed_effects": "owned workspace/docs/generated artifacts/private runtime only; no live sources/services/credentials writes",
        "cache_policy": "warm dependencies/filesystem; provider cache uncontrolled; historical v1 record" if case_id == "freshness" else "fresh source/records; warm dependencies/filesystem; provider cache uncontrolled",
        "preparation_policy": "fresh immutable-source copy and owned dependencies; preparation/discovery spans reported separately from native-through-acceptance wall",
        "budget": {"native_seconds": 600, "memory_mib": 2048},
        "oracle_identity": frozen["files"].get(str(REPO / "tools/outcome_oracles.py"), "not_loaded_discovery_only"),
        "instructions_identity": identity({"global": digest(home / "AGENTS.md"), "profile": normalize(profile, roots),
                                            "project": {k: v for k, v in initial.items() if Path(k).name in ("AGENTS.md", "AGENTS.override.md")}}),
        "other_skills": others, "stop_conditions": "600s native job; retain natural error/timeout; independent oracle required; no automatic retry",
        "criterion": {"relative_time_reduction": .15, "absolute_seconds": 15, "repeats_per_arm": 2,
                      "correctness_loss_allowed": False, "alternative": "repeatable correct completion/material defect detection gain"},
        "role_configuration": frozen["role_configuration"],
        "dependency_identity": dependency_identity(workspace),
        "focused_calibration": setup.get("focused_calibration", "not_applicable"),
        "first_signal_policy": signal_pattern(case_id),
        "treatment_identity": frozen["treatment_files"],
        "discovered_skill_identity": sorted((skill["name"], skill["scope"], identity({
            str(p.relative_to(Path(skill["path"]).parent)): digest(p)
            for p in Path(skill["path"]).parent.rglob("*") if p.is_file() and "__pycache__" not in p.parts}))
            for skill in discovery["skills"]),
    }


def run_suite(inputs: Path, base_home: Path, cases: list[str], *, run_model_probes: bool = False,
              discovery_only: bool = False, repetitions: int = 1,
              focused_calibration: Path | None = None) -> dict[str, Any]:
    """Every attempted preparation/execution is journaled; never auto-retry."""
    if not run_model_probes and not discovery_only:
        return {"status": "skipped", "model_calls": 0}
    if run_model_probes and discovery_only:
        raise ValueError("Choose discovery-only or model probes")
    catalogue = module("outcome_cases")
    runner.select_cases({case: case for case in catalogue.CASE_IDS}, cases, True)
    if not 1 <= repetitions <= 20:
        raise ValueError("Repetitions must be between 1 and 20")
    inputs = inputs.resolve()
    if inputs.is_dir():
        inputs /= "inputs.json"
    base_home = base_home.resolve()
    root = runner.private_root("codex-outcome-suite-")
    result: dict[str, Any] = {"status": "running", "evidence_root": str(root), "model_calls": 0, "attempts": []}

    def save() -> None:
        runner.write_json(root / "suite.json", result)
        report = reporter.summarize_attempts(result["attempts"])
        runner.write_json(root / "report.json", report)
        (root / "report.md").write_text(reporter.concise_report(report), encoding="utf-8")
    save()
    try:
        frozen = freeze(inputs, base_home, require_oracle=run_model_probes)
        calibration = None
        if "focused" in cases and run_model_probes:
            if focused_calibration is None:
                raise ValueError("Focused model cases require --focused-calibration from an actual passing execution")
            source_state = runner.read_json(inputs)["opencode-kit"]["tree_sha256"]
            calibration = calibration_record(focused_calibration.resolve(), source_state)
            frozen["files"].update({calibration["record"]: calibration["record_sha256"], calibration["evidence"]: calibration["evidence_sha256"]})
        runner.write_json(root / "frozen.json", frozen)
        oracle = module("outcome_oracles") if run_model_probes else None
        for repeat in range(repetitions):
            for case_id in cases:
                pair = []
                order = ("baseline", "candidate") if repeat % 2 == 0 else ("candidate", "baseline")
                for arm in order:
                    attempt_id = f"{case_id}-{repeat + 1}-{arm}"
                    folder = root / attempt_id
                    folder.mkdir()
                    workspace = folder / "workspace"
                    row: dict[str, Any] = {"attempt_id": attempt_id, "case_id": case_id, "arm": arm, "repeat": repeat + 1,
                        "status": "incomplete", "stage": "preparing", "started_at": time.time(), "ended_at": None,
                        "native_runs": [], "checks": [], "children": [], "interventions": [], "retry_of": None,
                        "excluded_reasons": [], "workspace": str(workspace), "evidence_root": str(folder), "matched": {}}
                    result["attempts"].append(row)
                    save()
                    try:
                        if check_frozen(frozen, preparation=True):
                            raise ValueError("Frozen source/configuration changed before preparation")
                        setup = catalogue.case_workspace(inputs.parent, workspace, case_id)
                        setup["arm"] = arm
                        if case_id == "focused" and calibration:
                            setup.update(focused_calibration_passed=True, focused_calibration=calibration)
                        runner.write_json(folder / "setup.json", setup)
                        home = prepare_home(base_home, folder, workspace)
                        discovery = runner.configure_arm(workspace, home, arm)
                        row["skill_files"] = pin_skills(discovery)
                        row["execution_inputs"] = execution_inputs(home)
                        row.update(discovery_verified=True, discovery=discovery, codex_home=str(home), stage="prepared")
                        row["matched"] = matched_fields(case_id, setup, workspace, home, discovery, frozen, inputs)
                        drift = check_frozen(frozen, preparation=True) + observe_execution(row, "after_preparation")
                        if drift:
                            row["excluded_reasons"].extend(drift)
                            raise ValueError("Input drift during isolated preparation")
                        pair.append((row, setup, home))
                    except Exception as error:
                        row.update(status="blocked", error_type=type(error).__name__)
                        (folder / "failure.txt").write_text(str(error), encoding="utf-8")
                        row["excluded_reasons"].append("preparation_failed")
                    finally:
                        row["ended_at"] = time.time()
                        row["preparation"] = {"started_at": row["started_at"], "ended_at": row["ended_at"]}
                        save()
                mismatches = reporter.comparison_reasons(pair[0][0], pair[1][0]) if len(pair) == 2 else ["pair_preparation_incomplete"]
                if mismatches:
                    for row, _, _ in pair:
                        row["excluded_reasons"].extend(mismatches)
                        row["status"] = "blocked"
                    save()
                    continue
                for row, setup, home in pair:
                    if discovery_only:
                        row["stage"] = "discovery_verified"
                        save()
                        continue
                    row.update(execution_started_at=time.time(), ended_at=None, stage="native")
                    save()
                    try:
                        drift = check_frozen(frozen) + check_frozen({"files": row["skill_files"]}) + observe_execution(row, "before_native")
                        if drift:
                            row["excluded_reasons"].extend(drift)
                            raise ValueError("Frozen configuration/runtime drift; no model call")
                        result["model_calls"] += 1
                        native = runner.run_native(Path(row["workspace"]), setup["prompt"], home, timeout=600,
                                                   useful_command_pattern=signal_pattern(case_id))
                        row["native_runs"].append(native)
                        row["excluded_reasons"].extend(observe_execution(row, "after_native"))
                        if native["status"] in ("failed", "timeout", "incomplete"):
                            row["status"] = native["status"]
                        row["stage"] = "acceptance"
                        save()
                        assert oracle is not None
                        evidence = Path(native["evidence_root"])
                        verdict = oracle.verify_case(case_id, Path(row["workspace"]), setup, native, evidence)
                        runner.write_json(Path(row["evidence_root"]) / "oracle.json", verdict)
                        row["checks"].append(verdict)
                        row["oracle"] = verdict
                        if dependency_identity(Path(row["workspace"])) != row["matched"].get("dependency_identity"):
                            row["excluded_reasons"].append("dependency_state_changed")
                    except Exception as error:
                        row.update(status="failed", error_type=type(error).__name__)
                        (Path(row["evidence_root"]) / "failure.txt").write_text(str(error), encoding="utf-8")
                    finally:
                        row["excluded_reasons"].extend(check_frozen(frozen) + check_frozen({"files": row["skill_files"]}) +
                                                       observe_execution(row, "after_attempt"))
                        row.update(ended_at=time.time(), stage="finished")
                        save()
        result["status"] = ("discovery_passed" if discovery_only and all(r.get("stage") == "discovery_verified" for r in result["attempts"])
                            else "accepted" if run_model_probes and all(reporter.finish_attempt(r)["correct"] and
                                not reporter.finish_attempt(r)["excluded_reasons"] for r in result["attempts"])
                            else "incomplete")
    except BaseException as error:
        result.update(status="blocked", error_type=type(error).__name__)
        (root / "failure.txt").write_text(str(error), encoding="utf-8")
        if isinstance(error, (KeyboardInterrupt, SystemExit)):
            raise
    finally:
        save()
    return result


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--run-model-probes", action="store_true")
    mode.add_argument("--discovery-only", action="store_true")
    mode.add_argument("--seed-frozen-kit", action="store_true")
    parser.add_argument("--inputs", type=Path)
    parser.add_argument("--codex-home", type=Path, default=Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))))
    parser.add_argument("--cases", nargs="+")
    parser.add_argument("--repetitions", type=int, default=1)
    parser.add_argument("--focused-calibration", type=Path)
    args = parser.parse_args(argv)
    if args.seed_frozen_kit:
        if not args.inputs:
            parser.error("--inputs is required to seed the accepted hook kit")
        kit = seed_frozen_kit(args.inputs)
        print(json.dumps({"status": "seeded", "model_calls": 0, "kit_source": str(kit), "cli": str(kit / "tools/outcome_suite.py")}))
        return 0
    if not args.run_model_probes and not args.discovery_only:
        print("SKIP: --run-model-probes or --discovery-only plus explicit --cases required")
        return 0
    if not args.inputs or not args.cases:
        parser.error("--inputs and an explicit --cases subset are required")
    result = run_suite(args.inputs, args.codex_home, args.cases, run_model_probes=args.run_model_probes,
                       discovery_only=args.discovery_only, repetitions=args.repetitions, focused_calibration=args.focused_calibration)
    print(json.dumps({k: result[k] for k in ("status", "model_calls", "evidence_root")}))
    return 0 if result["status"] in ("accepted", "discovery_passed") else 1


if __name__ == "__main__":
    raise SystemExit(main())
