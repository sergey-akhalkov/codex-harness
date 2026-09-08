"""Independent actual-entrypoint and external-regression oracles, no models."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
from typing import Any


def execute(argv, cwd, evidence, name, timeout=180) -> dict[str, Any]:
    evidence = Path(evidence)
    evidence.mkdir(parents=True, exist_ok=True)
    started = time.time()
    with (evidence / f"{name}.stdout").open("wb") as out, (evidence / f"{name}.stderr").open("wb") as err:
        try:
            result = subprocess.run(argv, cwd=cwd, stdout=out, stderr=err, timeout=timeout, check=False)
            status, code = "exited", result.returncode
        except subprocess.TimeoutExpired:
            status, code = "timeout", None
    record: dict[str, Any] = dict(argv=argv, cwd=str(cwd), started_at=started, ended_at=time.time(), status=status, exit_code=code)
    for stream in ("stdout", "stderr"):
        path = evidence / f"{name}.{stream}"
        record[stream] = dict(path=str(path), bytes=path.stat().st_size,
            sha256=hashlib.sha256(path.read_bytes()).hexdigest())
    (evidence / f"{name}.json").write_text(json.dumps(record, indent=2), encoding="utf-8")
    return record


def make_entrypoint(root):
    root.mkdir(parents=True, exist_ok=False)
    (root / "source.json").write_text('{"version":2}\n', encoding="utf-8")
    (root / "built.json").write_text('{"version":1}\n', encoding="utf-8")
    (root / "cli.py").write_text("import json\nfrom pathlib import Path\nprint(json.loads(Path(__file__).with_name('built.json').read_text())['version'])\n", encoding="utf-8")
    (root / "build.py").write_text("from pathlib import Path\nr=Path(__file__).parent\nr.joinpath('built.json').write_bytes(r.joinpath('source.json').read_bytes())\n", encoding="utf-8")
    (root / "README.md").write_text("# Fixture CLI\n\nBuild with `python build.py`; the product entry is `python cli.py`. Source version must be reflected in the built CLI.\n", encoding="utf-8")


def original_failure(record):
    return record["status"] == "exited" and record["exit_code"] == 1 and "ENOBUFS" in Path(record["stderr"]["path"]).read_text(encoding="utf-8", errors="replace")


def regression(consumer, evidence):
    node = shutil.which("node")
    assert node
    wrapper = consumer / "tools/run-focused-test.ts"
    identity = hashlib.sha256(wrapper.read_bytes()).hexdigest()
    attempts = []
    for name, code in (
        ("original", "import fs from 'node:fs'; fs.writeSync(1,Buffer.alloc(2097152,97)); fs.writeSync(2,Buffer.alloc(2097152,98)); console.log('OK: noisy child');"),
        ("reduced", "import fs from 'node:fs'; fs.writeSync(1,Buffer.alloc(1572864,97));"),
        ("wrong", "throw new SyntaxError('different failure');")):
        target = consumer / "tools" / f"outcome-{name}.mjs"
        target.write_text(code, encoding="utf-8")
        direct = execute([node, str(target)], consumer, evidence, name + "-reference")
        wrapped = execute([node, str(wrapper), str(target)], consumer, evidence, name + "-wrapper")
        preserved = original_failure(wrapped) and direct["exit_code"] == 0
        assert preserved == (name != "wrong"), (name, direct, wrapped)
        attempts.append(dict(name=name, reference=direct, failing=wrapped, original_failure_preserved=preserved))
    assert hashlib.sha256(wrapper.read_bytes()).hexdigest() == identity
    return dict(wrapper_sha256=identity, reference="same child via Node directly; no known-good wrapper revision asserted", attempts=attempts)


def focused_preparation(inputs: Path, evidence: Path) -> dict[str, Any]:
    """Exercise private preparation and one real exported case, never the full suite."""
    import importlib.util
    spec = importlib.util.spec_from_file_location("outcome_cases", Path(__file__).resolve().parents[1] / "tools/outcome_cases.py")
    assert spec and spec.loader
    cases = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(cases)
    case_workspace, digest = cases.case_workspace, cases.digest

    workspace = evidence / "focused-copy"
    setup = case_workspace(inputs, workspace, "focused")
    (evidence / "focused-setup.json").write_text(json.dumps(setup, indent=2), encoding="utf-8")
    state = json.loads((inputs / "inputs.json").read_text(encoding="utf-8"))["opencode-kit"]
    acceptance = workspace / "acceptance"
    assert "acceptance/README.md" in setup["immutable"]
    assert "acceptance/README.md" not in setup["documents"]
    preparation = json.loads((acceptance / "focused-input-state.json").read_text(encoding="utf-8"))
    assert preparation["source_state"] == state["tree_sha256"]
    assert setup["focused_preparation"]["input_state_sha256"] == digest(acceptance / "focused-input-state.json")
    assert all(digest(acceptance / name) == expected for name, expected in preparation["files"].items())
    selector = evidence / "selected-doctor.mjs"
    selector.write_text("""import path from 'node:path';
import {pathToFileURL} from 'node:url';
const root=process.argv[2];
const {doctorTests}=await import(pathToFileURL(path.join(root,'tools/test-library/doctor.ts')).href);
const {runTests}=await import(pathToFileURL(path.join(root,'tools/test-helpers/library.ts')).href);
const selected=doctorTests.filter(t=>t.name==='doctor keeps static unattended readiness passing when campaign configuration is absent');
if(selected.length!==1) throw new Error('Exact exported doctor case is unavailable');
runTests(selected,'library-selected');
""", encoding="utf-8")
    probe = evidence / "verify-focused-preparation.ps1"
    probe.write_text(r'''param([string]$Workspace, [string]$Evidence)
$ErrorActionPreference = 'Stop'
$acceptance = Join-Path $Workspace 'acceptance'
$before = [Environment]::GetEnvironmentVariables('Process')
foreach ($script in Get-ChildItem $acceptance -Filter '*.ps1') {
    $tokens = $null; $parseErrors = $null
    [Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref]$tokens, [ref]$parseErrors) | Out-Null
    if ($parseErrors.Count) { throw ($parseErrors | Out-String) }
}
. (Join-Path $acceptance 'focused-prepare.ps1')
$production = New-FocusedRequest -Workspace $Workspace
if (($production.arguments -join '|') -cne 'tools/run-focused-test.ts|tools/test-library.ts') { throw 'Native entrypoint drift' }
if ($production.timeoutSeconds -ne 600 -or $production.memoryLimitMiB -ne 2048) { throw 'Envelope drift' }
if ($production.environment.ContainsKey('CODEX_HOME')) { throw 'Must not override Codex HOME' }
$production | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Evidence 'production-request.json')
$request = New-FocusedRequest -Workspace $Workspace
if ($production.stdoutPath -eq $request.stdoutPath) { throw 'Attempts must retain distinct roots' }
$after = [Environment]::GetEnvironmentVariables('Process')
if ($before.Count -ne $after.Count) { throw 'Parent environment changed' }
foreach ($key in $before.Keys) { if ($before[$key] -cne $after[$key]) { throw 'Parent environment changed' } }
# Only this independent integration probe selects a case. Acceptance has no selector.
$request.arguments = @((Join-Path $Evidence 'selected-doctor.mjs'), $Workspace)
$request.timeoutSeconds = 45
$request | ConvertTo-Json -Depth 6 | Set-Content (Join-Path $Evidence 'selected-request.json')
$result = & (Join-Path $acceptance 'opencodex-process.ps1') -RequestPath (Join-Path $Evidence 'selected-request.json') -ResultPath (Join-Path $Evidence 'selected-result.json') -PassThru
[Console]::Out.Write([IO.File]::ReadAllText($request.stdoutPath))
[Console]::Error.Write([IO.File]::ReadAllText($request.stderrPath))
if ($result.Status -ne 'exited' -or $result.ExitCode -ne 0) { exit 1 }
''', encoding="utf-8")
    pwsh = shutil.which("pwsh")
    assert pwsh, "PowerShell prerequisite is required"
    run = execute([pwsh, "-NoProfile", "-File", str(probe), "-Workspace", str(workspace),
                   "-Evidence", str(evidence)], workspace, evidence, "focused-selected", timeout=90)
    assert run["status"] == "exited" and run["exit_code"] == 0, run
    assert Path(run["stdout"]["path"]).read_text().splitlines()[-1] == "OK: library-selected tests=1"
    assert Path(run["stderr"]["path"]).stat().st_size == 0
    assert all(digest(workspace / name) == expected for name, expected in state["files"].items())
    assert all(digest(workspace / name) == expected for name, expected in setup["immutable"].items())
    return {"selected": run, "source_state": state["tree_sha256"], "source_files_verified": len(state["files"]),
            "preparation": setup["focused_preparation"], "full_suite_rerun": False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--consumer", type=Path, help="Owned snapshot, never the live source")
    mode.add_argument("--focused-inputs", type=Path, help="Frozen inputs root; prepare a copy and run only the selected doctor case")
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    if args.focused_inputs:
        evidence = (args.evidence or Path(tempfile.mkdtemp(prefix="harness-focused-integration-"))).resolve()
        evidence.mkdir(parents=True, exist_ok=True)
        report = focused_preparation(args.focused_inputs.resolve(), evidence)
        (evidence / "focused-report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(f"PASS: immutable focused preparation, private subprocess environment, selected real doctor case. Evidence: {evidence}")
        return
    # Explicit ownership gate for this machine-local acceptance entry.
    consumer = args.consumer.resolve()
    if not consumer.is_relative_to(Path(tempfile.gettempdir()).resolve()):
        raise SystemExit("Use an owned temporary snapshot")
    evidence = args.evidence or Path(tempfile.mkdtemp(prefix="harness-outcome-oracles-"))
    evidence.mkdir(parents=True, exist_ok=True)
    root = evidence / "entrypoint"
    make_entrypoint(root)
    stale = execute([sys.executable, "cli.py"], root, evidence, "stale")
    assert Path(stale["stdout"]["path"]).read_text().strip() == "1"
    build = execute([sys.executable, "build.py"], root, evidence, "build")
    assert build["exit_code"] == 0
    fresh = execute([sys.executable, "cli.py"], root, evidence, "fresh")
    assert Path(fresh["stdout"]["path"]).read_text().strip() == "2"
    report = dict(entrypoint=dict(stale=stale, build=build, fresh=fresh), regression=regression(consumer, evidence))
    (evidence / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(f"PASS: stale CLI, actual rebuilt entrypoint, external ENOBUFS reproduction/reduction and wrong-failure rejection. Evidence: {evidence}")


if __name__ == "__main__":
    main()
